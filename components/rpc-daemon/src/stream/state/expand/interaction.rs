use super::*;

pub(super) fn expand(observation: EventObservation, update: StateUpdate) -> Vec<ClientEvent> {
    let mut events = Vec::with_capacity(1);
    match update {
        StateUpdate::Dialog(update) => {
            events.push(match update {
                darpc_model::DialogUpdate::Opened(dialog) => {
                    ClientEvent::DialogOpened(DialogOpened::new(observation, dialog))
                }
                darpc_model::DialogUpdate::Changed(dialog) => {
                    ClientEvent::DialogChanged(DialogChanged::new(observation, dialog))
                }
                darpc_model::DialogUpdate::Submitted {
                    state,
                    previous_revision,
                    submission,
                } => ClientEvent::DialogSubmitted(DialogSubmitted::new(
                    observation,
                    previous_revision,
                    state,
                    submission,
                )),
                darpc_model::DialogUpdate::Closed { previous, reason } => {
                    ClientEvent::DialogClosed(DialogClosed::new(observation, previous, reason))
                }
            });
            events
        }
        StateUpdate::Group(update) => {
            events.push(match update {
                darpc_model::GroupUpdate::SettingsChanged { state } => {
                    ClientEvent::GroupSettingsChanged(GroupSettingsChanged::new(observation, state))
                }
                darpc_model::GroupUpdate::InvitationSent { target } => {
                    ClientEvent::GroupInvitationSent(GroupInvitationSent::new(observation, target))
                }
                darpc_model::GroupUpdate::InvitationReceived { invitation, state } => {
                    ClientEvent::GroupInvitationReceived(GroupInvitationReceived::new(
                        observation,
                        invitation,
                        state,
                    ))
                }
                darpc_model::GroupUpdate::InvitationClosed {
                    invitation,
                    reason,
                    state,
                } => ClientEvent::GroupInvitationClosed(GroupInvitationClosed::new(
                    observation,
                    invitation,
                    reason,
                    state,
                )),
                darpc_model::GroupUpdate::Joined { state } => {
                    ClientEvent::GroupJoined(GroupJoined::new(observation, state))
                }
                darpc_model::GroupUpdate::MemberJoined { member, state } => {
                    ClientEvent::GroupMemberJoined(GroupMemberChanged::new(
                        observation,
                        member,
                        state,
                    ))
                }
                darpc_model::GroupUpdate::MemberLeft { member, state } => {
                    ClientEvent::GroupMemberLeft(GroupMemberChanged::new(
                        observation,
                        member,
                        state,
                    ))
                }
                darpc_model::GroupUpdate::Disbanded { state } => {
                    ClientEvent::GroupDisbanded(GroupDisbanded::new(observation, state))
                }
            });
            events
        }
        StateUpdate::Exchange(update) => {
            events.push(match update {
                darpc_model::ExchangeUpdate::Opened(state) => {
                    ClientEvent::ExchangeOpened(ExchangeOpened::new(observation, state))
                }
                darpc_model::ExchangeUpdate::ItemAdded { state, party, item } => {
                    ClientEvent::ExchangeItemAdded(ExchangeItemAdded::new(
                        observation,
                        state,
                        party,
                        item,
                    ))
                }
                darpc_model::ExchangeUpdate::GoldChanged { state, party, gold } => {
                    ClientEvent::ExchangeGoldChanged(ExchangeGoldChanged::new(
                        observation,
                        state,
                        party,
                        gold,
                    ))
                }
                darpc_model::ExchangeUpdate::Accepted {
                    state,
                    party,
                    message,
                } => ClientEvent::ExchangeAccepted(ExchangeAccepted::new(
                    observation,
                    state,
                    party,
                    message,
                )),
                darpc_model::ExchangeUpdate::Completed { state, message } => {
                    ClientEvent::ExchangeCompleted(ExchangeCompleted::new(
                        observation,
                        state,
                        message,
                    ))
                }
                darpc_model::ExchangeUpdate::Cancelled { state, message } => {
                    ClientEvent::ExchangeCancelled(ExchangeCancelled::new(
                        observation,
                        state,
                        message,
                    ))
                }
            });
            events
        }
        StateUpdate::Legend(update) => {
            events.push(match update {
                darpc_model::LegendUpdate::MarkAdded { mark } => {
                    ClientEvent::LegendMarkAdded(LegendMarkAdded {
                        observation,
                        mark: mark.into(),
                    })
                }
                darpc_model::LegendUpdate::MarkChanged { previous, current } => {
                    ClientEvent::LegendMarkChanged(LegendMarkChanged {
                        observation,
                        previous: previous.into(),
                        current: current.into(),
                    })
                }
                darpc_model::LegendUpdate::MarkRemoved { mark } => {
                    ClientEvent::LegendMarkRemoved(LegendMarkRemoved {
                        observation,
                        mark: mark.into(),
                    })
                }
            });
            events
        }
        _ => unreachable!("interaction expansion received a non-interaction update"),
    }
}
