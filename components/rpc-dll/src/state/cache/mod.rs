use super::*;

mod collections;
mod objects;

pub(super) use collections::MainThreadCollections;
pub(super) use objects::MainThreadObjects;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct CachedMap {
    pub(super) id: u32,
    pub(super) width: i32,
    pub(super) height: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct CachedCast {
    pub(super) slot: u8,
    pub(super) total_lines: u8,
}

impl From<QueuedMapChange> for CachedMap {
    fn from(value: QueuedMapChange) -> Self {
        Self {
            id: value.id,
            width: value.width,
            height: value.height,
        }
    }
}

impl QueuedMapChange {
    pub(super) fn new(id: u32, width: i32, height: i32, name: &[u8]) -> Self {
        let length = name.len().min(MAX_EVENT_MAP_NAME_BYTES);
        let mut owned_name = [0; MAX_EVENT_MAP_NAME_BYTES];
        owned_name[..length].copy_from_slice(&name[..length]);
        Self {
            id,
            width,
            height,
            name_length: u8::try_from(length).expect("map name length fits u8"),
            name: owned_name,
        }
    }

    pub(super) fn into_model(self) -> MapChange {
        MapChange {
            id: self.id,
            name: decode_map_name(&self.name[..usize::from(self.name_length)]),
            width: self.width,
            height: self.height,
        }
    }
}

#[cfg(windows)]
fn decode_map_name(bytes: &[u8]) -> Option<String> {
    crate::client_text::decode(bytes)
}

#[cfg(not(windows))]
fn decode_map_name(bytes: &[u8]) -> Option<String> {
    (!bytes.is_empty()).then(|| String::from_utf8_lossy(bytes).into_owned())
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct StateCache {
    pub(super) lifecycle: Option<ClientLifecycle>,
    pub(super) self_id: Option<u32>,
    pub(super) self_name: [u8; 16],
    pub(super) self_name_len: u8,
    pub(super) self_direction: Option<u8>,
    pub(super) core: Option<CoreStatus>,
    pub(super) vitals: Option<CurrentVitals>,
    pub(super) progression: Option<ProgressionStatus>,
    pub(super) gold: Option<u32>,
    pub(super) modifiers: Option<CharacterModifiers>,
    pub(super) is_blinded: Option<bool>,
    pub(super) is_action_restricted: Option<bool>,
    pub(super) is_casting: Option<bool>,
    pub(super) is_walking: Option<bool>,
    pub(super) is_hidden: Option<bool>,
    pub(super) map: Option<CachedMap>,
    pub(super) position: Option<(i32, i32)>,
    pub(super) pending_map: Option<QueuedMapChange>,
    pub(super) active_spell: Option<CachedCast>,
    pub(super) effects: [Option<Effect>; 10],
}

impl StateCache {
    pub(super) fn from_raw(raw: &RawStateSnapshot) -> Self {
        let mut cache = if raw.character_available {
            Self::from_character(&raw.character)
        } else {
            Self::default()
        };
        cache.lifecycle = Some(super::lifecycle(raw.lifecycle));
        cache
    }

    pub(super) fn from_character(raw: &RawCharacter) -> Self {
        Self {
            lifecycle: None,
            self_id: raw.id,
            self_name: raw.name,
            self_name_len: raw.name_len,
            self_direction: raw.direction,
            core: Some(CoreStatus {
                level: raw.level,
                ability_level: raw.ability_level,
                max_health: raw.max_health,
                max_mana: raw.max_mana,
                weight: raw.weight,
                max_weight: raw.max_weight,
                stats: CharacterStats {
                    stat_points: 0,
                    strength: raw.strength,
                    intelligence: raw.intelligence,
                    wisdom: raw.wisdom,
                    constitution: raw.constitution,
                    dexterity: raw.dexterity,
                },
            }),
            vitals: Some(CurrentVitals {
                health: raw.health,
                mana: raw.mana,
            }),
            progression: raw.pane_progression.map(|pane| ProgressionStatus {
                experience: raw.experience,
                ability_points: pane.ability_points,
                experience_to_next_level: pane.experience_to_next_level,
                ability_to_next_level: pane.ability_to_next_level,
            }),
            gold: Some(raw.gold),
            modifiers: raw.modifiers.map(modifiers),
            is_blinded: Some(raw.is_blinded),
            is_action_restricted: Some(raw.is_action_restricted),
            is_casting: Some(raw.is_casting),
            is_walking: Some(raw.is_walking),
            is_hidden: Some(raw.is_hidden),
            map: raw.location.map(|location| CachedMap {
                id: location.map_id,
                width: location.width,
                height: location.height,
            }),
            position: raw.location.and_then(|location| location.x.zip(location.y)),
            pending_map: None,
            active_spell: None,
            effects: raw.effects.map_or([None; 10], |effects| {
                effects.effects.map(|effect| {
                    effect.map(|effect| Effect {
                        icon: effect.icon,
                        duration: EffectDuration::from_raw(effect.duration)
                            .expect("captured effect duration is valid"),
                    })
                })
            }),
        }
    }

    pub(super) fn filter_status(&mut self, update: StatusUpdate) -> StatusUpdate {
        StatusUpdate {
            core: changed(&mut self.core, update.core),
            vitals: changed(&mut self.vitals, update.vitals),
            progression: changed(&mut self.progression, update.progression),
            gold: changed(&mut self.gold, update.gold),
            modifiers: changed(&mut self.modifiers, update.modifiers),
            is_blinded: changed(&mut self.is_blinded, update.is_blinded),
            is_action_restricted: changed(
                &mut self.is_action_restricted,
                update.is_action_restricted,
            ),
            is_casting: changed(&mut self.is_casting, update.is_casting),
        }
    }

    #[cfg_attr(
        test,
        expect(dead_code, reason = "called by the production-only stat action")
    )]
    pub(super) fn stat_points(&self) -> Option<u8> {
        self.core.map(|core| core.stats.stat_points)
    }

    pub(super) fn core_with_stat_points(&self, stat_points: u8) -> Option<CoreStatus> {
        let mut core = self.core?;
        core.stats.stat_points = stat_points;
        Some(core)
    }

    pub(super) fn lifecycle(&mut self, current: ClientLifecycle) -> Option<LifecycleUpdate> {
        let previous = self.lifecycle.replace(current)?;
        (previous != current).then_some(LifecycleUpdate { previous, current })
    }

    #[cfg(test)]
    pub(super) fn position_desynchronized(&self, map_id: u32, local: TilePosition) -> bool {
        let (Some(map), Some((x, y))) = (self.map, self.position) else {
            return false;
        };
        map.id != map_id || local != TilePosition { x, y }
    }

    pub(super) fn confirmed_location(&self) -> Option<(u32, TilePosition)> {
        let (map, (x, y)) = self.map.zip(self.position)?;
        Some((map.id, TilePosition { x, y }))
    }

    pub(super) fn confirmed_pose(&self) -> Option<(TilePosition, Direction)> {
        let (_, position) = self.confirmed_location()?;
        let direction = Direction::from_raw(self.self_direction?)?;
        Some((position, direction))
    }

    #[cfg(not(test))]
    pub(super) const fn position(&self) -> Option<(i32, i32)> {
        self.position
    }

    pub(super) fn merge_position(&self, raw: &mut RawCharacter) {
        let (Some(map), Some((x, y)), Some(location)) =
            (self.map, self.position, raw.location.as_mut())
        else {
            return;
        };
        if map.id != location.map_id
            || map.width != location.width
            || map.height != location.height
            || (location.x.is_some() && location.y.is_some())
        {
            return;
        }
        location.x = Some(x);
        location.y = Some(y);
    }

    #[cfg(not(test))]
    pub(super) fn valid_tile(&self, x: i32, y: i32) -> bool {
        self.map
            .is_some_and(|map| x >= 0 && y >= 0 && x < map.width && y < map.height)
    }

    pub(super) fn spell_begin(&mut self, slot: u8, total_lines: u8) -> Option<u8> {
        let replaced = (self.is_casting == Some(true))
            .then(|| self.active_spell.map(|cast| cast.slot))
            .flatten();
        let cast = CachedCast { slot, total_lines };
        self.is_casting = Some(true);
        self.active_spell = Some(cast);
        replaced
    }

    pub(super) fn spell_finished(&mut self) {
        self.is_casting = Some(false);
        self.active_spell = None;
    }

    pub(super) fn spell_cast(&mut self, slot: u8) -> Option<u8> {
        let replaced = (self.is_casting == Some(true))
            .then(|| self.active_spell.map(|cast| cast.slot))
            .flatten()
            .filter(|active_slot| *active_slot != slot);
        self.spell_finished();
        replaced
    }

    pub(super) fn spell_cancelled(&mut self, fallback_slot: Option<u8>) -> Option<u8> {
        let slot = self.active_spell.map(|cast| cast.slot).or(fallback_slot);
        self.spell_finished();
        slot.filter(|slot| *slot != 0 && *slot <= 90)
    }

    pub(super) fn observe_casting(&mut self, state: CastingState) -> Option<QueuedAbilityUpdate> {
        if state.active {
            self.is_casting = Some(true);
            if let Some(slot) = state.slot.filter(|slot| *slot != 0 && *slot <= 90) {
                self.active_spell = Some(CachedCast {
                    slot,
                    total_lines: state.total_lines,
                });
            }
            return None;
        }
        if self.is_casting != Some(true) {
            self.is_casting = Some(false);
            return None;
        }
        self.spell_cancelled(state.slot)
            .map(|slot| QueuedAbilityUpdate::SpellCancelled {
                slot,
                source: SpellCancellationSource::Client,
            })
    }

    pub(super) fn movement(
        &mut self,
        is_walking: bool,
        source: ActionSource,
        destination: Option<TilePosition>,
        stop_reason: Option<MovementStopReason>,
    ) -> Option<MovementUpdate> {
        if self.is_walking == Some(is_walking) {
            return None;
        }
        let (x, y) = self.position?;
        self.is_walking = Some(is_walking);
        let current = TilePosition { x, y };
        Some(if is_walking {
            MovementUpdate::Started {
                source,
                current,
                destination,
            }
        } else {
            MovementUpdate::Stopped {
                source,
                current,
                destination,
                reached_destination: destination.map(|destination| destination == current),
                reason: stop_reason.unwrap_or_else(|| {
                    if destination.is_none_or(|destination| destination == current) {
                        MovementStopReason::Completed
                    } else {
                        MovementStopReason::Obstructed
                    }
                }),
            }
        })
    }

    pub(super) fn stage_map_transition(
        &mut self,
        map_id: u32,
        width: i32,
        height: i32,
        name: &[u8],
    ) -> Option<QueuedLocationUpdate> {
        let pending = QueuedMapChange::new(map_id, width, height, name);
        if self.map == Some(pending.into()) {
            self.pending_map = None;
            return None;
        }
        if self.map.is_none()
            && let Some((x, y)) = self.position
        {
            self.map = Some(pending.into());
            self.pending_map = None;
            return Some(QueuedLocationUpdate {
                x,
                y,
                map: Some(pending),
            });
        }
        self.pending_map = Some(pending);
        None
    }

    pub(super) fn user_position(&mut self, x: i32, y: i32) -> Option<(QueuedLocationUpdate, bool)> {
        let map = self.pending_map.take();
        if map.is_none() && self.position == Some((x, y)) {
            return None;
        }
        self.position = Some((x, y));
        if let Some(map) = map {
            self.map = Some(map.into());
        }
        let map_changed = map.is_some();
        Some((QueuedLocationUpdate { x, y, map }, map_changed))
    }

    pub(super) fn move_position(&mut self, x: i32, y: i32) -> Option<QueuedLocationUpdate> {
        if self.pending_map.is_some() || self.position == Some((x, y)) {
            return None;
        }
        self.position = Some((x, y));
        Some(QueuedLocationUpdate { x, y, map: None })
    }

    pub(super) fn effect(
        &mut self,
        icon: u16,
        duration: Option<EffectDuration>,
    ) -> Option<EffectUpdate> {
        if let Some(index) = self
            .effects
            .iter()
            .position(|effect| effect.is_some_and(|effect| effect.icon == icon))
        {
            return match duration {
                None => {
                    self.effects[index] = None;
                    Some(EffectUpdate::Removed { icon })
                }
                Some(duration) => {
                    let effect = Effect { icon, duration };
                    if self.effects[index] == Some(effect) {
                        None
                    } else {
                        self.effects[index] = Some(effect);
                        Some(EffectUpdate::Changed(effect))
                    }
                }
            };
        }

        let duration = duration?;
        let slot = self.effects.iter_mut().find(|effect| effect.is_none())?;
        let effect = Effect { icon, duration };
        *slot = Some(effect);
        Some(EffectUpdate::Added(effect))
    }
}

fn changed<T: Copy + Eq>(cached: &mut Option<T>, incoming: Option<T>) -> Option<T> {
    let value = incoming?;
    if *cached == Some(value) {
        return None;
    }
    *cached = Some(value);
    Some(value)
}

fn modifiers(raw: RawModifiers) -> CharacterModifiers {
    CharacterModifiers {
        armor_class: raw.armor_class,
        damage: raw.damage,
        hit: raw.hit,
        magic_resistance: raw.magic_resistance_units.saturating_mul(10),
        attack_element: Element::from_raw(raw.attack_element),
        defense_element: Element::from_raw(raw.defense_element),
    }
}

pub(super) struct MainThreadCache(UnsafeCell<StateCache>);

// SAFETY: access is restricted to the client main thread except during reset,
// which runs only while the producer hook is absent.
unsafe impl Sync for MainThreadCache {}

impl MainThreadCache {
    pub(super) const fn new() -> Self {
        Self(UnsafeCell::new(StateCache {
            lifecycle: None,
            self_id: None,
            self_name: [0; 16],
            self_name_len: 0,
            self_direction: None,
            core: None,
            vitals: None,
            progression: None,
            gold: None,
            modifiers: None,
            is_blinded: None,
            is_action_restricted: None,
            is_casting: None,
            is_walking: None,
            is_hidden: None,
            map: None,
            position: None,
            pending_map: None,
            active_spell: None,
            effects: [None; 10],
        }))
    }

    pub(super) unsafe fn replace(&self, cache: StateCache) {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { *self.0.get() = cache };
    }

    pub(super) unsafe fn self_id(&self) -> Option<u32> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&*self.0.get()).self_id }
    }

    #[cfg(not(test))]
    pub(super) unsafe fn confirmed_location(&self) -> Option<(u32, TilePosition)> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&*self.0.get()).confirmed_location() }
    }

    #[cfg(all(windows, not(test)))]
    pub(super) unsafe fn confirmed_pose(&self) -> Option<(TilePosition, Direction)> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&*self.0.get()).confirmed_pose() }
    }

    pub(super) unsafe fn gold(&self) -> Option<u32> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&*self.0.get()).gold }
    }

    #[cfg_attr(
        test,
        expect(dead_code, reason = "called by the production-only stat action")
    )]
    pub(super) unsafe fn stat_points(&self) -> Option<u8> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&*self.0.get()).stat_points() }
    }

    pub(super) unsafe fn core_with_stat_points(&self, stat_points: u8) -> Option<CoreStatus> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&*self.0.get()).core_with_stat_points(stat_points) }
    }

    pub(super) unsafe fn merge_position(&self, raw: &mut RawCharacter) {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&*self.0.get()).merge_position(raw) };
    }

    pub(super) unsafe fn self_name(&self) -> Option<([u8; 16], u8)> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        let cache = unsafe { &*self.0.get() };
        (cache.self_name_len != 0).then_some((cache.self_name, cache.self_name_len))
    }

    pub(super) unsafe fn self_entity(&self, id: u32) -> Option<RawWorldObject> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        let cache = unsafe { &*self.0.get() };
        if cache.self_id != Some(id) {
            return None;
        }
        let (x, y) = cache.position?;
        let direction = cache.self_direction?;
        let is_hidden = cache.is_hidden?;
        let name_len = usize::from(cache.self_name_len).min(MAX_OBJECT_NAME_BYTES);
        let mut name = [0; MAX_OBJECT_NAME_BYTES];
        name[..name_len].copy_from_slice(&cache.self_name[..name_len]);
        Some(RawWorldObject::Player {
            id,
            name,
            name_len: u8::try_from(name_len).expect("self name length fits u8"),
            x,
            y,
            direction,
            is_hidden,
            visual: None,
        })
    }

    #[cfg(not(test))]
    pub(super) unsafe fn position(&self) -> Option<(i32, i32)> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&*self.0.get()).position() }
    }

    #[cfg(not(test))]
    pub(super) unsafe fn valid_tile(&self, x: i32, y: i32) -> bool {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&*self.0.get()).valid_tile(x, y) }
    }

    pub(super) unsafe fn spell_begin(&self, slot: u8, total_lines: u8) -> Option<u8> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&mut *self.0.get()).spell_begin(slot, total_lines) }
    }

    pub(super) unsafe fn spell_cast(&self, slot: u8) -> Option<u8> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&mut *self.0.get()).spell_cast(slot) }
    }

    pub(super) unsafe fn spell_cancelled(&self, fallback_slot: Option<u8>) -> Option<u8> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&mut *self.0.get()).spell_cancelled(fallback_slot) }
    }

    pub(super) unsafe fn observe_casting(
        &self,
        state: CastingState,
    ) -> Option<QueuedAbilityUpdate> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&mut *self.0.get()).observe_casting(state) }
    }

    pub(super) unsafe fn filter_status(&self, update: StatusUpdate) -> StatusUpdate {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&mut *self.0.get()).filter_status(update) }
    }

    #[cfg(not(test))]
    pub(super) unsafe fn lifecycle(&self, current: ClientLifecycle) -> Option<LifecycleUpdate> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&mut *self.0.get()).lifecycle(current) }
    }

    #[cfg(not(test))]
    pub(super) unsafe fn movement(
        &self,
        is_walking: bool,
        source: ActionSource,
        destination: Option<TilePosition>,
        stop_reason: Option<MovementStopReason>,
    ) -> Option<MovementUpdate> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&mut *self.0.get()).movement(is_walking, source, destination, stop_reason) }
    }

    pub(super) unsafe fn map_transition_pending(&self) -> bool {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&*self.0.get()).pending_map.is_some() }
    }

    pub(super) unsafe fn stage_map_transition(
        &self,
        map_id: u32,
        width: i32,
        height: i32,
        name: &[u8],
    ) -> Option<QueuedLocationUpdate> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&mut *self.0.get()).stage_map_transition(map_id, width, height, name) }
    }

    pub(super) unsafe fn user_position(
        &self,
        x: i32,
        y: i32,
    ) -> Option<(QueuedLocationUpdate, bool)> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&mut *self.0.get()).user_position(x, y) }
    }

    pub(super) unsafe fn move_position(&self, x: i32, y: i32) -> Option<QueuedLocationUpdate> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&mut *self.0.get()).move_position(x, y) }
    }

    pub(super) unsafe fn effect(
        &self,
        icon: u16,
        duration: Option<EffectDuration>,
    ) -> Option<EffectUpdate> {
        // SAFETY: the caller guarantees exclusive main-thread access.
        unsafe { (&mut *self.0.get()).effect(icon, duration) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_only_emits_real_transitions() {
        let mut cache = StateCache::default();
        assert_eq!(cache.lifecycle(ClientLifecycle::Title), None);
        assert_eq!(cache.lifecycle(ClientLifecycle::Title), None);
        assert_eq!(
            cache.lifecycle(ClientLifecycle::InGame),
            Some(LifecycleUpdate {
                previous: ClientLifecycle::Title,
                current: ClientLifecycle::InGame,
            })
        );
    }
}
