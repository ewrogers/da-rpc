#![cfg_attr(not(windows), allow(dead_code))]

use darpc_game_client::{
    ABILITY_SLOT_COUNT, INVENTORY_SLOT_COUNT, RawInventory, RawInventoryItem, RawSkill,
    RawSkillbook, RawSpell, RawSpellbook, RawStateSnapshot,
};
use darpc_model::{
    CollectionBatch, CollectionChange, CollectionKind, CooldownStatus, InventoryItem,
    InventoryUpdate, Skill, SkillbookUpdate, Spell, SpellTargetType, SpellbookUpdate, StateUpdate,
};

mod convert;
mod cooldown;

use crate::wrapping_time::deadline_reached;
use convert::trim_ascii;
pub(crate) use convert::{inventory_item, skill_model, spell};
use cooldown::CooldownWatch;

const MAX_INVENTORY_CHANGES: usize = INVENTORY_SLOT_COUNT * 2;
const MAX_ABILITY_CHANGES: usize = ABILITY_SLOT_COUNT * 2;
const SETTLE_MS: u32 = 5;

#[derive(Clone, Copy)]
struct ActionDelayTiming {
    started_at: u32,
    ends_at: u32,
}

pub(crate) struct CollectionTracker {
    inventory: RawInventory,
    skillbook: RawSkillbook,
    spellbook: RawSpellbook,
    inventory_scratch: RawInventory,
    skillbook_scratch: RawSkillbook,
    spellbook_scratch: RawSpellbook,
    inventory_dirty: [bool; INVENTORY_SLOT_COUNT],
    skillbook_dirty: [bool; ABILITY_SLOT_COUNT],
    spellbook_dirty: [bool; ABILITY_SLOT_COUNT],
    skill_cooldowns: [CooldownWatch; ABILITY_SLOT_COUNT],
    spell_cooldowns: [CooldownWatch; ABILITY_SLOT_COUNT],
    skill_action_delays: [Option<ActionDelayTiming>; ABILITY_SLOT_COUNT],
    spell_action_delays: [Option<ActionDelayTiming>; ABILITY_SLOT_COUNT],
    inventory_tick_ms: u32,
    skillbook_tick_ms: u32,
    spellbook_tick_ms: u32,
}

impl CollectionTracker {
    pub(crate) const fn new() -> Self {
        Self {
            inventory: RawInventory::empty(),
            skillbook: RawSkillbook::empty(),
            spellbook: RawSpellbook::empty(),
            inventory_scratch: RawInventory::empty(),
            skillbook_scratch: RawSkillbook::empty(),
            spellbook_scratch: RawSpellbook::empty(),
            inventory_dirty: [false; INVENTORY_SLOT_COUNT],
            skillbook_dirty: [false; ABILITY_SLOT_COUNT],
            spellbook_dirty: [false; ABILITY_SLOT_COUNT],
            skill_cooldowns: [CooldownWatch::Idle; ABILITY_SLOT_COUNT],
            spell_cooldowns: [CooldownWatch::Idle; ABILITY_SLOT_COUNT],
            skill_action_delays: [None; ABILITY_SLOT_COUNT],
            spell_action_delays: [None; ABILITY_SLOT_COUNT],
            inventory_tick_ms: 0,
            skillbook_tick_ms: 0,
            spellbook_tick_ms: 0,
        }
    }

    pub(crate) fn reset(&mut self) {
        *self = Self::new();
    }

    pub(crate) fn replace(&mut self, raw: &RawStateSnapshot, tick_ms: u32) {
        self.inventory = RawInventory::empty();
        self.skillbook = RawSkillbook::empty();
        self.spellbook = RawSpellbook::empty();
        if raw.character_available {
            if raw.character.inventory_available {
                self.inventory = raw.character.inventory;
            }
            if raw.character.skillbook_available {
                self.skillbook = raw.character.skillbook;
            }
            if raw.character.spellbook_available {
                self.spellbook = raw.character.spellbook;
            }
        }
        self.inventory_dirty.fill(false);
        self.skillbook_dirty.fill(false);
        self.spellbook_dirty.fill(false);
        self.skill_cooldowns.fill(CooldownWatch::Idle);
        self.spell_cooldowns.fill(CooldownWatch::Idle);
        reconcile_skill_cooldowns(
            &mut self.skill_cooldowns,
            &mut self.skill_action_delays,
            &self.skillbook.skills,
            &[true; ABILITY_SLOT_COUNT],
            tick_ms,
        );
        reconcile_spell_cooldowns(
            &mut self.spell_cooldowns,
            &mut self.spell_action_delays,
            &self.spellbook.spells,
            &[true; ABILITY_SLOT_COUNT],
            tick_ms,
        );
    }

    pub(crate) fn mark(&mut self, kind: CollectionKind, slot: u8, tick_ms: u32) {
        let Some(index) = usize::from(slot).checked_sub(1) else {
            return;
        };
        match kind {
            CollectionKind::Inventory if index < INVENTORY_SLOT_COUNT => {
                self.inventory_dirty[index] = true;
                self.inventory_tick_ms = tick_ms;
            }
            CollectionKind::Spellbook if index < ABILITY_SLOT_COUNT => {
                self.spellbook_dirty[index] = true;
                self.spellbook_tick_ms = tick_ms;
            }
            CollectionKind::Skillbook if index < ABILITY_SLOT_COUNT => {
                self.skillbook_dirty[index] = true;
                self.skillbook_tick_ms = tick_ms;
            }
            _ => {}
        }
    }

    pub(crate) fn watch_cooldown(&mut self, kind: CollectionKind, slot: u8, tick_ms: u32) {
        let Some(index) = usize::from(slot).checked_sub(1) else {
            return;
        };
        match kind {
            CollectionKind::Spellbook if index < ABILITY_SLOT_COUNT => {
                self.spell_cooldowns[index] = CooldownWatch::start(tick_ms, SETTLE_MS);
                self.mark(kind, slot, tick_ms);
            }
            CollectionKind::Skillbook if index < ABILITY_SLOT_COUNT => {
                self.skill_cooldowns[index] = CooldownWatch::start(tick_ms, SETTLE_MS);
                self.mark(kind, slot, tick_ms);
            }
            _ => {}
        }
    }

    pub(crate) fn observe_action_delay(
        &mut self,
        kind: CollectionKind,
        slot: u8,
        duration_ms: Option<u32>,
        tick_ms: u32,
    ) {
        self.watch_cooldown(kind, slot, tick_ms);
        let Some(index) = usize::from(slot).checked_sub(1) else {
            return;
        };
        if index < ABILITY_SLOT_COUNT
            && let Some(duration_ms) = duration_ms
        {
            let timing = Some(ActionDelayTiming {
                started_at: tick_ms,
                ends_at: tick_ms.wrapping_add(duration_ms),
            });
            match kind {
                CollectionKind::Skillbook => self.skill_action_delays[index] = timing,
                CollectionKind::Spellbook => self.spell_action_delays[index] = timing,
                _ => {}
            }
        }
    }

    pub(crate) fn merge_snapshot(&self, raw: &mut RawStateSnapshot, tick_ms: u32) {
        if raw.character_available && raw.character.spellbook_available {
            apply_spell_action_delays(
                &mut raw.character.spellbook,
                &self.spell_action_delays,
                tick_ms,
            );
        }
        if raw.character_available && raw.character.skillbook_available {
            apply_skill_action_delays(
                &mut raw.character.skillbook,
                &self.skill_action_delays,
                tick_ms,
            );
        }
    }

    #[cfg(windows)]
    pub(crate) fn observe_tick(
        &mut self,
        current_tick_ms: u32,
        mut emit: impl FnMut(QueuedCollectionUpdate, u32),
    ) {
        schedule_cooldown_polls(
            &mut self.skill_cooldowns,
            &mut self.skillbook_dirty,
            &mut self.skillbook_tick_ms,
            current_tick_ms,
        );
        schedule_cooldown_polls(
            &mut self.spell_cooldowns,
            &mut self.spellbook_dirty,
            &mut self.spellbook_tick_ms,
            current_tick_ms,
        );
        if collection_ready(
            &self.inventory_dirty,
            self.inventory_tick_ms,
            current_tick_ms,
        ) && matches!(
            crate::snapshot::capture_inventory(&mut self.inventory_scratch),
            Ok(true)
        ) {
            emit_updates::<_, _, INVENTORY_SLOT_COUNT, MAX_INVENTORY_CHANGES>(
                &self.inventory.items,
                &self.inventory_scratch.items,
                &self.inventory_dirty,
                QueuedCollectionUpdate::Inventory,
                &mut emit,
                self.inventory_tick_ms,
            );
            replace_dirty(
                &mut self.inventory.items,
                &self.inventory_scratch.items,
                &mut self.inventory_dirty,
            );
        }

        let skills_ready = collection_ready(
            &self.skillbook_dirty,
            self.skillbook_tick_ms,
            current_tick_ms,
        );
        let spells_ready = collection_ready(
            &self.spellbook_dirty,
            self.spellbook_tick_ms,
            current_tick_ms,
        );
        if (skills_ready || spells_ready)
            && let Ok((skills_available, spells_available)) = crate::snapshot::capture_abilities(
                &mut self.skillbook_scratch,
                &mut self.spellbook_scratch,
            )
        {
            if skills_available && skills_ready {
                apply_skill_action_delays(
                    &mut self.skillbook_scratch,
                    &self.skill_action_delays,
                    current_tick_ms,
                );
                emit_updates::<_, _, ABILITY_SLOT_COUNT, MAX_ABILITY_CHANGES>(
                    &self.skillbook.skills,
                    &self.skillbook_scratch.skills,
                    &self.skillbook_dirty,
                    QueuedCollectionUpdate::Skillbook,
                    &mut emit,
                    self.skillbook_tick_ms,
                );
                reconcile_skill_cooldowns(
                    &mut self.skill_cooldowns,
                    &mut self.skill_action_delays,
                    &self.skillbook_scratch.skills,
                    &self.skillbook_dirty,
                    current_tick_ms,
                );
                replace_dirty(
                    &mut self.skillbook.skills,
                    &self.skillbook_scratch.skills,
                    &mut self.skillbook_dirty,
                );
            }
            if spells_available && spells_ready {
                apply_spell_action_delays(
                    &mut self.spellbook_scratch,
                    &self.spell_action_delays,
                    current_tick_ms,
                );
                emit_updates::<_, _, ABILITY_SLOT_COUNT, MAX_ABILITY_CHANGES>(
                    &self.spellbook.spells,
                    &self.spellbook_scratch.spells,
                    &self.spellbook_dirty,
                    QueuedCollectionUpdate::Spellbook,
                    &mut emit,
                    self.spellbook_tick_ms,
                );
                reconcile_spell_cooldowns(
                    &mut self.spell_cooldowns,
                    &mut self.spell_action_delays,
                    &self.spellbook_scratch.spells,
                    &self.spellbook_dirty,
                    current_tick_ms,
                );
                replace_dirty(
                    &mut self.spellbook.spells,
                    &self.spellbook_scratch.spells,
                    &mut self.spellbook_dirty,
                );
            }
        }
    }

    #[cfg(not(windows))]
    pub(crate) fn observe_tick(
        &mut self,
        _current_tick_ms: u32,
        _emit: impl FnMut(QueuedCollectionUpdate, u32),
    ) {
    }
}

fn schedule_cooldown_polls<const N: usize>(
    watches: &mut [CooldownWatch; N],
    dirty: &mut [bool; N],
    marked_tick_ms: &mut u32,
    now_ms: u32,
) {
    let mut scheduled = false;
    for (watch, dirty) in watches.iter().zip(dirty.iter_mut()) {
        if !*dirty && watch.due(now_ms) {
            *dirty = true;
            scheduled = true;
        }
    }
    if scheduled {
        *marked_tick_ms = now_ms;
    }
}

fn reconcile_skill_cooldowns(
    watches: &mut [CooldownWatch; ABILITY_SLOT_COUNT],
    timings: &mut [Option<ActionDelayTiming>; ABILITY_SLOT_COUNT],
    skills: &[Option<RawSkill>; ABILITY_SLOT_COUNT],
    observed: &[bool; ABILITY_SLOT_COUNT],
    now_ms: u32,
) {
    for (index, watch) in watches
        .iter_mut()
        .enumerate()
        .filter(|(index, _)| observed[*index])
    {
        let skill = skills[index];
        let active =
            skill.is_some_and(|skill| skill.cooldown_visual_active || skill.action_delay_active);
        let exact_end_ms = skill
            .filter(|skill| skill.cooldown_visual_active)
            .map(|skill| skill.cooldown_ends_at);
        *watch = watch.observed(active, exact_end_ms, now_ms);
        if !active && matches!(*watch, CooldownWatch::Idle) {
            timings[index] = None;
        }
    }
}

fn apply_skill_action_delays(
    skillbook: &mut RawSkillbook,
    timings: &[Option<ActionDelayTiming>; ABILITY_SLOT_COUNT],
    now_ms: u32,
) {
    for (skill, timing) in skillbook.skills.iter_mut().zip(timings) {
        let Some(skill) = skill else {
            continue;
        };
        skill.action_delay_timing_available = false;
        let Some(timing) = timing.filter(|timing| {
            (skill.cooldown_visual_active || skill.action_delay_active)
                && !deadline_reached(now_ms, timing.ends_at)
        }) else {
            continue;
        };
        skill.action_delay_duration_ms = timing.ends_at.wrapping_sub(timing.started_at);
        skill.action_delay_timing_available = true;
    }
}

fn reconcile_spell_cooldowns(
    watches: &mut [CooldownWatch; ABILITY_SLOT_COUNT],
    timings: &mut [Option<ActionDelayTiming>; ABILITY_SLOT_COUNT],
    spells: &[Option<RawSpell>; ABILITY_SLOT_COUNT],
    observed: &[bool; ABILITY_SLOT_COUNT],
    now_ms: u32,
) {
    for (index, watch) in watches
        .iter_mut()
        .enumerate()
        .filter(|(index, _)| observed[*index])
    {
        let timing = timings[index];
        let timing_active = timing.is_some_and(|timing| !deadline_reached(now_ms, timing.ends_at));
        let active = timing.map_or_else(
            || spells[index].is_some_and(|spell| spell.action_delay_active),
            |_| timing_active,
        );
        let exact_end_ms = timing
            .filter(|_| timing_active)
            .map(|timing| timing.ends_at);
        *watch = watch.observed(active, exact_end_ms, now_ms);
        if !active && matches!(*watch, CooldownWatch::Idle) {
            timings[index] = None;
        }
    }
}

fn apply_spell_action_delays(
    spellbook: &mut RawSpellbook,
    timings: &[Option<ActionDelayTiming>; ABILITY_SLOT_COUNT],
    now_ms: u32,
) {
    for (spell, timing) in spellbook.spells.iter_mut().zip(timings) {
        let Some(spell) = spell else {
            continue;
        };
        spell.action_delay_timing_available = false;
        let Some(timing) = timing else {
            continue;
        };
        spell.action_delay_active = !deadline_reached(now_ms, timing.ends_at);
        if !spell.action_delay_active {
            continue;
        }
        spell.action_delay_started_at = timing.started_at;
        spell.action_delay_ends_at = timing.ends_at;
        spell.action_delay_timing_available = true;
    }
}

fn collection_ready<const N: usize>(dirty: &[bool; N], marked_tick_ms: u32, now: u32) -> bool {
    dirty.iter().any(|dirty| *dirty) && now.wrapping_sub(marked_tick_ms) >= SETTLE_MS
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
// Collection events retain fixed, pointer-free before/after values so the game
// thread never allocates. The size difference is deliberate.
#[allow(clippy::large_enum_variant)]
pub(crate) enum QueuedCollectionUpdate {
    Inventory(QueuedSlotUpdate<RawInventoryItem>),
    Spellbook(QueuedSlotUpdate<RawSpell>),
    Skillbook(QueuedSlotUpdate<RawSkill>),
}

impl QueuedCollectionUpdate {
    pub(crate) const fn kind(self) -> CollectionKind {
        match self {
            Self::Inventory(_) => CollectionKind::Inventory,
            Self::Spellbook(_) => CollectionKind::Spellbook,
            Self::Skillbook(_) => CollectionKind::Skillbook,
        }
    }

    pub(crate) fn batch(self) -> CollectionBatch {
        match self {
            Self::Inventory(update) => update.batch(),
            Self::Spellbook(update) => update.batch(),
            Self::Skillbook(update) => update.batch(),
        }
    }

    pub(crate) fn into_model(self, tick_ms: u32) -> StateUpdate {
        match self {
            Self::Inventory(update) => StateUpdate::Inventory(InventoryUpdate {
                batch_index: update.batch_index,
                batch_count: update.batch_count,
                change: update.change,
                slot: update.slot,
                before: update.before.map(inventory_item),
                after: update.after.map(inventory_item),
            }),
            Self::Spellbook(update) => StateUpdate::Spellbook(SpellbookUpdate {
                batch_index: update.batch_index,
                batch_count: update.batch_count,
                change: update.change,
                slot: update.slot,
                before: update.before.map(|raw| spell(raw, tick_ms)),
                after: update.after.map(|raw| spell(raw, tick_ms)),
            }),
            Self::Skillbook(update) => StateUpdate::Skillbook(SkillbookUpdate {
                batch_index: update.batch_index,
                batch_count: update.batch_count,
                change: update.change,
                slot: update.slot,
                before: update.before.map(|skill| skill_model(skill, tick_ms)),
                after: update.after.map(|skill| skill_model(skill, tick_ms)),
            }),
        }
    }

    #[cfg(test)]
    pub(crate) fn test_inventory_batch(batch_index: u8, batch_count: u8) -> Self {
        Self::Inventory(QueuedSlotUpdate {
            batch_index,
            batch_count,
            change: CollectionChange::Changed,
            slot: batch_index.saturating_add(1),
            before: None,
            after: Some(RawInventoryItem {
                slot: batch_index.saturating_add(1),
                sprite: 21,
                dye_color: 2,
                name: darpc_game_client::RawClientText {
                    bytes: [0; 128],
                    length: 0,
                },
                quantity: 1,
                can_stack: false,
                durability: 900,
                max_durability: 1_000,
            }),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct QueuedSlotUpdate<T> {
    batch_index: u8,
    batch_count: u8,
    change: CollectionChange,
    slot: u8,
    before: Option<T>,
    after: Option<T>,
}

impl<T> QueuedSlotUpdate<T> {
    fn batch(self) -> CollectionBatch {
        CollectionBatch {
            index: self.batch_index,
            count: self.batch_count,
        }
    }
}

#[derive(Clone, Copy)]
struct PendingUpdate<T> {
    change: CollectionChange,
    slot: u8,
    before: Option<T>,
    after: Option<T>,
}

fn emit_updates<T, U, const N: usize, const MAX: usize>(
    before: &[Option<T>; N],
    after: &[Option<T>; N],
    dirty: &[bool; N],
    wrap: impl Fn(QueuedSlotUpdate<T>) -> U + Copy,
    emit: &mut impl FnMut(U, u32),
    tick_ms: u32,
) where
    T: CollectionValue,
{
    let count = (0..N)
        .filter(|index| dirty[*index])
        .map(|index| {
            pending_updates(index, before, after)
                .iter()
                .flatten()
                .count()
        })
        .sum::<usize>();
    if count == 0 {
        return;
    }
    debug_assert!(count <= MAX);
    let batch_count = u8::try_from(count).expect("collection change batch fits u8");
    let mut batch_index = 0_u8;
    for index in (0..N).filter(|index| dirty[*index]) {
        for pending in pending_updates(index, before, after).into_iter().flatten() {
            emit(
                wrap(QueuedSlotUpdate {
                    batch_index,
                    batch_count,
                    change: pending.change,
                    slot: pending.slot,
                    before: pending.before,
                    after: pending.after,
                }),
                tick_ms,
            );
            batch_index += 1;
        }
    }
}

fn pending_updates<T: CollectionValue, const N: usize>(
    index: usize,
    before: &[Option<T>; N],
    after: &[Option<T>; N],
) -> [Option<PendingUpdate<T>>; 2] {
    let previous = before[index];
    let current = after[index];
    if previous == current {
        return [None, None];
    }
    let slot = u8::try_from(index + 1).expect("collection slot fits u8");
    match (previous, current) {
        (None, Some(current)) => [
            Some(PendingUpdate {
                change: arrival(current, before, after),
                slot,
                before: None,
                after: Some(current),
            }),
            None,
        ],
        (Some(previous), None) => [
            Some(PendingUpdate {
                change: departure(previous, before, after),
                slot,
                before: Some(previous),
                after: None,
            }),
            None,
        ],
        (Some(previous), Some(current)) if previous.same_identity(current) => [
            Some(PendingUpdate {
                change: quantity_change(current, before, after),
                slot,
                before: Some(previous),
                after: Some(current),
            }),
            None,
        ],
        (Some(previous), Some(current)) => {
            let departure = departure(previous, before, after);
            let arrival = arrival(current, before, after);
            if departure == CollectionChange::Changed && arrival == CollectionChange::Changed {
                [
                    Some(PendingUpdate {
                        change: CollectionChange::Changed,
                        slot,
                        before: Some(previous),
                        after: Some(current),
                    }),
                    None,
                ]
            } else {
                [
                    Some(PendingUpdate {
                        change: departure,
                        slot,
                        before: Some(previous),
                        after: None,
                    }),
                    Some(PendingUpdate {
                        change: arrival,
                        slot,
                        before: None,
                        after: Some(current),
                    }),
                ]
            }
        }
        (None, None) => [None, None],
    }
}

fn arrival<T: CollectionValue, const N: usize>(
    value: T,
    before: &[Option<T>; N],
    after: &[Option<T>; N],
) -> CollectionChange {
    if total(value, after) > total(value, before) {
        CollectionChange::Added
    } else {
        CollectionChange::Changed
    }
}

fn departure<T: CollectionValue, const N: usize>(
    value: T,
    before: &[Option<T>; N],
    after: &[Option<T>; N],
) -> CollectionChange {
    if total(value, after) < total(value, before) {
        CollectionChange::Removed
    } else {
        CollectionChange::Changed
    }
}

fn quantity_change<T: CollectionValue, const N: usize>(
    value: T,
    before: &[Option<T>; N],
    after: &[Option<T>; N],
) -> CollectionChange {
    match total(value, after).cmp(&total(value, before)) {
        core::cmp::Ordering::Greater => CollectionChange::Added,
        core::cmp::Ordering::Less => CollectionChange::Removed,
        core::cmp::Ordering::Equal => CollectionChange::Changed,
    }
}

fn total<T: CollectionValue, const N: usize>(value: T, items: &[Option<T>; N]) -> u64 {
    items
        .iter()
        .flatten()
        .copied()
        .filter(|item| value.same_identity(*item))
        .map(CollectionValue::amount)
        .sum()
}

fn replace_dirty<T: Copy, const N: usize>(
    current: &mut [Option<T>; N],
    captured: &[Option<T>; N],
    dirty: &mut [bool; N],
) {
    for index in 0..N {
        if dirty[index] {
            current[index] = captured[index];
            dirty[index] = false;
        }
    }
}

trait CollectionValue: Copy + Eq {
    fn same_identity(self, other: Self) -> bool;
    fn amount(self) -> u64;
}

impl CollectionValue for RawInventoryItem {
    fn same_identity(self, other: Self) -> bool {
        self.sprite & 0x3FFF == other.sprite & 0x3FFF
            && self.dye_color == other.dye_color
            && self.can_stack == other.can_stack
            && self.max_durability == other.max_durability
            && inventory_identity_name(&self) == inventory_identity_name(&other)
    }

    fn amount(self) -> u64 {
        u64::from(self.quantity.max(1))
    }
}

fn inventory_identity_name(item: &RawInventoryItem) -> &[u8] {
    let name = trim_ascii(&item.name.bytes[..usize::from(item.name.length)]);
    if !item.can_stack || name.last() != Some(&b']') {
        return name;
    }
    let Some(open) = name.iter().rposition(|byte| *byte == b'[') else {
        return name;
    };
    let count = trim_ascii(&name[open + 1..name.len() - 1]);
    let parsed = count.iter().try_fold(0_u32, |value, byte| {
        byte.is_ascii_digit()
            .then_some(u32::from(*byte - b'0'))
            .and_then(|digit| value.checked_mul(10)?.checked_add(digit))
    });
    if parsed == Some(item.quantity) {
        trim_ascii(&name[..open])
    } else {
        name
    }
}

impl CollectionValue for RawSpell {
    fn same_identity(self, other: Self) -> bool {
        self.icon == other.icon
            && ability_identity(&self.name.bytes, self.name.length, self.base_name_length)
                == ability_identity(&other.name.bytes, other.name.length, other.base_name_length)
    }

    fn amount(self) -> u64 {
        1
    }
}

impl CollectionValue for RawSkill {
    fn same_identity(self, other: Self) -> bool {
        self.icon == other.icon
            && ability_identity(&self.name.bytes, self.name.length, self.base_name_length)
                == ability_identity(&other.name.bytes, other.name.length, other.base_name_length)
    }

    fn amount(self) -> u64 {
        1
    }
}

fn ability_identity(bytes: &[u8; 128], length: u8, base_name_length: i32) -> &[u8] {
    let name = &bytes[..usize::from(length)];
    if let Ok(length) = usize::try_from(base_name_length)
        && (1..=name.len()).contains(&length)
    {
        return trim_ascii(&name[..length]);
    }
    name.windows(5)
        .position(|window| window == b"(Lev:")
        .map(|marker| trim_ascii(&name[..marker]))
        .unwrap_or_else(|| trim_ascii(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use darpc_game_client::RawClientText;

    #[test]
    fn waits_for_the_bounded_settling_window_after_the_last_slot_packet() {
        let dirty = [true];
        assert!(!collection_ready(&dirty, 100, 104));
        assert!(collection_ready(&dirty, 100, 105));
        assert!(!collection_ready(&[false], 100, 110));
        assert!(collection_ready(&dirty, u32::MAX - 2, 2));
    }

    #[test]
    fn ignores_an_identical_same_slot_update() {
        let mut before = [None; INVENTORY_SLOT_COUNT];
        let mut after = [None; INVENTORY_SLOT_COUNT];
        before[0] = Some(item(1, 10, 1));
        after[0] = before[0];
        assert!(inventory_updates(&before, &after, &[true; INVENTORY_SLOT_COUNT]).is_empty());
    }

    #[test]
    fn treats_moves_and_swaps_as_slot_changes() {
        let mut before = [None; INVENTORY_SLOT_COUNT];
        let mut moved = [None; INVENTORY_SLOT_COUNT];
        before[0] = Some(item(1, 10, 1));
        moved[1] = Some(item(2, 10, 1));
        let updates = inventory_updates(&before, &moved, &[true; INVENTORY_SLOT_COUNT]);
        assert_eq!(updates.len(), 2);
        assert!(
            updates
                .iter()
                .all(|update| update.change == CollectionChange::Changed)
        );
        assert_eq!((updates[0].slot, updates[1].slot), (1, 2));

        before[1] = Some(item(2, 20, 1));
        let mut swapped = [None; INVENTORY_SLOT_COUNT];
        swapped[0] = Some(item(1, 20, 1));
        swapped[1] = Some(item(2, 10, 1));
        let updates = inventory_updates(&before, &swapped, &[true; INVENTORY_SLOT_COUNT]);
        assert_eq!(updates.len(), 2);
        assert!(
            updates
                .iter()
                .all(|update| update.change == CollectionChange::Changed)
        );
        assert!(
            updates
                .iter()
                .all(|update| update.before.is_some() && update.after.is_some())
        );
    }

    #[test]
    fn classifies_add_remove_and_replacement() {
        let empty = [None; INVENTORY_SLOT_COUNT];
        let mut added = empty;
        added[0] = Some(item(1, 10, 1));
        let updates = inventory_updates(&empty, &added, &[true; INVENTORY_SLOT_COUNT]);
        assert_eq!(updates[0].change, CollectionChange::Added);

        let updates = inventory_updates(&added, &empty, &[true; INVENTORY_SLOT_COUNT]);
        assert_eq!(updates[0].change, CollectionChange::Removed);

        let mut replaced = empty;
        replaced[0] = Some(item(1, 20, 1));
        let updates = inventory_updates(&added, &replaced, &[true; INVENTORY_SLOT_COUNT]);
        assert_eq!(updates.len(), 2);
        assert_eq!(updates[0].change, CollectionChange::Removed);
        assert_eq!(updates[1].change, CollectionChange::Added);
        assert_eq!((updates[0].batch_index, updates[1].batch_index), (0, 1));
        assert!(updates.iter().all(|update| update.batch_count == 2));
    }

    #[test]
    fn uses_total_stack_quantity_to_distinguish_transfer_from_gain() {
        let mut before = [None; INVENTORY_SLOT_COUNT];
        before[0] = Some(item(1, 10, 3));
        let mut increased = before;
        increased[0] = Some(item(1, 10, 4));
        assert_eq!(
            inventory_updates(&before, &increased, &[true; INVENTORY_SLOT_COUNT])[0].change,
            CollectionChange::Added
        );

        let mut split = [None; INVENTORY_SLOT_COUNT];
        split[0] = Some(item(1, 10, 1));
        split[1] = Some(item(2, 10, 2));
        let updates = inventory_updates(&before, &split, &[true; INVENTORY_SLOT_COUNT]);
        assert_eq!(updates.len(), 2);
        assert!(
            updates
                .iter()
                .all(|update| update.change == CollectionChange::Changed)
        );
    }

    #[test]
    fn stack_count_suffix_does_not_change_item_identity() {
        let mut before = [None; INVENTORY_SLOT_COUNT];
        before[0] = Some(item_with_name(1, 10, 3, b"Dark Belt [ 3 ]"));
        let mut after = before;
        after[0] = Some(item_with_name(1, 10, 4, b"Dark Belt [ 4 ]"));

        let updates = inventory_updates(&before, &after, &[true; INVENTORY_SLOT_COUNT]);
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].change, CollectionChange::Added);
        assert!(updates[0].before.is_some());
        assert!(updates[0].after.is_some());
    }

    #[test]
    fn conversion_keeps_canonical_names_and_ascii_prompts() {
        let converted = inventory_item(item_with_name(1, 10, 3, b"Dark Belt [ 3 ]"));
        assert_eq!(converted.name.as_deref(), Some("Dark Belt"));

        let spell = spell(
            RawSpell {
                slot: 1,
                icon: 20,
                name: text(b"Fas Spiorad (Lev:3/5)"),
                argument_type: 1,
                prompt: Some(text(b"Target \xFFname?")),
                cast_lines: 4,
                action_delay_active: false,
                action_delay_started_at: 0,
                action_delay_ends_at: 0,
                action_delay_timing_available: false,
                name_suffix_left: 0,
                base_name_length: 0,
            },
            100,
        );
        assert_eq!(spell.name.as_deref(), Some("Fas Spiorad"));
        assert_eq!(spell.level, 3);
        assert_eq!(spell.max_level, 5);
        assert_eq!(spell.prompt.as_deref(), Some("Target name?"));
    }

    #[test]
    fn skill_remaining_time_does_not_exceed_total_duration() {
        let skill = skill_model(
            RawSkill {
                slot: 11,
                icon: 91,
                name: text(b"Throw"),
                cooldown_started_at: 1_006,
                cooldown_ends_at: 46_006,
                cooldown_visual_active: true,
                action_delay_active: false,
                action_delay_duration_ms: 0,
                action_delay_timing_available: false,
                name_suffix_left: 0,
                base_name_length: 0,
            },
            1_000,
        );

        assert_eq!(skill.cooldown.cooldown_ms, Some(45_000));
        assert_eq!(skill.cooldown.remaining_ms, Some(45_000));
    }

    #[test]
    fn used_skill_is_polled_at_start_and_exact_expiry() {
        let mut tracker = CollectionTracker::new();
        tracker.watch_cooldown(CollectionKind::Skillbook, 3, 100);
        assert!(tracker.skillbook_dirty[2]);

        let mut skills = [None; ABILITY_SLOT_COUNT];
        skills[2] = Some(RawSkill {
            slot: 3,
            icon: 91,
            name: text(b"Assail"),
            cooldown_started_at: 105,
            cooldown_ends_at: 1_000,
            cooldown_visual_active: true,
            action_delay_active: false,
            action_delay_duration_ms: 0,
            action_delay_timing_available: false,
            name_suffix_left: 0,
            base_name_length: 0,
        });
        reconcile_skill_cooldowns(
            &mut tracker.skill_cooldowns,
            &mut tracker.skill_action_delays,
            &skills,
            &tracker.skillbook_dirty,
            105,
        );
        tracker.skillbook_dirty.fill(false);

        schedule_cooldown_polls(
            &mut tracker.skill_cooldowns,
            &mut tracker.skillbook_dirty,
            &mut tracker.skillbook_tick_ms,
            999,
        );
        assert!(!tracker.skillbook_dirty[2]);
        schedule_cooldown_polls(
            &mut tracker.skill_cooldowns,
            &mut tracker.skillbook_dirty,
            &mut tracker.skillbook_tick_ms,
            1_000,
        );
        assert!(tracker.skillbook_dirty[2]);
        assert_eq!(tracker.skillbook_tick_ms, 1_000);
    }

    #[test]
    fn active_spell_without_exact_expiry_uses_targeted_polling() {
        let mut watches = [CooldownWatch::Idle; ABILITY_SLOT_COUNT];
        let mut timings = [None; ABILITY_SLOT_COUNT];
        let mut spells = [None; ABILITY_SLOT_COUNT];
        let mut observed = [false; ABILITY_SLOT_COUNT];
        spells[3] = Some(RawSpell {
            slot: 4,
            icon: 82,
            name: text(b"beag srad"),
            argument_type: 0,
            prompt: None,
            cast_lines: 0,
            action_delay_active: true,
            action_delay_started_at: 0,
            action_delay_ends_at: 0,
            action_delay_timing_available: false,
            name_suffix_left: 0,
            base_name_length: 0,
        });
        observed[3] = true;
        reconcile_spell_cooldowns(&mut watches, &mut timings, &spells, &observed, 200);
        assert!(!watches[3].due(224));
        assert!(watches[3].due(225));
        assert_eq!(watches[2], CooldownWatch::Idle);
    }

    #[test]
    fn action_delay_timing_populates_spell_total_and_remaining_time() {
        let mut spellbook = RawSpellbook::empty();
        spellbook.spells[37] = Some(RawSpell {
            slot: 38,
            icon: 173,
            name: text(b"Mud Wall"),
            argument_type: 0,
            prompt: None,
            cast_lines: 0,
            action_delay_active: true,
            action_delay_started_at: 0,
            action_delay_ends_at: 0,
            action_delay_timing_available: false,
            name_suffix_left: 0,
            base_name_length: 0,
        });
        let mut timings = [None; ABILITY_SLOT_COUNT];
        timings[37] = Some(ActionDelayTiming {
            started_at: 1_000,
            ends_at: 13_000,
        });

        apply_spell_action_delays(&mut spellbook, &timings, 2_000);
        let converted = spell(spellbook.spells[37].unwrap(), 2_000);

        assert_eq!(converted.cooldown.cooldown_ms, Some(12_000));
        assert_eq!(converted.cooldown.remaining_ms, Some(11_000));
    }

    #[test]
    fn action_delay_timing_authoritatively_completes_spell_cooldown() {
        let mut spellbook = RawSpellbook::empty();
        spellbook.spells[37] = Some(RawSpell {
            slot: 38,
            icon: 173,
            name: text(b"Mud Wall"),
            argument_type: 0,
            prompt: None,
            cast_lines: 0,
            action_delay_active: true,
            action_delay_started_at: 0,
            action_delay_ends_at: 0,
            action_delay_timing_available: false,
            name_suffix_left: 0,
            base_name_length: 0,
        });
        let mut timings = [None; ABILITY_SLOT_COUNT];
        timings[37] = Some(ActionDelayTiming {
            started_at: 1_000,
            ends_at: 13_000,
        });

        apply_spell_action_delays(&mut spellbook, &timings, 13_000);
        let raw = spellbook.spells[37].unwrap();
        assert!(!raw.action_delay_active);
        assert!(!raw.action_delay_timing_available);

        let mut watches = [CooldownWatch::Idle; ABILITY_SLOT_COUNT];
        watches[37] = CooldownWatch::Active {
            next_poll_ms: 13_000,
        };
        let mut observed = [false; ABILITY_SLOT_COUNT];
        observed[37] = true;
        reconcile_spell_cooldowns(
            &mut watches,
            &mut timings,
            &spellbook.spells,
            &observed,
            13_000,
        );
        assert_eq!(watches[37], CooldownWatch::Idle);
        assert!(timings[37].is_none());
    }

    #[test]
    fn action_delay_total_is_authoritative_for_live_skill_timing() {
        let mut skillbook = RawSkillbook::empty();
        skillbook.skills[10] = Some(RawSkill {
            slot: 11,
            icon: 91,
            name: text(b"Throw"),
            cooldown_started_at: 1_050,
            cooldown_ends_at: 46_050,
            cooldown_visual_active: true,
            action_delay_active: true,
            action_delay_duration_ms: 0,
            action_delay_timing_available: false,
            name_suffix_left: 0,
            base_name_length: 0,
        });
        let mut timings = [None; ABILITY_SLOT_COUNT];
        timings[10] = Some(ActionDelayTiming {
            started_at: 1_000,
            ends_at: 47_000,
        });

        apply_skill_action_delays(&mut skillbook, &timings, 2_000);
        let converted = skill_model(skillbook.skills[10].unwrap(), 2_000);

        assert_eq!(converted.cooldown.cooldown_ms, Some(46_000));
        assert_eq!(converted.cooldown.remaining_ms, Some(44_050));
    }

    fn inventory_updates(
        before: &[Option<RawInventoryItem>; INVENTORY_SLOT_COUNT],
        after: &[Option<RawInventoryItem>; INVENTORY_SLOT_COUNT],
        dirty: &[bool; INVENTORY_SLOT_COUNT],
    ) -> Vec<QueuedSlotUpdate<RawInventoryItem>> {
        let mut updates = Vec::new();
        emit_updates::<_, _, INVENTORY_SLOT_COUNT, MAX_INVENTORY_CHANGES>(
            before,
            after,
            dirty,
            core::convert::identity,
            &mut |update, _| updates.push(update),
            100,
        );
        updates
    }

    fn item(slot: u8, sprite: u16, quantity: u32) -> RawInventoryItem {
        item_with_name(slot, sprite, quantity, b"Item")
    }

    fn item_with_name(slot: u8, sprite: u16, quantity: u32, name: &[u8]) -> RawInventoryItem {
        RawInventoryItem {
            slot,
            sprite,
            dye_color: 0,
            name: text(name),
            quantity,
            can_stack: true,
            durability: 10,
            max_durability: 20,
        }
    }

    fn text(value: &[u8]) -> RawClientText<128> {
        let mut bytes = [0; 128];
        bytes[..value.len()].copy_from_slice(value);
        RawClientText {
            bytes,
            length: u8::try_from(value.len()).unwrap(),
        }
    }
}
