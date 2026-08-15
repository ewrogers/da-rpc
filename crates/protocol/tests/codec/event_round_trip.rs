use super::*;

#[test]
fn event_messages_round_trip() {
    let messages = [
        Message::EventPollResponse(EventPollResponse {
            request_id: 12,
            result: EventPollResult::Events(vec![
                StateEvent {
                    sequence: 37,
                    revision: 6,
                    tick_ms: 119,
                    update: StateUpdate::Lifecycle(LifecycleUpdate {
                        previous: ClientLifecycle::Title,
                        current: ClientLifecycle::InGame,
                    }),
                },
                StateEvent {
                    sequence: 38,
                    revision: 7,
                    tick_ms: 120,
                    update: StateUpdate::Audio(AudioUpdate::SoundPlayed { effect: 12 }),
                },
                StateEvent {
                    sequence: 39,
                    revision: 8,
                    tick_ms: 121,
                    update: StateUpdate::Audio(AudioUpdate::MusicStarted { track: 4 }),
                },
                StateEvent {
                    sequence: 40,
                    revision: 9,
                    tick_ms: 122,
                    update: StateUpdate::Audio(AudioUpdate::MusicStopped),
                },
                StateEvent {
                    sequence: 400,
                    revision: 90,
                    tick_ms: 122,
                    update: StateUpdate::Command(ClientCommand {
                        command: "walk".into(),
                        args: vec!["x".into(), "y".into()],
                    }),
                },
                StateEvent {
                    sequence: 401,
                    revision: 91,
                    tick_ms: 122,
                    update: StateUpdate::Action(ActionUpdate::Resync),
                },
                StateEvent {
                    sequence: 41,
                    revision: 10,
                    tick_ms: 123,
                    update: StateUpdate::Status(StatusUpdate {
                        core: Some(CoreStatus {
                            level: 99,
                            ability_level: 12,
                            max_health: 2_000,
                            max_mana: 1_500,
                            weight: 88,
                            max_weight: 120,
                            stats: CharacterStats {
                                stat_points: 3,
                                strength: 11,
                                intelligence: 12,
                                wisdom: 13,
                                constitution: 14,
                                dexterity: 15,
                            },
                        }),
                        vitals: Some(CurrentVitals {
                            health: 1_900,
                            mana: 1_400,
                        }),
                        progression: Some(ProgressionStatus {
                            experience: 100,
                            ability_points: 200,
                            experience_to_next_level: 300,
                            ability_to_next_level: 400,
                        }),
                        gold: Some(500),
                        modifiers: Some(CharacterModifiers {
                            armor_class: -10,
                            damage: 8,
                            hit: 7,
                            magic_resistance: 60,
                            attack_element: Element::Fire,
                            defense_element: Element::Water,
                        }),
                        is_blinded: Some(true),
                        is_action_restricted: Some(true),
                        is_casting: Some(false),
                    }),
                },
                StateEvent {
                    sequence: 42,
                    revision: 11,
                    tick_ms: 124,
                    update: StateUpdate::Location(LocationUpdate {
                        x: 43,
                        y: 40,
                        map: Some(MapChange {
                            id: 3001,
                            name: Some("Mileth".into()),
                            width: 100,
                            height: 80,
                        }),
                    }),
                },
                StateEvent {
                    sequence: 43,
                    revision: 12,
                    tick_ms: 125,
                    update: StateUpdate::Effect(EffectUpdate::Added(Effect {
                        icon: 300,
                        duration: EffectDuration::White,
                    })),
                },
                StateEvent {
                    sequence: 44,
                    revision: 13,
                    tick_ms: 126,
                    update: StateUpdate::Effect(EffectUpdate::Changed(Effect {
                        icon: 300,
                        duration: EffectDuration::Red,
                    })),
                },
                StateEvent {
                    sequence: 45,
                    revision: 14,
                    tick_ms: 127,
                    update: StateUpdate::Effect(EffectUpdate::Removed { icon: 300 }),
                },
                StateEvent {
                    sequence: 46,
                    revision: 15,
                    tick_ms: 128,
                    update: StateUpdate::Object(ObjectUpdate::Moved(WorldObject::Player {
                        id: 10,
                        name: Some("Eidolon".into()),
                        x: 41,
                        y: 30,
                        direction: Direction::East,
                        is_hidden: false,
                        visual: Some(player_visual()),
                        profile: None,
                    })),
                },
                StateEvent {
                    sequence: 47,
                    revision: 16,
                    tick_ms: 129,
                    update: StateUpdate::Message(ClientMessage {
                        kind: MessageKind::Whisper,
                        sender: Some("Eidolon".into()),
                        recipient: Some("Monitor".into()),
                        text: "hello".into(),
                    }),
                },
                StateEvent {
                    sequence: 470,
                    revision: 160,
                    tick_ms: 129,
                    update: StateUpdate::Message(ClientMessage {
                        kind: MessageKind::Chant,
                        sender: Some("Eidolon".into()),
                        recipient: None,
                        text: "ard cradh".into(),
                    }),
                },
                StateEvent {
                    sequence: 48,
                    revision: 17,
                    tick_ms: 130,
                    update: StateUpdate::Inventory(SlotUpdate {
                        batch_index: 0,
                        batch_count: 2,
                        change: CollectionChange::Changed,
                        slot: 1,
                        before: Some(InventoryItem {
                            slot: 1,
                            sprite: 21,
                            dye_color: 2,
                            name: Some("Hy-Brasyl Gauntlet".into()),
                            quantity: 1,
                            can_stack: false,
                            durability: 900,
                            max_durability: 1_000,
                        }),
                        after: None,
                    }),
                },
                StateEvent {
                    sequence: 49,
                    revision: 18,
                    tick_ms: 130,
                    update: StateUpdate::Inventory(SlotUpdate {
                        batch_index: 1,
                        batch_count: 2,
                        change: CollectionChange::Changed,
                        slot: 2,
                        before: None,
                        after: Some(InventoryItem {
                            slot: 2,
                            sprite: 21,
                            dye_color: 2,
                            name: Some("Hy-Brasyl Gauntlet".into()),
                            quantity: 1,
                            can_stack: false,
                            durability: 900,
                            max_durability: 1_000,
                        }),
                    }),
                },
                StateEvent {
                    sequence: 50,
                    revision: 19,
                    tick_ms: 131,
                    update: StateUpdate::Spellbook(SlotUpdate {
                        batch_index: 0,
                        batch_count: 1,
                        change: CollectionChange::Added,
                        slot: 4,
                        before: None,
                        after: Some(Spell {
                            slot: 4,
                            icon: 82,
                            name: Some("beag srad".into()),
                            level: 10,
                            max_level: 100,
                            lines: 2,
                            target_type: SpellTargetType::Target,
                            prompt: None,
                            cooldown: CooldownStatus {
                                active: false,
                                cooldown_ms: None,
                                remaining_ms: None,
                            },
                        }),
                    }),
                },
                StateEvent {
                    sequence: 51,
                    revision: 20,
                    tick_ms: 132,
                    update: StateUpdate::Skillbook(SlotUpdate {
                        batch_index: 0,
                        batch_count: 1,
                        change: CollectionChange::Removed,
                        slot: 7,
                        before: Some(Skill {
                            slot: 7,
                            icon: 91,
                            name: Some("Assail".into()),
                            level: 100,
                            max_level: 100,
                            cooldown: CooldownStatus {
                                active: false,
                                cooldown_ms: None,
                                remaining_ms: None,
                            },
                        }),
                        after: None,
                    }),
                },
                StateEvent {
                    sequence: 52,
                    revision: 21,
                    tick_ms: 133,
                    update: StateUpdate::Movement(MovementUpdate::Started {
                        current: TilePosition { x: 10, y: 20 },
                        destination: Some(TilePosition { x: 30, y: 40 }),
                    }),
                },
                StateEvent {
                    sequence: 53,
                    revision: 22,
                    tick_ms: 134,
                    update: StateUpdate::Movement(MovementUpdate::Stopped {
                        current: TilePosition { x: 30, y: 40 },
                        destination: Some(TilePosition { x: 30, y: 40 }),
                        reached_destination: Some(true),
                    }),
                },
                StateEvent {
                    sequence: 530,
                    revision: 220,
                    tick_ms: 1_340,
                    update: StateUpdate::Movement(MovementUpdate::Obstructed {
                        map_id: 3000,
                        current: TilePosition { x: 11, y: 20 },
                        attempted: TilePosition { x: 12, y: 20 },
                        direction: Direction::East,
                        destination: Some(TilePosition { x: 30, y: 40 }),
                        mode: WalkMode::ExactRoute,
                    }),
                },
                StateEvent {
                    sequence: 531,
                    revision: 221,
                    tick_ms: 1_341,
                    update: StateUpdate::MapExclusions(MapExclusionsUpdate::Replaced {
                        exclusions: MapExclusions {
                            map_id: 3000,
                            tiles: vec![TilePosition { x: 4, y: 5 }, TilePosition { x: 9, y: 2 }],
                        },
                        map_count: 2,
                    }),
                },
                StateEvent {
                    sequence: 54,
                    revision: 23,
                    tick_ms: 135,
                    update: StateUpdate::Ability(AbilityUpdate::SpellCast {
                        slot: 4,
                        arguments: SpellCastArguments::Target {
                            id: Some(0x1122_3344),
                            x: 30,
                            y: 40,
                        },
                    }),
                },
                StateEvent {
                    sequence: 55,
                    revision: 24,
                    tick_ms: 136,
                    update: StateUpdate::Ability(AbilityUpdate::SpellCancelled {
                        slot: 4,
                        source: SpellCancellationSource::Replaced,
                    }),
                },
                StateEvent {
                    sequence: 56,
                    revision: 25,
                    tick_ms: 137,
                    update: StateUpdate::Action(ActionUpdate::GoldDropped {
                        amount: 500,
                        position: TilePosition { x: 30, y: 40 },
                    }),
                },
                StateEvent {
                    sequence: 57,
                    revision: 25,
                    tick_ms: 138,
                    update: StateUpdate::Entity(EntityUpdate::Animated {
                        entity: WorldObject::Player {
                            id: 10,
                            name: Some("Eidolon".into()),
                            x: 30,
                            y: 40,
                            direction: Direction::North,
                            is_hidden: false,
                            visual: Some(player_visual()),
                            profile: None,
                        },
                        animation: 7,
                        duration_10ms: 25,
                    }),
                },
                StateEvent {
                    sequence: 58,
                    revision: 25,
                    tick_ms: 139,
                    update: StateUpdate::Entity(EntityUpdate::Effect {
                        entity: WorldObject::Creature {
                            id: 20,
                            kind: CreatureKind::Monster,
                            is_solid: false,
                            sprite: Some(123),
                            name: None,
                            x: 31,
                            y: 40,
                            direction: Direction::West,
                        },
                        effect: 42,
                        source: Some(WorldObject::Player {
                            id: 10,
                            name: Some("Eidolon".into()),
                            x: 30,
                            y: 40,
                            direction: Direction::North,
                            is_hidden: false,
                            visual: Some(player_visual()),
                            profile: None,
                        }),
                        frame_interval_ms: Some(50),
                    }),
                },
                StateEvent {
                    sequence: 59,
                    revision: 25,
                    tick_ms: 140,
                    update: StateUpdate::Entity(EntityUpdate::Damaged {
                        entity: WorldObject::Creature {
                            id: 21,
                            kind: CreatureKind::Npc,
                            is_solid: true,
                            sprite: Some(456),
                            name: Some("Beggar".into()),
                            x: 32,
                            y: 40,
                            direction: Direction::South,
                        },
                        health_percent: 73,
                    }),
                },
                StateEvent {
                    sequence: 60,
                    revision: 26,
                    tick_ms: 141,
                    update: StateUpdate::Dialog(DialogUpdate::Changed(dialog_state())),
                },
                StateEvent {
                    sequence: 61,
                    revision: 27,
                    tick_ms: 142,
                    update: StateUpdate::Group(GroupUpdate::Joined {
                        state: group_state(),
                    }),
                },
                StateEvent {
                    sequence: 62,
                    revision: 28,
                    tick_ms: 143,
                    update: StateUpdate::Group(GroupUpdate::SettingsChanged {
                        state: group_state(),
                    }),
                },
                StateEvent {
                    sequence: 63,
                    revision: 29,
                    tick_ms: 144,
                    update: StateUpdate::Exchange(ExchangeUpdate::ItemAdded {
                        state: exchange_state(),
                        party: ExchangeParty::Local,
                        item: exchange_state().local.items[0].clone(),
                    }),
                },
                StateEvent {
                    sequence: 64,
                    revision: 30,
                    tick_ms: 145,
                    update: StateUpdate::Legend(LegendUpdate::MarkChanged {
                        previous: LegendMark {
                            text: "Found the grove".into(),
                            tag: "Quest".into(),
                            color: 3,
                            icon: LegendIcon::Aisling,
                        },
                        current: LegendMark {
                            text: "Found the hidden grove".into(),
                            tag: "Quest".into(),
                            color: 7,
                            icon: LegendIcon::Wizard,
                        },
                    }),
                },
                StateEvent {
                    sequence: 65,
                    revision: 31,
                    tick_ms: 146,
                    update: StateUpdate::Player(PlayerUpdate {
                        player: WorldObject::Player {
                            id: 77,
                            name: Some("Eidolon".into()),
                            x: 10,
                            y: 20,
                            direction: Direction::North,
                            is_hidden: false,
                            visual: Some(player_visual()),
                            profile: Some(Box::new(player_profile())),
                        },
                        changes: PlayerInspectionChanges {
                            info: true,
                            equipment: false,
                            legend: true,
                        },
                        trigger: PlayerInspectionTrigger::Manual,
                    }),
                },
                StateEvent {
                    sequence: 66,
                    revision: 32,
                    tick_ms: 147,
                    update: StateUpdate::CharacterProfile(CharacterProfileUpdate {
                        previous: None,
                        current: player_profile().identity,
                    }),
                },
                StateEvent {
                    sequence: 67,
                    revision: 33,
                    tick_ms: 148,
                    update: StateUpdate::PlannedRoute(PlannedRoute {
                        generation: 12,
                        tiles: vec![
                            TilePosition { x: 10, y: 20 },
                            TilePosition { x: 11, y: 20 },
                            TilePosition { x: 11, y: 21 },
                        ],
                    }),
                },
                StateEvent {
                    sequence: 68,
                    revision: 34,
                    tick_ms: 149,
                    update: StateUpdate::FieldMap(FieldMapUpdate::Opened(field_map_state())),
                },
                StateEvent {
                    sequence: 69,
                    revision: 35,
                    tick_ms: 150,
                    update: StateUpdate::FieldMap(FieldMapUpdate::Changed(field_map_state())),
                },
                StateEvent {
                    sequence: 70,
                    revision: 36,
                    tick_ms: 151,
                    update: StateUpdate::FieldMap(FieldMapUpdate::SelectionSubmitted(
                        field_map_state(),
                    )),
                },
                StateEvent {
                    sequence: 71,
                    revision: 37,
                    tick_ms: 152,
                    update: StateUpdate::FieldMap(FieldMapUpdate::Closed {
                        previous: field_map_state(),
                    }),
                },
            ]),
        }),
        Message::EventPollResponse(EventPollResponse {
            request_id: 13,
            result: EventPollResult::ResyncRequired {
                missing_sequence: 42,
                latest_sequence: 900,
            },
        }),
    ];

    for message in messages {
        let frame = Frame::new(7, 123, message);
        let bytes = encode_frame(&frame).unwrap();
        assert_eq!(decode_frame(&bytes).unwrap(), frame);
    }
}
