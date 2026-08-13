# Binary protocol

This is the wire-level reference between `darpc.dll`, `darpc.exe`, and
`darpcd.exe`. Web API consumers normally do not need it. Read this chapter when
implementing a direct pipe client, changing protocol messages, or debugging
compatibility.

`darpc.dll` communicates with a controller using a purpose-built binary
protocol. The codec is platform-independent; the Windows transport carries one
frame at a time over a process-specific named pipe.

All integers are unsigned and little-endian. The definitions below resemble
Rust for readability, but they describe serialized fields in order without
compiler padding. Rust memory layout is never copied directly onto the wire.

## Frame

Every frame begins with this fixed 20-byte header:

```rust,ignore
struct FrameHeader {
    magic: [u8; 4],       // offset 0: ASCII "DRPC"
    frame_version: u16,   // offset 4: currently 1
    message_type: u16,    // offset 6: MessageType discriminant
    sequence: u16,        // offset 8: per-sender wrapping counter
    flags: u16,           // offset 10: reserved, must be zero
    sender_tick_ms: u32,  // offset 12: wrapping Windows millisecond tick
    payload_len: u32,     // offset 16: bytes after this header
}
```

Payloads are limited to 4 MiB, making the largest complete frame 4 MiB plus 20
bytes. A receiver reads the header into a fixed-size buffer, validates it, and
only then reads the declared payload. A frame must contain exactly that payload
length. Truncation and trailing bytes are errors.

The frame version describes the envelope above and remains the simple integer
`1`. It is independent from the negotiated protocol version carried by
`Hello`.

## Protocol versions

A negotiated protocol version is one `u16` split into major and minor bytes:

```rust,ignore
let version: u16 = ((major as u16) << 8) | minor as u16;

const VERSION_1_0: u16 = 0x0100;
const VERSION_1_1: u16 = 0x0101;
const VERSION_2_0: u16 = 0x0200;
```

A major change may be incompatible. A minor change is additive and must remain
compatible with earlier minor versions in the same major line. Each peer
advertises an inclusive, continuous range and the controller selects the
highest version in the overlap. A peer must not advertise one continuous range
across an incompatible major boundary. No overlap rejects the connection.

The only currently supported version is 1.1 (`0x0101`). Version 1.1 adds exact
route and path-exclusion commands plus obstruction events. Peers advertise only
1.1 because these features can introduce command and event discriminants that
a 1.0 decoder cannot interpret safely.

## Message types

```rust,ignore
enum MessageType: u16 {
    Hello        = 1,
    HelloAck     = 2,
    Ping         = 3,
    Pong         = 4,
    EchoRequest  = 5,
    EchoResponse = 6,
    TickHealthRequest  = 7,
    TickHealthResponse = 8,
    SnapshotRequest    = 9,
    SnapshotResponse   = 10,
    EventPollRequest   = 11,
    EventPollResponse  = 12,
    CommandRequest     = 13,
    CommandResponse    = 14,
}
```

The normal request direction is controller to DLL. Responses travel from DLL to
controller. `Hello` starts in the opposite direction because the DLL announces
its identity and capabilities immediately after a connection is established.

## Hello and HelloAck

```rust,ignore
struct Hello {
    protocol_min: u16,             // offset 0: inclusive 0xMMmm version
    protocol_max: u16,             // offset 2: inclusive 0xMMmm version
    dll_instance_id: [u8; 16],     // offset 4: one initialized DLL lifetime
    process_id: u32,                // offset 20
    process_creation_time: u64,     // offset 24: raw Windows FILETIME
    architecture: Architecture,     // offset 32: encoded as u8
    dll_version_major: u16,         // offset 33
    dll_version_minor: u16,         // offset 35
    dll_version_patch: u16,         // offset 37
    executable_fingerprint: [u8; 32], // offset 39: SHA-256
    client_version: u32,            // offset 71
} // 75 bytes

enum Architecture: u8 {
    X86    = 1,
    X86_64 = 2,
}

struct HelloAck {
    selected_version: u16,      // offset 0: negotiated 0xMMmm version
    dll_instance_id: [u8; 16],  // offset 2: copied from Hello
} // 18 bytes
```

The instance ID identifies one initialized DLL lifetime. The process ID and raw
creation time together distinguish PID reuse. The executable fingerprint,
architecture, and client version identify the supported client contract.

`HelloAck` copies the instance ID so an acknowledgement for one DLL instance
cannot complete another instance's handshake. Application messages are invalid
until this exchange completes:

```text
DLL                                      controller
 |---- Hello (version range, identity) ------>|
 |<--- HelloAck (selected version, identity) -|
 |                 ready                       |
```

These identity fields protect against stale or accidental connections. They are
not authentication against a malicious local process; transport-level peer
validation remains a separate responsibility.

## Ping, Pong, and echo

```rust,ignore
struct Ping {
    request_id: u32,  // offset 0
} // 4 bytes

struct Pong {
    request_id: u32,  // offset 0: copied from Ping
} // 4 bytes

struct EchoRequest {
    request_id: u32,  // offset 0
    text_len: u16,    // offset 4: UTF-8 byte count, at most 4,096
    text: utf8[text_len], // offset 6
}

struct EchoResponse {
    request_id: u32,  // offset 0: copied from EchoRequest
    text_len: u16,    // offset 4: copied from EchoRequest
    text: utf8[text_len], // offset 6: copied from EchoRequest
}
```

Request IDs wrap as `u32` and provide request/response correlation. They are
separate from frame sequence numbers because unsolicited events may be
interleaved and one future request may produce more than one frame.

## Tick-hook health

```rust,ignore
struct TickHealthRequest {
    request_id: u32,  // offset 0
} // 4 bytes

struct TickHealthResponse {
    request_id: u32,      // offset 0: copied from TickHealthRequest
    installed: bool,      // offset 4: u8, exactly 0 or 1
    relocated_bytes: u8,  // offset 5: complete target bytes in trampoline
    tick_count: u32,      // offset 6: wrapping observation counter
} // 10 bytes
```

The response is a worker-thread snapshot of atomic hook state. Comparing two
responses with `wrapping_sub` shows whether the client dispatcher advanced
during the sample window without performing IPC or logging in the hook itself.

## Client snapshot

```rust,ignore
struct SnapshotRequest {
    request_id: u32;
}

enum SnapshotResult {
    Unavailable(SnapshotUnavailableReason),
    Ready(ClientSnapshot),
}

struct SnapshotResponse {
    request_id: u32;
    result: SnapshotResult;
}

enum ClientLifecycle: u8 {
    Unknown      = 0,
    Title        = 1,
    Transition   = 2,
    InGame       = 3,
    Disconnected = 4,
}

struct ClientSnapshot {
    revision: u32;
    event_sequence: u32;
    captured_tick_ms: u32;
    updated_tick_ms: u32;
    capture_duration_us: u32;
    world_generation: u32;
    lifecycle: ClientLifecycle;
    character: Option<CharacterSnapshot>;
    objects: Option<Vec<WorldObject>>;
    dialog: Option<DialogState>;
    group: Option<GroupState>;
    exchange: Option<ExchangeState>;
    legend: Option<Vec<LegendMark>>;
    planned_route: Option<PlannedRoute>;
    map_exclusions: Vec<MapExclusions>;     // u16 count, maximum 1,024
}

struct PlannedRoute {
    generation: u32;
    tiles: Vec<TilePosition>;              // u32 count, maximum 160,001
}

struct MapExclusions {
    map_id: u32;                           // 0 through 65,535
    tile_count: u16;                        // 1 through 256
    tiles: [RouteTile; tile_count];
}

struct LegendMark {
    icon: u8;                              // 0 through 8 are named icons
    color: u8;
    tag: string16;                         // maximum 255 UTF-8 bytes
    text: string16;                        // maximum 255 UTF-8 bytes
}

struct ExchangeState {
    id: u32;
    partner: string16;                     // maximum 255 UTF-8 bytes
    local: ExchangeOffer;
    other: ExchangeOffer;
}

struct ExchangeOffer {
    items: Vec<ExchangeItem>;              // u8 count, maximum 8
    gold: u32;
    accepted: bool;
}

struct ExchangeItem {
    index: u8;                             // zero-based, 0 through 7
    sprite: u16;
    dye_color: u8;
    quantity: Option<u8>;
    name: string16;                        // maximum 255 UTF-8 bytes
}

struct GroupState {
    members: Vec<GroupMember>;             // u8 count, maximum 64
    invitations: Vec<GroupInvitation>;     // u8 count, maximum 8
    is_group_open: Option<bool>;
    auto_accept: Option<bool>;
}

struct GroupMember {
    name: string8;                         // 1 through 64 UTF-8 bytes
    is_leader: bool;
}

struct GroupInvitation {
    id: u32;                               // nonzero DLL-lifetime identifier
    inviter: string8;                      // 1 through 64 UTF-8 bytes
    received_tick_ms: Option<u32>;
}

struct CharacterSnapshot {
    id: Option<u32>;
    name: Option<utf8>;
    identity: Option<PlayerIdentity>;
    appearance: Option<CharacterAppearance>;
    class: CharacterClass;
    is_action_restricted: bool;
    is_blinded: bool;
    is_walking: bool;
    is_casting: bool;
    gold: u32;
    weight: u32;
    max_weight: u32;
    progression: CharacterProgression;
    stats: CharacterStats;
    vitals: CharacterVitals;
    modifiers: Option<CharacterModifiers>;
    location: Option<MapLocation>;
    inventory: Option<Vec<InventoryItem>>;
    equipment: Option<Vec<EquipmentItem>>;
    spellbook: Option<Vec<Spell>>;
    skillbook: Option<Vec<Skill>>;
    effects: Option<Vec<Effect>>;
}

struct PlayerIdentity {
    nation: u8;                           // exact Nation value 0 through 13
    title: string16;
    guild_rank: string16;
    display_class: string16;
    guild: string16;
}

struct PlayerProfile {
    identity: PlayerIdentity;
    user_state: u8;
    is_group_open: bool;
    equipment: Vec<PlayerEquipmentItem>; // maximum 18
    legend: Vec<LegendMark>;              // maximum 255
    inspected_tick_ms: u32;
}

struct CharacterAppearance {
    gender: Gender;
    hair_style: u16;
    hair_color: u8;
    body_sprite: u16;
}

struct InventoryItem {
    slot: u8;
    sprite: u16;
    dye_color: u8;
    name: Option<utf8>;
    quantity: u32;
    can_stack: bool;
    durability: u32;
    max_durability: u32;
}

enum EquipmentSlot: u8 {
    Weapon       = 1,
    Armor        = 2,
    Shield       = 3,
    Helmet       = 4,
    Earrings     = 5,
    Necklace     = 6,
    LeftRing     = 7,
    RightRing    = 8,
    LeftGauntlet = 9,
    RightGauntlet = 10,
    Belt         = 11,
    Greaves      = 12,
    Boots        = 13,
    Accessory1   = 14,
    Overcoat     = 15,
    OverHelm     = 16,
    Accessory2   = 17,
    Accessory3   = 18,
}

struct EquipmentItem {
    slot: EquipmentSlot;
    sprite: u16;
    dye_color: u8;
    name: Option<utf8>;
    durability: u32;
    max_durability: u32;
}

struct Spell {
    slot: u8;
    icon: u16;
    name: Option<utf8>;
    level: u8;
    max_level: u8;
    lines: u8;
    target_type: u8;
    prompt: Option<utf8>;
    cooldown: CooldownStatus;
}

struct Effect {
    icon: u16;
    duration: EffectDuration;
}

enum Direction: u8 {
    North = 0,
    East  = 1,
    South = 2,
    West  = 3,
}

enum CreatureKind: u8 {
    Monster = 1,
    Npc     = 2,
}

enum WorldObject: u8 {
    Player = 1 {
        id: u32;
        x: i32;
        y: i32;
        direction: Direction;
        name: Option<utf8>;
        profile: Option<PlayerProfile>; // stored in the appended profile table
    },
    Creature = 2 {
        id: u32;
        x: i32;
        y: i32;
        direction: Direction;
        kind: CreatureKind;
        sprite: Option<u16>;
        name: Option<utf8>;
    },
    Item = 3 {
        id: u32;
        x: i32;
        y: i32;
        sprite: u16;
        z_index: u16;
    },
}

enum EffectDuration: u8 {
    Blue   = 1,
    Green  = 2,
    Yellow = 3,
    Orange = 4,
    Red    = 5,
    White  = 6,
}
```

The dialog, group, exchange, legend, local identity, and visible-player profile
fields were appended during protocol 1.0 development. A 1.0 decoder accepts an
older snapshot ending at any supported tail boundary and treats missing values
as unavailable. New encoders append local identity and a profile table keyed by
visible player ID after the original object records.

Optional values begin with a strict boolean byte. Strings use a `u16` UTF-8
byte length. Character collections use a `u8` count followed by occupied
entries; inventory permits at most 60 entries, equipment 18, each ability book
90, and effects 10. World objects use a `u16` count and permit at most 512
entries. Collection names are limited to 127 bytes, character names to 15 bytes,
world-object names to 63 bytes, and map names to 255 bytes. Slots are one-based,
unique within a slotted collection, and strictly range checked. World-object IDs
are unique. Directions accept only 0 through 3, effect icons are unique, and
duration values outside 1 through 6 are rejected. The overall 4 MiB frame
payload cap still applies even when every individual collection count is valid.

Snapshot scalars use explicit little-endian integer widths. Collection entries
carry their slot, appearance identifier, optional name, and their domain fields:
inventory quantity, stackability, and durability, equipment durability, spell
levels, lines, target type, optional text-input prompt, and cooldown, or skill
levels and cooldown. Equipment slots use numeric values 1 through 18 on the
wire and typed names in public presentation. A cooldown contains an active flag
and an optional wrapping millisecond duration.

Unavailable reason values distinguish an absent hook, a bounded capture
timeout, and a failed state walk. A ready response may still contain absent
groups when the client lifecycle or validated pointers do not expose them.
`Disconnected` means that the client has an active reconnect dialog. It may
still contain character state when the underlying world remains valid.
These snapshot-tail additions remain compatible with their protocol 1.0
encoding. The command and event additions documented below require protocol
1.1.

## Event polling and state updates

The daemon uses bounded long polling rather than unsolicited pipe writes. This
keeps the DLL pipe worker in a simple request and response loop while still
delivering active updates immediately and limiting idle polling to one request
every 50 milliseconds.

```rust,ignore
struct EventPollRequest {
    request_id: u32;
    after_sequence: u32;
    max_events: u16;  // 1 through 192
    wait_ms: u16;     // 0 through 1,000
}

enum EventPollResult {
    Events(Vec<StateEvent>),
    ResyncRequired {
        missing_sequence: u32,
        latest_sequence: u32,
    },
}

struct EventPollResponse {
    request_id: u32;
    result: EventPollResult;
}

struct StateEvent {
    sequence: u32;
    revision: u32;
    tick_ms: u32;
    update: StateUpdate;
}

enum StateUpdate: u8 {
    Status(StatusUpdate) = 1,
    Location(LocationUpdate) = 2,
    Effect(EffectUpdate) = 3,
    Object(ObjectUpdate) = 4,
    Message(ClientMessage) = 5,
    Inventory(SlotUpdate<InventoryItem>) = 6,
    Spellbook(SlotUpdate<Spell>) = 7,
    Skillbook(SlotUpdate<Skill>) = 8,
    Movement(MovementUpdate) = 9,
    Ability(AbilityUpdate) = 10,
    Action(ActionUpdate) = 11,
    Entity(EntityUpdate) = 12,
    Dialog(DialogUpdate) = 13,
    Group(GroupUpdate) = 14,
    Exchange(ExchangeUpdate) = 15,
    Legend(LegendUpdate) = 16,
    Lifecycle(LifecycleUpdate) = 17,
    Audio(AudioUpdate) = 18,
    Command(ClientCommand) = 19,
    Player(PlayerUpdate) = 20,
    CharacterProfile(CharacterProfileUpdate) = 21,
    PlannedRoute(PlannedRoute) = 22,
    MapExclusions(MapExclusionsUpdate) = 23,
}

enum MapExclusionsUpdate: u8 {
    Replaced { exclusions: MapExclusions, map_count: u16 } = 1,
    Removed { map_id: u32, map_count: u16 } = 2,
    Cleared { removed_map_count: u16 } = 3,
}

struct ClientCommand {
    command: string16;
    arg_count: u8;
    args: [string16; arg_count];
}

struct LifecycleUpdate {
    previous: ClientLifecycle;
    current: ClientLifecycle;
}

enum AudioUpdate: u8 {
    SoundPlayed { effect: u8 } = 0,
    MusicStarted { track: u8 } = 1,
    MusicStopped = 2,
}

enum LegendUpdate: u8 {
    MarkAdded { mark: LegendMark } = 1,
    MarkChanged { previous: LegendMark, current: LegendMark } = 2,
    MarkRemoved { mark: LegendMark } = 3,
}

enum ExchangeUpdate: u8 {
    Opened(ExchangeState) = 1,
    ItemAdded { state: ExchangeState, party: ExchangeParty, item: ExchangeItem } = 2,
    GoldChanged { state: ExchangeState, party: ExchangeParty, gold: u32 } = 3,
    Accepted { state: ExchangeState, party: ExchangeParty, message: string16 } = 4,
    Completed { state: ExchangeState, message: string16 } = 5,
    Cancelled { state: ExchangeState, message: string16 } = 6,
}

enum ExchangeParty: u8 {
    Local = 0,
    Other = 1,
}

enum GroupUpdate: u8 {
    InvitationSent { target: string } = 1,
    InvitationReceived { invitation: GroupInvitation, state: GroupState } = 2,
    InvitationClosed {
        invitation: GroupInvitation,
        reason: GroupInvitationCloseReason,
        state: GroupState,
    } = 3,
    Joined { state: GroupState } = 4,
    MemberJoined { member: GroupMember, state: GroupState } = 5,
    MemberLeft { member: GroupMember, state: GroupState } = 6,
    Disbanded { state: GroupState } = 7,
    SettingsChanged { state: GroupState } = 8,
}

enum DialogUpdate: u8 {
    Opened(DialogState) = 1,
    Changed(DialogState) = 2,
    Submitted {
        state: DialogState,
        previous_revision: u32,
        submission: DialogSubmission,
    } = 3,
    Closed {
        previous: Option<DialogState>,
        reason: DialogCloseReason,
    } = 4,
}

enum EntityUpdate: u8 {
    Animated {
        entity: WorldObject,
        animation: u8,
        duration_10ms: u16,
    } = 1,
    Effect {
        entity: WorldObject,
        effect: u16,
        source: Option<WorldObject>,
        frame_interval_ms: Option<i16>,
    } = 2,
    Damaged {
        entity: WorldObject,
        health_percent: u8,
    } = 3,
}

enum CollectionChange: u8 {
    Added = 1,
    Removed = 2,
    Changed = 3,
}

struct SlotUpdate<T> {
    batch_index: u8;          // zero-based position in the batch
    batch_count: u8;          // nonzero total batch size
    change: CollectionChange;
    slot: u8;                 // one-based collection slot
    before: Option<T>;        // field bit 0
    after: Option<T>;         // field bit 1
}

enum MessageKind: u8 {
    Say = 1,
    Shout = 2,
    Whisper = 3,
    Guild = 4,
    Group = 5,
    System = 6,
    World = 7,
    Chant = 8,
}

struct ClientMessage {
    kind: MessageKind;
    sender: Option<String>;     // presence byte, then bounded u16 UTF-8 string
    recipient: Option<String>;  // presence byte, then bounded u16 UTF-8 string
    text: String;               // bounded u16 UTF-8 string
}

enum EffectUpdate: u8 {
    Added(Effect) = 1,
    Removed { icon: u16 } = 2,
    Changed(Effect) = 3,
}

enum ObjectUpdate: u8 {
    Appeared(WorldObject) = 1,
    Disappeared(WorldObject) = 2,
    Moved(WorldObject) = 3,
    DirectionChanged(WorldObject) = 4,
    Cleared = 5,
}

struct StatusUpdate {
    core: Option<CoreStatus>;                 // field bit 0
    vitals: Option<CurrentVitals>;            // field bit 1
    progression: Option<ProgressionStatus>;   // field bit 2
    gold: Option<u32>;                        // field bit 3
    modifiers: Option<CharacterModifiers>;    // field bit 4
    is_blinded: Option<bool>;                 // field bit 5
    is_action_restricted: Option<bool>;       // field bit 6
    is_casting: Option<bool>;                 // field bit 7
}

enum AbilityUpdate: u8 {
    SkillUsed { slot: u8 } = 1,
    SpellBegin { slot: u8, total_lines: u8 } = 2,
    SpellChant { slot: u8, line: u8, total_lines: u8 } = 3,
    SpellCast { slot: u8, arguments: SpellCastArguments } = 4,
    SpellCancelled { slot: u8, source: SpellCancellationSource } = 5,
}

enum ActionUpdate: u8 {
    ItemUsed { slot: u8 } = 1,
    ItemDropped { slot: u8, quantity: u32, position: TilePosition } = 2,
    ItemGiven { slot: u8, quantity: u32, object_id: u32 } = 3,
    GoldDropped { amount: u32, position: TilePosition } = 4,
    GoldGiven { amount: u32, object_id: u32 } = 5,
    ItemPickedUp { destination_slot: u8, position: TilePosition } = 6,
    EquipmentUnequipped { slot: u8 } = 7,
    Emoted { code: u8 } = 8,
    Turned(Direction) = 9,
    Resync = 10,
}

enum SpellCastArguments: u8 {
    None = 0,
    Target { id: Option<u32>, x: i32, y: i32 } = 1,
    Input(String) = 2,
    Values(Vec<u16>) = 3,
    Unknown = 4,
}

enum SpellCancellationSource: u8 {
    Client = 1,
    Server = 2,
    Replaced = 3,
}

struct TilePosition {
    x: i32;
    y: i32;
}

enum MovementUpdate: u8 {
    Started {
        current: TilePosition;
        destination: Option<TilePosition>;
    } = 1,
    Stopped {
        current: TilePosition;
        destination: Option<TilePosition>;
        reached_destination: Option<bool>;
    } = 2,
    Obstructed {
        map_id: u32;
        current: TilePosition;
        attempted: TilePosition;
        direction: Direction;
        destination: Option<TilePosition>;
        mode: WalkMode;
    } = 3,
}

enum WalkMode: u8 {
    Direct = 0,
    NativeRoute = 1,
    ExactRoute = 2,
    Pursuit = 3,
}

struct CoreStatus {
    level: u8;
    ability_level: u8;
    max_health: u32;
    max_mana: u32;
    weight: u32;
    max_weight: u32;
    stats: CharacterStats;
}

struct LocationUpdate {
    x: i32;
    y: i32;
    map: Option<MapChange>;
}

struct MapChange {
    id: u32;
    name: Option<utf8>;
    width: i32;
    height: i32;
}
```

`PlannedRoute` encodes its generation and tile count as little-endian `u32`
values. Each tile uses two little-endian `u16` coordinates on the wire and is
expanded to the public signed coordinate type after validation. The maximum
160,001 tiles matches the supported client's 400 by 400 pathfinder grid plus
the starting tile.

Within an ability update, fields specific to its discriminant are encoded
before the final one-byte slot. Ability slots are strict one-based values from
1 through 90. Spell input is bounded to 100 UTF-8 bytes on observed events;
values contains from one through four `u16` entries.

Message participant names are limited to 15 UTF-8 bytes and message text is
limited to 4 KiB at the protocol boundary. The DLL's observed game messages are
smaller still: the game-thread event queue reserves a fixed 256-byte text field
and ignores a longer displayed line. Invalid UTF-8, unknown message kinds, and
oversized fields reject the containing frame.

Client command names and individual arguments are limited to 255 UTF-8 bytes.
The originating public-speech packet is smaller still because its complete text
uses a one-byte length. The command argument count is encoded as `u8`.

Every included group is an absolute replacement value, not a delta. Most
decoded server packets produce one atomic `StateEvent`. Inventory and ability
packets can affect several slots, so they produce a complete ordered batch of
`StateEvent` values. The DLL never splits one collection batch across poll
responses, and the daemon validates and reduces the full batch before
publishing its new REST state.

Collection updates reuse the snapshot entry encodings. `before` and `after`
describe the exact occupied value on each side of the change; at least one must
be present, and any present entry must match `slot`. A move therefore has one
changed source slot and one changed destination slot. A swap has two changed
slots. A same-slot packet whose resulting value is identical produces no event.
Stack increases and decreases use `Added` and `Removed`; splitting, merging, or
moving an unchanged total uses `Changed`.

The public Server-Sent Events view emits one frame per changed collection slot.
Its `batch_index` and `batch_count` fields preserve the atomic relationship even
though the frames remain individually routable.

Ability cooldown-only transitions are presented as semantic `skill.cooldown`,
`skill.ready`, `spell.cooldown`, or `spell.ready` frames instead of collection
`changed` frames. A simultaneous ability-metadata and cooldown transition emits
both frames with the same event sequence. The DLL watches only submitted or
already-active ability slots. It rereads skills at their exact retained expiry
when available and otherwise polls the watched active slot until it becomes
ready.

A location update contains an absolute accepted position. `map` is absent for
ordinary movement and present when the position completes a map transition.
The latter replaces the map identity, name, dimensions, and coordinates in one
reducer operation, so consumers never observe a new map paired with the prior
map's position.

A movement update describes the native queued-route lifecycle rather than a
single directional step. `current` is copied from the DLL's accepted-position
cache. A destination requested through daRPC is retained until the route stops;
routes started directly through the game may have no known destination. A
stopped update carries `reached_destination` only when the destination is
known. It is true only when the stopped position equals that destination.
Destination presence uses a strict Boolean byte. The stopped outcome uses `0`
for unavailable, `1` for false, and `2` for true, and its presence must match
the destination.

Object updates also carry absolute values. Appeared, disappeared, moved, and
direction-changed updates include the complete object at that boundary.
`Cleared` contains no object and resets the observed collection at a map or
world transition.

`ClientSnapshot.event_sequence` is the event boundary already represented by
the snapshot. A controller discards queued events at or before that boundary
and applies only consecutive later events. `updated_tick_ms` initially equals
`captured_tick_ms` and advances with each applied event, while the capture tick
and duration continue to describe the last complete memory walk.

The DLL stores at most 1 MiB of pointer-free events. Overflow, a nonconsecutive
event sequence, or a nonconsecutive revision yields `ResyncRequired`. The
daemon then requests a fresh snapshot and resumes polling from its new boundary.
No unbounded outage replay log exists. Reconnect always starts with current
state from a fresh snapshot.

## Main-thread commands

Commands use one bounded envelope. The diagnostic records execution metadata
without changing client state. Movement, `skill use`, and `spell cast` commands
carry bounded pointer-free arguments and execute through confirmed native
client functions.

```rust,ignore
struct CommandRequest {
    request_id: u32;
    operation: CommandOperation;
}

enum CommandOperation: u8 {
    Submit {
        kind: CommandKind;
        timeout_ms: u16;  // 1 through 5,000
        wait_ms: u16;     // 0 through 1,000
    } = 0,
    Query {
        command_id: u32;  // nonzero, local to one DLL instance
        wait_ms: u16;     // 0 through 1,000
    } = 1,
    Cancel {
        command_id: u32;
    } = 2,
}

enum CommandKind: u8 {
    Diagnostic = 0,
    Turn(Direction) = 1,
    Walk(WalkTarget) = 2,
    UseSkill { slot: u8 } = 3,  // one-based, 1 through 90
    CastSpell(SpellCast) = 4,
    UseItem { slot: u8 } = 5,    // one-based, 1 through 59
    DropItem(ItemTransfer) = 6,
    DropGold(GoldTransfer) = 7,
    PickupItem(TilePosition) = 8,
    Unequip { slot: u8 } = 9,    // one-based, 1 through 18
    Emote { code: u8 } = 10,
    GiveItem(ItemTransfer) = 11,
    GiveGold(GoldTransfer) = 12,
    SwapSlots(SlotSwap) = 13,
    Interact { id: u32 } = 14,       // nonzero visible Mundane object ID
    Dialog(DialogCommand) = 15,
    Group(GroupCommand) = 16,
    Who = 17,
    Exchange(ExchangeCommand) = 18,
    Chant { text: string8 } = 19,  // 1 through 255 ASCII bytes
    Legend = 20,
    Raw {
        direction: u8;  // 0 = client to server, 1 = server to client
        command: u8;
        payload_length: u8;
        payload: [u8; payload_length];
    } = 21,
    Assail = 22,
    InspectPlayer { id: u32 } = 23, // nonzero visible player object ID
    Resync = 24,
    Message(MessageCommand) = 25,
    SetPathExclusions {
        map_id: u32;               // 0 through 65,535
        tile_count: u16;       // 1 through 256
        tiles: [RouteTile; tile_count];
    } = 26,
    RemovePathExclusions { map_id: u32 } = 27, // 0 through 65,535
    ClearPathExclusions = 28,
}

enum MessageCommand: u8 {
    Say { content: string8 } = 0,
    Shout { content: string8 } = 1,
    Whisper { recipient: string8, content: string8 } = 2,
    Guild { content: string8 } = 3,
    Group { content: string8 } = 4,
}

enum ExchangeCommand: u8 {
    AddItem { slot: u8, quantity: u8 } = 1,
    SetGold { amount: u32 } = 2,
    Accept = 3,
    Cancel = 4,
}

`Chant` becomes the client packet body `0x0E 0x02 string8`, where mode `2` is
the spell-chant channel. Text bytes are preserved exactly. The convenience NPC
actions are controller-side formatters and use this same typed command.

`Message` accepts 1 through 100 ASCII content bytes. Whisper recipients accept
1 through 15 non-whitespace ASCII bytes. Say and shout become `0x0E` packets
with modes `0` and `1`. Whisper becomes `0x19 string8-recipient string8-content`.
Guild and group use that same directed-message packet with fixed recipients `!`
and `!!`.

`Raw` carries a bounded plaintext packet body split into a command byte and up
to 255 payload bytes. It is an intentionally unsafe semantic escape hatch: the
codec validates its direction and bounds, but it cannot validate arbitrary
game packet contents.

`Assail` submits the one-byte client packet body `0x13` through the confirmed
client packet function.

`Resync` submits the opcode-only client refresh packet `0x38`, matching the F5
key behavior.

enum GroupCommand: u8 {
    Invite { target: string8 } = 1,
    Accept { invitation_id: u32 } = 2,
    Decline { invitation_id: u32 } = 3,
    Toggle = 4,
}

struct DialogCommand {
    revision: u32;
    action: DialogAction;
}

enum DialogAction: u8 {
    Select { index: u16, quantity: u8 } = 0,
    Input(String) = 1,       // 1 through 255 ASCII bytes
    Previous = 2,
    Next = 3,
    Close = 4,
}

struct ItemTransfer {
    slot: u8;
    quantity: u32;
    target: TransferTarget;
}

struct GoldTransfer {
    amount: u32;
    target: TransferTarget;
}

enum TransferTarget: u8 {
    Tile(TilePosition) = 0,
    Object { id: u32 } = 1,      // nonzero
}

enum SlotSwap: u8 {
    Inventory { source: u8, destination: u8 } = 0, // 1 through 59
    Spellbook { source: u8, destination: u8 } = 1, // 1 through 90
    Skillbook { source: u8, destination: u8 } = 2, // 1 through 90
}

struct SpellCast {
    slot: u8;                  // one-based, 1 through 90
    arguments: SpellArguments;
}

enum SpellArguments: u8 {
    None = 0,
    ObjectTarget { id: u32 } = 1,  // nonzero
    TileTarget { x: i32, y: i32 } = 2,
    Input(String) = 3,             // 1 through 100 ASCII bytes
}

enum WalkTarget: u8 {
    Direction(Direction) = 0,
    Destination { x: i32, y: i32 } = 1,
    Route {
        map_id: u32;
        tile_count: u16;       // 1 through 256
        tiles: [RouteTile; tile_count];
    } = 2,
}

struct RouteTile {
    x: u16;
    y: u16;
}

enum CommandState: u8 {
    Accepted = 0,
    Executed = 1,
    Failed = 2,
    Cancelled = 3,
    TimedOut = 4,
}

struct CommandStatus {
    command_id: u32;
    kind: CommandKind;
    state: CommandState;
    enqueued_tick_ms: u32;
    deadline_tick_ms: u32;
    started_tick_ms: Option<u32>;
    completed_tick_ms: Option<u32>;
    execution_us: Option<u32>;
    main_thread_id: Option<u32>;
    failure: Option<CommandFailure>;
}

enum CommandFailure: u8 {
    Internal = 0,
    InvalidState = 1,
    InvalidDestination = 2,
    Rejected = 3,
    NoPath = 4,
    InvalidSkill = 5,
    InvalidSpell = 6,
    InvalidArguments = 7,
    InvalidTarget = 8,
}

struct CommandResponse {
    request_id: u32;
    result: CommandResult;
}

enum CommandResult: u8 {
    Status(CommandStatus) = 0,
    Busy = 1,
    NotFound = 2,
    Unavailable = 3,
    Who { status: CommandStatus, list: WhoList } = 4,
    Legend { status: CommandStatus, marks: Vec<LegendMark> } = 5,
    Player { status: CommandStatus, id: u32, profile: PlayerProfile } = 6,
}

struct WhoList {
    world_count: u16;
    country_count: u16;
    players: Vec<WhoPlayer>;       // u16 count, maximum 768
}

struct WhoPlayer {
    name: string8;                 // at most 24 UTF-8 bytes
    title: string8;                // at most 48 UTF-8 bytes
    class: CharacterClass;
    state: UserState;
    color: u8;
    is_master: bool;
    is_guildmate: bool;
}
```

Each optional field is encoded as a strict Boolean followed by its `u32` value
when present. Submission only validates and copies bounded scalar values on the
IPC worker. Execution occurs later through the client tick hook. Directions use
the same strict discriminants as object facing. Destination coordinates are
signed wire values and must satisfy the live zero-based map bounds before native
pathfinding. Skill slots are strict one-based values from 1 through 90; the DLL
also requires the live entry to retain the requested slot before activation.
Spell slots use the same range. The DLL checks the live spell's expected
argument type, current map or object target, action delay, and denial state
before calling the native spell routine. A new spell may replace a delayed cast
already in progress.
Drop commands accept only tile transfers and give commands accept only object
transfers. Slot swaps submit the client's normal `0x30` rearrangement packet
with the collection discriminator followed by source and destination slots.
`Busy` is an
immediate response when all fixed queue entries are pending, and `Unavailable`
means the tick execution path is not installed. Terminal results are retained
for bounded status queries and may be evicted under command pressure.

`Who` submits the client's ordinary server request on the main thread and stays
accepted until its matching response arrives. The result preserves server row
order. Requests share an in-flight or completed command for one second and use
a three-second command deadline. The DLL suppresses the stock Who panel only
for a correlated daRPC request. A player-started request remains untouched.

`InspectPlayer` submits `43 01 <u32be id>` and stays accepted until the matching
`0x34` response arrives. Internal requests are correlated by object ID and send
order. Only their responses skip the stock other-player pane; player-started
requests run the original handler and still refresh the profile cache. Its
terminal command result carries the complete player object captured with the
profile, rather than requiring the caller to merge data from a separate
snapshot.

Command deadlines and queue delay use the same wrapping millisecond tick as
frame timestamps. `execution_us` uses a higher-resolution local duration so a
short diagnostic can still report sub-millisecond work. A disconnect drops no
pointer because queued commands contain no client or controller addresses.

## Ordering and time

Each sender maintains its own sequence counter for each connection. It starts
at zero, increments for every frame, and wraps from 65,535 to zero. A receiver
expects the same progression. A mismatch is a connection-level protocol error;
it does not silently resynchronize.

State-event sequence and revision counters are separate wrapping nonzero `u32`
values. The event sequence orders state mutations; the revision orders both
full snapshots and mutations. They wrap from `u32::MAX` to one. A gap in either
counter causes a fresh snapshot instead of attempting to infer a lost value.

On Windows, the sender tick comes from
[`timeGetTime`](https://learn.microsoft.com/en-us/windows/win32/api/timeapi/nf-timeapi-timegettime),
the millisecond clock also used by the supported client's dispatchers. It is
elapsed time since Windows started, not wall-clock time. It wraps every 2^32
milliseconds, about 49.71 days, so elapsed time is calculated as
`end.wrapping_sub(start)`.

The tick is diagnostic metadata for event comparison, sequencing, round-trip
time, and other observability. Millisecond resolution is sufficient for these
uses. daRPC does not change the system multimedia timer resolution merely to
stamp frames. The codec accepts the tick from its caller and has no Windows
dependency.

Samples carried by different processes are coarse observations and must not be
used alone to assert strict event ordering. `darpc.exe` measures round-trip time
from two `timeGetTime` samples in its own process; the remote request and
response ticks remain visible for comparison and diagnosis.

## Validation rules

Protocol handling is deliberately strict. The codec rejects invalid magic,
unsupported frame versions, unknown message types, nonzero flags, invalid
architecture values, invalid version ranges, invalid UTF-8, truncated fields,
invalid boolean bytes, command discriminants, command limits, zero command
identifiers, oversized lengths, arithmetic overflow, and trailing bytes.
Lengths are checked
before allocation. The session layer rejects unsupported negotiated versions,
invalid message order, mismatched instance IDs, and sequence gaps.

Malformed input returns a structured error. It must never panic, read past the
provided bytes, guess where another frame begins, or silently accept a value it
does not understand.

## Golden Hello frame

The codec tests use this exact 95-byte frame: a 20-byte header followed by a
75-byte `Hello` payload.

```text
44 52 50 43 01 00 01 00 34 12 00 00 12 34 56 78
4b 00 00 00 00 01 00 01 00 01 02 03 04 05 06 07
08 09 0a 0b 0c 0d 0e 0f 44 33 22 11 08 07 06 05
04 03 02 01 01 01 00 00 00 00 00 a0 a1 a2 a3 a4
a5 a6 a7 a8 a9 aa ab ac ad ae af b0 b1 b2 b3 b4
b5 b6 b7 b8 b9 ba bb bc bd be bf e5 02 00 00
```

The fixture uses protocol range 1.0 through 1.0, sequence `0x1234`, sender tick
`0x78563412`, process ID `0x11223344`, process creation time
`0x0102030405060708`, DLL version `1.0.0`, and client version code `741`. Tests both encode
to these bytes and decode them back to the expected values.

## Accepted design decisions

- The 20-byte header is intentionally fixed. No current field justifies making
  it variable or larger.
- The 4 MiB payload cap is bounded but large enough for four maximum native
  route revisions in one event poll. Route coordinates use compact `u16` wire
  values; public API coordinates remain signed integers for consistency.
- Hello identity is sufficient for stale and accidental pipe connections. It is
  not treated as security authentication.
- The `u16` sequence supports ordering diagnostics now and can support a future
  bounded buffer or replay design. Request correlation remains a separate
  `u32` value.
- `timeGetTime` is the shared diagnostic clock because it matches the client and
  provides adequate millisecond resolution for round-trip and sequencing data.
- Unknown values, gaps, and trailing bytes remain strict errors unless real
  interoperability evidence shows that a specific rule should be relaxed. The
  `0x34` player response is one documented exception: the stock client accepts
  a presence-only portrait marker and ignores extension bytes after the bounded
  known fields.
- Echo text remains limited to 4 KiB. A future domain field with a known larger
  bound, such as message-board content near `0x8000` bytes, should receive an
  explicit field-specific limit or chunking design rather than silently lifting
  every string limit.

The implementation maps directly to this chapter: framing is in
`crates/protocol/src/frame/mod.rs`, command messages are in `command/mod.rs`,
remaining message fields are in `message/mod.rs`, handshake and sequence rules
are in `session/mod.rs`, and the exact fixture and malformed-input coverage are
under `crates/protocol/tests/`.
