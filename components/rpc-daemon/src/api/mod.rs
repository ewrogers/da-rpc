use crate::{
    commands::{ClientOperation, CommandCall, ROUTER_CAPACITY},
    dialog::{
        DialogChanged, DialogChoice, DialogCloseReason, DialogClosed, DialogInput,
        DialogInteraction, DialogItem, DialogKind, DialogNavigation, DialogOpened, DialogSlot,
        DialogSnapshot, DialogSpeaker, DialogSpriteType, DialogState, DialogSubmission,
        DialogSubmitted,
    },
    event::DaemonEvent,
    exchange::{
        ExchangeAccepted, ExchangeCancelled, ExchangeCompleted, ExchangeGoldChanged, ExchangeItem,
        ExchangeItemAdded, ExchangeOffer, ExchangeOpened, ExchangeParty, ExchangeSnapshot,
        ExchangeState,
    },
    field_map::{
        FieldMapChanged, FieldMapClosed, FieldMapDestination, FieldMapOpened, FieldMapSelection,
        FieldMapSelectionSubmitted, FieldMapSnapshot, FieldMapState,
    },
    group::{
        GroupDisbanded, GroupInvitation, GroupInvitationCloseReason, GroupInvitationClosed,
        GroupInvitationReceived, GroupInvitationSent, GroupJoined, GroupMember, GroupMemberChanged,
        GroupSettingsChanged, GroupSnapshot, GroupState,
    },
    lifecycle::{
        LaunchOptions as ManagedLaunchOptions, LifecycleControl, LifecycleOperation,
        LifecycleOutcome, ManagementError, ServerEndpoint as ManagedServerEndpoint,
    },
    message_dialog::{
        MessageDialog, MessageDialogsChanged, MessageDialogsSnapshot, MessageDialogsState,
    },
    messages::{
        DEFAULT_MESSAGE_COUNT, MAX_MESSAGE_COUNT, Message, MessageChannel, MessageFilter,
        MessageStore, Messages,
    },
    registry::{
        ClientIdentity as RegistryClientIdentity, ClientSnapshot as RegistryClientSnapshot,
        ClientSnapshotStatus, ConnectionEvent, RegistrySnapshot, architecture, hex,
    },
    resync_status::{ResyncSchedulerStatus, ResyncTrackers},
    state::{
        CharacterClass as SnapshotCharacterClass, CharacterGender, CharacterModifiers,
        CharacterProgression, CharacterStats, CharacterStatus, CharacterVitals,
        ClientLifecycle as SnapshotClientLifecycle, CooldownStatus, Direction, Effect,
        EffectDuration, Effects, Element, Equipment, EquipmentItem, EquipmentSlot, GameStatus,
        Inventory, InventoryItem, MapLocation, ObservationMetadata, Skill, Skillbook, Spell,
        SpellTargetType, Spellbook, WorldObject, WorldObjectKind, WorldObjects,
    },
    stream::{self, ClientEvent, PublishedEvent, SpellFeedbackTrackers},
};
use axum::{
    Json, Router,
    body::Body,
    extract::{
        DefaultBodyLimit, Path, Query, State,
        rejection::{JsonRejection, QueryRejection},
    },
    http::{Method, Request, StatusCode, header},
    middleware::{Next, from_fn},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post, put},
};
use chrono::{DateTime, Utc};
use darpc_game_client::CLIENT_VERSION;
use darpc_model::{ActionUpdate, SequenceNumber, StateUpdate};
use darpc_protocol::{Hello, protocol_version_major, protocol_version_minor};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    io,
    net::{SocketAddrV4, TcpListener},
    path::PathBuf,
    sync::{
        Arc, Mutex, RwLock, Weak,
        atomic::{AtomicU32, Ordering},
        mpsc::{self, Sender, SyncSender, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use tokio::sync::{Mutex as AsyncMutex, broadcast, oneshot};
use utoipa::{IntoParams, OpenApi, ToSchema};
use utoipa_swagger_ui::SwaggerUi;

mod clients;
mod diagnostics;
mod internal_messages;
mod lifecycle;
mod maps;
mod schema;

use clients::*;
pub(crate) use lifecycle::resolve_client;
use lifecycle::{current_character_name, launch, load, resolve_game_snapshot, unload};
pub(crate) use schema::*;

const SWAGGER_INDEX: &str = include_str!("../../assets/swagger.html");
const SWAGGER_THEME: &str = include_str!("../../assets/swagger-ayu.css");
const DEFAULT_SERVER_PORT: u16 = 2610;
const MAX_REQUEST_BODY: usize = 4 * 1024;

#[derive(Clone)]
pub(crate) struct ApiState {
    snapshot: Arc<RwLock<Arc<RegistrySnapshot>>>,
    lifecycle: Arc<dyn LifecycleControl>,
    events: Sender<DaemonEvent>,
    commands: SyncSender<CommandCall>,
    published_events: broadcast::Sender<PublishedEvent>,
    messages: Arc<RwLock<MessageStore>>,
    spell_feedback: Arc<Mutex<SpellFeedbackTrackers>>,
    resyncs: Arc<Mutex<ResyncTrackers>>,
    resync_request_locks: Arc<Mutex<BTreeMap<RegistryClientIdentity, Weak<AsyncMutex<()>>>>>,
    maps_directory: Arc<RwLock<Option<PathBuf>>>,
    internal_message_sequence: Arc<AtomicU32>,
    stat_spends: Arc<Mutex<HashMap<u32, Instant>>>,
}

impl ApiState {
    #[must_use]
    pub(crate) fn new(
        snapshot: RegistrySnapshot,
        lifecycle: Arc<dyn LifecycleControl>,
        events: Sender<DaemonEvent>,
    ) -> Self {
        let (published_events, _) = broadcast::channel(stream::EVENT_CHANNEL_CAPACITY);
        let (commands, command_receiver) = mpsc::sync_channel(ROUTER_CAPACITY);
        drop(command_receiver);
        Self {
            snapshot: Arc::new(RwLock::new(Arc::new(snapshot))),
            lifecycle,
            events,
            commands,
            published_events,
            messages: Arc::new(RwLock::new(MessageStore::default())),
            spell_feedback: Arc::new(Mutex::new(SpellFeedbackTrackers::default())),
            resyncs: Arc::new(Mutex::new(ResyncTrackers::default())),
            resync_request_locks: Arc::new(Mutex::new(BTreeMap::new())),
            maps_directory: Arc::new(RwLock::new(None)),
            internal_message_sequence: Arc::new(AtomicU32::new(0)),
            stat_spends: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    #[must_use]
    pub(crate) fn with_command_sender(mut self, commands: SyncSender<CommandCall>) -> Self {
        self.commands = commands;
        self
    }

    #[must_use]
    pub(crate) fn with_maps_directory(self, directory: Option<PathBuf>) -> Self {
        *self
            .maps_directory
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = directory;
        self
    }

    pub(crate) fn set_maps_directory_if_unset(&self, directory: PathBuf) -> bool {
        let mut current = self
            .maps_directory
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if current.is_some() {
            return false;
        }
        *current = Some(directory);
        true
    }

    pub(crate) fn maps_directory(&self) -> Option<PathBuf> {
        self.maps_directory
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub(crate) fn publish(&self, snapshot: RegistrySnapshot) {
        self.messages
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(&snapshot);
        let mut current = self
            .snapshot
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *current = Arc::new(snapshot);
    }

    #[cfg(test)]
    pub(crate) fn publish_connection_event(&self, event: &ConnectionEvent) {
        let previous = self.snapshot();
        self.publish_connection_event_from(event, &previous);
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) fn publish_connection_event_from(
        &self,
        event: &ConnectionEvent,
        previous: &RegistrySnapshot,
    ) {
        let observed_at_utc = Utc::now();
        self.messages
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .observe(event, observed_at_utc);
        match event {
            ConnectionEvent::Snapshot {
                pid,
                identity,
                snapshot,
            } => {
                let previous_snapshot = previous
                    .clients
                    .iter()
                    .find(|client| client.pid == *pid && client.identity == Some(*identity))
                    .and_then(|client| client.game_snapshot.as_ref());
                if let Some(previous_snapshot) = previous_snapshot {
                    let _ = self.published_events.send(PublishedEvent::Snapshot {
                        pid: *pid,
                        identity: *identity,
                        previous: Box::new(previous_snapshot.clone()),
                        current: snapshot.clone(),
                    });
                }
            }
            ConnectionEvent::StateEvents {
                pid,
                identity,
                events,
            } => {
                let registry = self.snapshot();
                let game_snapshot = registry
                    .clients
                    .iter()
                    .find(|client| client.pid == *pid && client.identity == Some(*identity))
                    .and_then(|client| client.game_snapshot.as_ref());
                let mut previous_game_snapshot = previous
                    .clients
                    .iter()
                    .find(|client| client.pid == *pid && client.identity == Some(*identity))
                    .and_then(|client| client.game_snapshot.clone());
                let mut spell_feedback = self
                    .spell_feedback
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                for state_event in events {
                    match &state_event.update {
                        StateUpdate::Action(ActionUpdate::Resync { resync_id }) => {
                            self.resyncs
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .outgoing(*identity, *resync_id);
                        }
                        StateUpdate::Action(ActionUpdate::ResyncCompleted { resync_id }) => {
                            self.resyncs
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .completed(*identity, *resync_id);
                        }
                        StateUpdate::Action(ActionUpdate::ResyncTimedOut { resync_id }) => {
                            self.resyncs
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .timed_out(*identity, *resync_id);
                        }
                        _ => {}
                    }
                    let replaced_players = previous_game_snapshot
                        .as_ref()
                        .map_or_else(Vec::new, |snapshot| {
                            replaced_players(snapshot, &state_event.update)
                        });
                    if previous_game_snapshot
                        .as_mut()
                        .is_some_and(|snapshot| snapshot.apply_event(state_event.clone()).is_err())
                    {
                        previous_game_snapshot = None;
                    }
                    let (ability_name, target_name) =
                        ability_context(game_snapshot, &state_event.update);
                    let feedback = spell_feedback.observe(
                        *identity,
                        game_snapshot,
                        state_event,
                        ability_name.as_deref(),
                        target_name.as_deref(),
                    );
                    let _ = self.published_events.send(PublishedEvent::State {
                        pid: *pid,
                        identity: *identity,
                        event: Box::new(state_event.clone()),
                        replaced_players,
                        ability_name,
                        target_name,
                        feedback: feedback.map(Box::new),
                        observed_at_utc,
                    });
                }
            }
            ConnectionEvent::Disconnected {
                pid,
                identity: Some(identity),
                reason,
            }
            | ConnectionEvent::Incompatible {
                pid,
                identity: Some(identity),
                reason,
            } => {
                self.spell_feedback
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .remove(*identity);
                self.resyncs
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .remove(*identity);
                let _ = self.published_events.send(PublishedEvent::Closed {
                    pid: *pid,
                    identity: *identity,
                    reason: reason.clone(),
                });
            }
            _ => {}
        }
    }

    fn subscribe(&self) -> broadcast::Receiver<PublishedEvent> {
        self.published_events.subscribe()
    }

    fn messages(&self, identity: RegistryClientIdentity, filter: &MessageFilter) -> Messages {
        self.messages
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(identity, filter)
    }

    pub(crate) fn accept_resync(
        &self,
        identity: RegistryClientIdentity,
        resync_id: u32,
    ) -> ResyncSchedulerStatus {
        self.resyncs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .accepted(identity, resync_id)
    }

    pub(crate) fn resync_status(&self, identity: RegistryClientIdentity) -> ResyncSchedulerStatus {
        self.resyncs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .status(identity)
    }

    pub(crate) fn resync_request_lock(
        &self,
        identity: RegistryClientIdentity,
    ) -> Arc<AsyncMutex<()>> {
        let mut locks = self
            .resync_request_locks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        locks.retain(|_, lock| lock.strong_count() != 0);
        if let Some(lock) = locks.get(&identity).and_then(Weak::upgrade) {
            return lock;
        }
        let lock = Arc::new(AsyncMutex::new(()));
        locks.insert(identity, Arc::downgrade(&lock));
        lock
    }

    fn publish_internal_message(
        &self,
        identities: Vec<RegistryClientIdentity>,
        recipient: Option<String>,
        payload: serde_json::Map<String, serde_json::Value>,
    ) {
        if identities.is_empty() {
            return;
        }
        let previous = self
            .internal_message_sequence
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                Some(SequenceNumber::new(value).next().get())
            })
            .expect("internal message sequence update is infallible");
        let sequence = SequenceNumber::new(previous).next().get();
        let observed_at_utc = Utc::now();
        let message = Message::internal(
            sequence,
            observed_at_utc,
            recipient.clone(),
            payload.clone(),
        );
        self.messages
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push_internal(&identities, sequence, observed_at_utc, recipient, payload);
        let _ = self.published_events.send(PublishedEvent::Internal {
            recipients: identities.into(),
            message,
        });
    }

    pub(crate) fn snapshot(&self) -> Arc<RegistrySnapshot> {
        self.snapshot
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub(crate) fn reserve_stat_spend(&self, pid: u32) -> bool {
        let now = Instant::now();
        let cooldown = Duration::from_millis(500);
        let mut spends = self
            .stat_spends
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        spends.retain(|_, spent_at| now.duration_since(*spent_at) < cooldown);
        if spends.contains_key(&pid) {
            return false;
        }
        spends.insert(pid, now);
        true
    }

    fn emit(&self, event: DaemonEvent) -> Result<(), ApiError> {
        self.events.send(event).map_err(|_| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "daemon_unavailable",
                "daemon state manager is unavailable",
                None,
            )
        })
    }

    pub(crate) fn route_command(
        &self,
        pid: u32,
        identity: RegistryClientIdentity,
        operation: darpc_protocol::CommandOperation,
    ) -> Result<oneshot::Receiver<crate::commands::CommandReply>, ApiError> {
        self.route_client(pid, identity, ClientOperation::Command(operation))
    }

    pub(crate) fn route_snapshot(
        &self,
        pid: u32,
        identity: RegistryClientIdentity,
    ) -> Result<oneshot::Receiver<crate::commands::CommandReply>, ApiError> {
        self.route_client(pid, identity, ClientOperation::Snapshot)
    }

    pub(crate) fn route_diagnostics(
        &self,
        pid: u32,
        identity: RegistryClientIdentity,
        operation: darpc_protocol::DiagnosticsOperation,
    ) -> Result<oneshot::Receiver<crate::commands::CommandReply>, ApiError> {
        self.route_client(pid, identity, ClientOperation::Diagnostics(operation))
    }

    fn route_client(
        &self,
        pid: u32,
        identity: RegistryClientIdentity,
        operation: ClientOperation,
    ) -> Result<oneshot::Receiver<crate::commands::CommandReply>, ApiError> {
        let (reply, receiver) = oneshot::channel();
        let call = CommandCall {
            pid,
            identity,
            operation,
            reply,
        };
        match self.commands.try_send(call) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                return Err(ApiError::new(
                    StatusCode::TOO_MANY_REQUESTS,
                    "command_router_full",
                    "the bounded daemon command router is full",
                    Some(pid),
                ));
            }
            Err(TrySendError::Disconnected(_)) => {
                return Err(ApiError::new(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "daemon_unavailable",
                    "daemon command routing is unavailable",
                    Some(pid),
                ));
            }
        }
        self.emit(DaemonEvent::CommandsReady)?;
        Ok(receiver)
    }
}

fn replaced_players(
    snapshot: &darpc_model::ClientSnapshot,
    update: &darpc_model::StateUpdate,
) -> Vec<darpc_model::WorldObject> {
    let darpc_model::StateUpdate::Object(darpc_model::ObjectUpdate::Appeared(
        darpc_model::WorldObject::Player {
            id,
            name: Some(name),
            ..
        },
    )) = update
    else {
        return Vec::new();
    };
    snapshot.objects.as_ref().map_or_else(Vec::new, |objects| {
        objects
            .iter()
            .filter(|object| {
                matches!(
                    object,
                    darpc_model::WorldObject::Player {
                        id: current_id,
                        name: Some(current_name),
                        ..
                    } if current_id != id && current_name == name
                )
            })
            .cloned()
            .collect()
    })
}

fn ability_context(
    snapshot: Option<&darpc_model::ClientSnapshot>,
    update: &darpc_model::StateUpdate,
) -> (Option<String>, Option<String>) {
    let Some(snapshot) = snapshot else {
        return (None, None);
    };
    let Some(character) = snapshot.character.as_ref() else {
        return (None, None);
    };
    let (slot, skill, target_id) = match update {
        darpc_model::StateUpdate::Ability(darpc_model::AbilityUpdate::SkillUsed { slot }) => {
            (*slot, true, None)
        }
        darpc_model::StateUpdate::Ability(darpc_model::AbilityUpdate::SpellCast {
            slot,
            arguments: darpc_model::SpellCastArguments::Target { id, .. },
        }) => (*slot, false, *id),
        darpc_model::StateUpdate::Ability(
            darpc_model::AbilityUpdate::SpellBegin { slot, .. }
            | darpc_model::AbilityUpdate::SpellChant { slot, .. }
            | darpc_model::AbilityUpdate::SpellCast { slot, .. }
            | darpc_model::AbilityUpdate::SpellCancelled { slot, .. },
        ) => (*slot, false, None),
        _ => return (None, None),
    };
    let ability_name = if skill {
        character.skillbook.as_ref().and_then(|abilities| {
            abilities
                .iter()
                .find(|ability| ability.slot == slot)
                .and_then(|ability| ability.name.clone())
        })
    } else {
        character.spellbook.as_ref().and_then(|abilities| {
            abilities
                .iter()
                .find(|ability| ability.slot == slot)
                .and_then(|ability| ability.name.clone())
        })
    };
    let target_name = target_id.and_then(|target_id| {
        if character.id == Some(target_id) {
            return character.name.clone();
        }
        snapshot
            .objects
            .as_ref()?
            .iter()
            .find_map(|object| match object {
                darpc_model::WorldObject::Player { id, name, .. }
                | darpc_model::WorldObject::Creature { id, name, .. }
                    if *id == target_id =>
                {
                    name.clone()
                }
                _ => None,
            })
    });
    (ability_name, target_name)
}

pub(crate) fn start(address: SocketAddrV4, state: ApiState) -> io::Result<JoinHandle<()>> {
    let listener = TcpListener::bind(address)?;
    listener.set_nonblocking(true)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()?;

    thread::Builder::new()
        .name("darpcd-http".into())
        .spawn(move || {
            runtime.block_on(async move {
                let listener = match tokio::net::TcpListener::from_std(listener) {
                    Ok(listener) => listener,
                    Err(error) => {
                        eprintln!("darpcd: HTTP listener failed: {error}");
                        return;
                    }
                };
                if let Err(error) = axum::serve(listener, router(state)).await {
                    eprintln!("darpcd: HTTP server failed: {error}");
                }
            });
        })
}

fn router(state: ApiState) -> Router {
    Router::<ApiState>::new()
        .route("/health", get(health))
        .route("/maps/{map_id}/download", get(maps::download))
        .route("/clients", get(clients))
        .route("/messages/send", post(internal_messages::send))
        .route("/clients/{client}/status", get(client_status))
        .route(
            "/clients/{client}/diagnostics/hooks",
            get(diagnostics::hooks),
        )
        .route("/clients/{client}/diagnostics", put(diagnostics::update))
        .route(
            "/clients/{client}/messages/send",
            post(crate::commands::message::send),
        )
        .route("/clients/{client}/dialog", get(client_dialog))
        .route("/clients/{client}/field-map", get(client_field_map))
        .route(
            "/clients/{client}/message-dialogs",
            get(client_message_dialogs),
        )
        .route(
            "/clients/{client}/field-map/select",
            post(crate::commands::field_map::select),
        )
        .route(
            "/clients/{client}/message-dialogs/dismiss",
            post(crate::commands::message_dialog::dismiss),
        )
        .route("/clients/{client}/group", get(client_group))
        .route("/clients/{client}/exchange", get(client_exchange))
        .route(
            "/clients/{client}/exchange/items",
            post(crate::commands::add_exchange_item),
        )
        .route(
            "/clients/{client}/exchange/gold",
            post(crate::commands::set_exchange_gold),
        )
        .route(
            "/clients/{client}/exchange/accept",
            post(crate::commands::accept_exchange),
        )
        .route(
            "/clients/{client}/exchange/cancel",
            post(crate::commands::cancel_exchange),
        )
        .route("/clients/{client}/who", get(crate::commands::who))
        .route("/clients/{client}/legend", get(crate::commands::legend))
        .route(
            "/clients/{client}/players/{player}",
            get(crate::commands::cached_player),
        )
        .route(
            "/clients/{client}/players/{player}/inspect",
            post(crate::commands::inspect_player),
        )
        .route(
            "/clients/{client}/group/invite",
            post(crate::commands::invite_group),
        )
        .route(
            "/clients/{client}/group/toggle",
            post(crate::commands::toggle_group),
        )
        .route(
            "/clients/{client}/group/invitations/{invitation_id}/accept",
            post(crate::commands::accept_group_invitation),
        )
        .route(
            "/clients/{client}/group/invitations/{invitation_id}/decline",
            post(crate::commands::decline_group_invitation),
        )
        .route(
            "/clients/{client}/interact",
            post(crate::commands::interact),
        )
        .route(
            "/clients/{client}/dialog/select",
            post(crate::commands::dialog_select),
        )
        .route(
            "/clients/{client}/dialog/input",
            post(crate::commands::dialog_input),
        )
        .route(
            "/clients/{client}/dialog/previous",
            post(crate::commands::dialog_previous),
        )
        .route(
            "/clients/{client}/dialog/next",
            post(crate::commands::dialog_next),
        )
        .route(
            "/clients/{client}/dialog/close",
            post(crate::commands::close_dialog),
        )
        .route("/clients/{client}/items", get(client_items))
        .route(
            "/clients/{client}/items/use",
            post(crate::commands::use_item),
        )
        .route(
            "/clients/{client}/items/drop",
            post(crate::commands::drop_item),
        )
        .route(
            "/clients/{client}/items/give",
            post(crate::commands::give_item),
        )
        .route(
            "/clients/{client}/items/swap",
            post(crate::commands::swap_items),
        )
        .route(
            "/clients/{client}/items/pickup",
            post(crate::commands::pickup_item),
        )
        .route(
            "/clients/{client}/chant",
            post(crate::commands::chant::chant),
        )
        .route(
            "/clients/{client}/items/sell",
            post(crate::commands::chant::sell),
        )
        .route(
            "/clients/{client}/items/sell-all",
            post(crate::commands::chant::sell_all),
        )
        .route(
            "/clients/{client}/items/deposit",
            post(crate::commands::chant::deposit),
        )
        .route(
            "/clients/{client}/items/withdraw",
            post(crate::commands::chant::withdraw),
        )
        .route(
            "/clients/{client}/items/repair",
            post(crate::commands::chant::repair),
        )
        .route(
            "/clients/{client}/items/repair-all",
            post(crate::commands::chant::repair_all),
        )
        .route("/clients/{client}/equipment", get(client_equipment))
        .route(
            "/clients/{client}/equipment/unequip",
            post(crate::commands::unequip),
        )
        .route(
            "/clients/{client}/gold/drop",
            post(crate::commands::drop_gold),
        )
        .route(
            "/clients/{client}/gold/give",
            post(crate::commands::give_gold),
        )
        .route("/clients/{client}/emote", post(crate::commands::emote))
        .route(
            "/clients/{client}/raw/send",
            post(crate::commands::raw::send),
        )
        .route(
            "/clients/{client}/assail",
            post(crate::commands::assail::assail),
        )
        .route(
            "/clients/{client}/stats/{stat}",
            post(crate::commands::stat::add),
        )
        .route(
            "/clients/{client}/resync",
            post(crate::commands::resync::resync),
        )
        .route("/clients/{client}/spells", get(client_spells))
        .route("/clients/{client}/skills", get(client_skills))
        .route("/clients/{client}/effects", get(client_effects))
        .route("/clients/{client}/objects", get(client_objects))
        .route("/clients/{client}/messages", get(client_messages))
        .route("/clients/{client}/events", get(client_events))
        .route(
            "/clients/{client}/commands/diagnostic",
            post(crate::commands::diagnostic),
        )
        .route("/clients/{client}/turn", post(crate::commands::turn))
        .route(
            "/clients/{client}/walk",
            post(crate::commands::walk).delete(crate::commands::cancel_walk),
        )
        .route(
            "/clients/{client}/skills/use",
            post(crate::commands::use_skill),
        )
        .route(
            "/clients/{client}/skills/swap",
            post(crate::commands::swap_skills),
        )
        .route(
            "/clients/{client}/spells/cast",
            post(crate::commands::cast_spell),
        )
        .route(
            "/clients/{client}/spells/swap",
            post(crate::commands::swap_spells),
        )
        .route(
            "/clients/{client}/commands/{command_id}",
            get(crate::commands::status).delete(crate::commands::cancel),
        )
        .route("/clients/launch", post(launch))
        .route("/clients/{client}/load", post(load))
        .route("/clients/{client}/unload", post(unload))
        .route("/docs", get(swagger_redirect))
        .route("/docs/", get(swagger_index))
        .route("/docs/ayu.css", get(swagger_theme))
        .merge(SwaggerUi::new("/docs/assets").url("/openapi.json", openapi()))
        .layer(from_fn(reject_request_body))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY))
        .with_state(state)
}

async fn swagger_redirect() -> Redirect {
    Redirect::to("/docs/")
}

async fn swagger_index() -> impl IntoResponse {
    ([(header::CACHE_CONTROL, "no-store")], Html(SWAGGER_INDEX))
}

async fn swagger_theme() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        SWAGGER_THEME,
    )
}

async fn reject_request_body(request: Request<Body>, next: Next) -> Response {
    if (request.method() == Method::POST
        && (request.uri().path() == "/clients/launch"
            || request.uri().path().ends_with("/commands/diagnostic")
            || request.uri().path().ends_with("/turn")
            || request.uri().path().ends_with("/walk")
            || request.uri().path().ends_with("/skills/use")
            || request.uri().path().ends_with("/chant")
            || request.uri().path().ends_with("/messages/send")
            || request.uri().path().ends_with("/group/invite")
            || request.uri().path().ends_with("/group/toggle")
            || request.uri().path().ends_with("/items/sell")
            || request.uri().path().ends_with("/items/sell-all")
            || request.uri().path().ends_with("/items/deposit")
            || request.uri().path().ends_with("/items/withdraw")
            || request.uri().path().ends_with("/items/repair")
            || request.uri().path().ends_with("/skills/swap")
            || request.uri().path().ends_with("/spells/cast")
            || request.uri().path().ends_with("/spells/swap")
            || request.uri().path().ends_with("/items/use")
            || request.uri().path().ends_with("/items/drop")
            || request.uri().path().ends_with("/items/give")
            || request.uri().path().ends_with("/items/swap")
            || request.uri().path().ends_with("/items/pickup")
            || request.uri().path().ends_with("/equipment/unequip")
            || request.uri().path().ends_with("/gold/drop")
            || request.uri().path().ends_with("/gold/give")
            || request.uri().path().ends_with("/exchange/items")
            || request.uri().path().ends_with("/exchange/gold")
            || request.uri().path().ends_with("/emote")
            || request.uri().path().ends_with("/raw/send")
            || request.uri().path().ends_with("/interact")
            || request.uri().path().ends_with("/dialog/select")
            || request.uri().path().ends_with("/dialog/input")
            || request.uri().path().ends_with("/dialog/previous")
            || request.uri().path().ends_with("/dialog/next")
            || request.uri().path().ends_with("/dialog/close")
            || request.uri().path().ends_with("/message-dialogs/dismiss")
            || request.uri().path().ends_with("/field-map/select")))
        || (request.method() == Method::PUT && request.uri().path().ends_with("/diagnostics"))
    {
        return next.run(request).await;
    }
    if request.headers().contains_key(header::TRANSFER_ENCODING) {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    }
    if let Some(length) = request.headers().get(header::CONTENT_LENGTH) {
        let Some(length) = length
            .to_str()
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
        else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        if length != 0 {
            return StatusCode::PAYLOAD_TOO_LARGE.into_response();
        }
    }
    next.run(request).await
}

pub(crate) fn openapi() -> utoipa::openapi::OpenApi {
    let mut document = ApiDoc::openapi();
    document.info.title = "daRPC API".into();
    document
}

#[derive(OpenApi)]
#[openapi(
    paths(
        health,
        maps::download,
        clients,
        client_status,
        diagnostics::hooks,
        diagnostics::update,
        client_dialog,
        client_field_map,
        client_group,
        client_exchange,
        crate::commands::who::who,
        crate::commands::legend::legend,
        crate::commands::player::cached_player,
        crate::commands::player::inspect_player,
        client_items,
        client_equipment,
        client_spells,
        client_skills,
        client_effects,
        client_objects,
        client_messages,
        client_events,
        lifecycle::load,
        lifecycle::unload,
        lifecycle::launch,
        crate::commands::diagnostic,
        crate::commands::movement::turn,
        crate::commands::movement::walk,
        crate::commands::movement::cancel_walk,
        crate::commands::ability::use_skill,
        crate::commands::ability::swap_skills,
        crate::commands::ability::cast_spell,
        crate::commands::ability::swap_spells,
        crate::commands::interaction::use_item,
        crate::commands::interaction::drop_item,
        crate::commands::interaction::give_item,
        crate::commands::interaction::swap_items,
        crate::commands::interaction::drop_gold,
        crate::commands::interaction::give_gold,
        crate::commands::interaction::pickup_item,
        crate::commands::interaction::unequip,
        crate::commands::interaction::emote,
        crate::commands::raw::send,
        crate::commands::assail::assail,
        crate::commands::stat::add,
        crate::commands::resync::resync,
        crate::commands::message::send,
        internal_messages::send,
        crate::commands::chant::chant,
        crate::commands::chant::sell,
        crate::commands::chant::sell_all,
        crate::commands::chant::deposit,
        crate::commands::chant::withdraw,
        crate::commands::chant::repair,
        crate::commands::chant::repair_all,
        crate::commands::dialog::interact,
        crate::commands::dialog::select,
        crate::commands::dialog::input,
        crate::commands::dialog::previous,
        crate::commands::dialog::next,
        crate::commands::dialog::close,
        crate::commands::field_map::select,
        crate::commands::message_dialog::dismiss,
        crate::commands::group::invite,
        crate::commands::group::toggle,
        crate::commands::group::accept,
        crate::commands::group::decline,
        crate::commands::exchange::add_item,
        crate::commands::exchange::set_gold,
        crate::commands::exchange::accept,
        crate::commands::exchange::cancel,
        crate::commands::status,
        crate::commands::cancel
    ),
    components(schemas(
        HealthState,
        HealthStatus,
        ClientList,
        ClientState,
        ClientStatus,
        ClientIdentity,
        ConnectionMetadata,
        ObservationMetadata,
        GameStatus,
        SnapshotClientLifecycle,
        CharacterStatus,
        CharacterGender,
        SnapshotCharacterClass,
        CharacterProgression,
        CharacterStats,
        CharacterVitals,
        CharacterModifiers,
        Element,
        MapLocation,
        Inventory,
        InventoryItem,
        Equipment,
        EquipmentItem,
        EquipmentSlot,
        Spellbook,
        Spell,
        Skillbook,
        Skill,
        CooldownStatus,
        SpellTargetType,
        Effects,
        Effect,
        EffectDuration,
        WorldObjects,
        WorldObject,
        Direction,
        Messages,
        Message,
        MessageChannel,
        internal_messages::InternalMessageChannel,
        internal_messages::InternalMessageOptions,
        internal_messages::InternalMessageResult,
        DialogSnapshot,
        DialogState,
        DialogKind,
        DialogSpeaker,
        DialogSpriteType,
        DialogNavigation,
        DialogInteraction,
        DialogChoice,
        DialogInput,
        DialogItem,
        DialogSlot,
        DialogOpened,
        DialogChanged,
        DialogSubmitted,
        DialogClosed,
        DialogSubmission,
        DialogCloseReason,
        crate::commands::field_map::FieldMapSelectOptions,
        crate::commands::message_dialog::MessageDialogDismissOptions,
        FieldMapSnapshot,
        FieldMapState,
        FieldMapDestination,
        FieldMapSelection,
        FieldMapOpened,
        FieldMapChanged,
        FieldMapSelectionSubmitted,
        FieldMapClosed,
        MessageDialogsSnapshot,
        MessageDialogsState,
        MessageDialog,
        MessageDialogsChanged,
        GroupSnapshot,
        GroupState,
        GroupMember,
        GroupInvitation,
        GroupInvitationSent,
        GroupInvitationReceived,
        GroupInvitationClosed,
        GroupInvitationCloseReason,
        GroupSettingsChanged,
        GroupJoined,
        GroupMemberChanged,
        GroupDisbanded,
        ExchangeSnapshot,
        ExchangeState,
        ExchangeOffer,
        ExchangeItem,
        ExchangeParty,
        ExchangeOpened,
        ExchangeItemAdded,
        ExchangeGoldChanged,
        ExchangeAccepted,
        ExchangeCompleted,
        ExchangeCancelled,
        crate::stream::LegendMarkAdded,
        crate::stream::LegendMarkChanged,
        crate::stream::LegendMarkRemoved,
        crate::commands::WhoList,
        crate::commands::WhoPlayer,
        crate::commands::WhoClass,
        crate::commands::WhoUserState,
        crate::commands::LegendSnapshot,
        crate::commands::LegendMark,
        crate::commands::LegendIcon,
        LaunchOptions,
        LoadResult,
        UnloadResult,
        LifecycleResult,
        LifecycleAction,
        ErrorState,
        ErrorDetail,
        diagnostics::DiagnosticsOptions,
        diagnostics::DiagnosticsState,
        diagnostics::DiagnosticsMode,
        diagnostics::HookTiming,
        diagnostics::HookTimingStage,
        ClientEvent,
        crate::stream::ClientLifecycleChanged,
        crate::stream::SoundPlayed,
        crate::stream::MusicStarted,
        crate::stream::MusicStopped,
        crate::stream::CharacterAppearanceChanged,
        crate::stream::CharacterAppearance,
        crate::stream::CharacterHiddenChanged,
        crate::stream::WalkingObstructed,
        crate::stream::WalkingMode,
        crate::stream::WalkingStopReason,
        crate::commands::DiagnosticOptions,
        crate::commands::raw::RawDirection,
        crate::commands::raw::RawSendOptions,
        crate::commands::GroupInviteOptions,
        crate::commands::AddExchangeItemOptions,
        crate::commands::SetExchangeGoldOptions,
        crate::commands::ActionDirection,
        crate::commands::TurnOptions,
        crate::commands::WalkDirectionOptions,
        crate::commands::Destination,
        crate::commands::WalkDestinationOptions,
        crate::commands::WalkOptions,
        crate::commands::RouteOptions,
        crate::commands::WalkRouteOptions,
        crate::commands::SkillSlotOptions,
        crate::commands::SkillNameOptions,
        crate::commands::UseSkillOptions,
        crate::commands::SpellTargetOptions,
        crate::commands::CastSpellBySlot,
        crate::commands::CastSpellByName,
        crate::commands::CastSpellOptions,
        crate::commands::UseItemOptions,
        crate::commands::DropItemOptions,
        crate::commands::GiveItemOptions,
        crate::commands::DropGoldOptions,
        crate::commands::GiveGoldOptions,
        crate::commands::SlotSelector,
        crate::commands::SwapSlotsOptions,
        crate::commands::PickupItemOptions,
        crate::commands::UnequipOptions,
        crate::commands::EmoteOptions,
        crate::commands::ChantOptions,
        crate::commands::ItemChantOptions,
        crate::commands::SendMessageChannel,
        crate::commands::SendMessageOptions,
        crate::commands::InteractOptions,
        crate::commands::DialogSelectOptions,
        crate::commands::DialogInputOptions,
        crate::commands::DialogRevisionOptions,
        crate::commands::CommandStatus,
        crate::commands::CommandKind,
        crate::commands::CommandState,
        crate::commands::CommandFailure
    ))
)]
struct ApiDoc;

#[cfg(test)]
mod tests;
