use darpc_model::{TilePosition, WalkMode};
use darpc_protocol::{ExactRouteInvalidState, ExactRouteInvalidStateReason};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RouteActivation {
    AdvanceNow,
    AwaitStepCompletion,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RouteReplacement {
    pub(crate) origin: TilePosition,
    pub(crate) activation: RouteActivation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LocalMovementTransition {
    committed: TilePosition,
    staged: TilePosition,
    active: bool,
}

impl LocalMovementTransition {
    pub(crate) const fn new(committed: TilePosition, staged: TilePosition, active: bool) -> Self {
        Self {
            committed,
            staged,
            active,
        }
    }

    pub(crate) const fn direct_walk_allowed(self) -> bool {
        !self.active
    }

    pub(crate) const fn committed_position(self) -> TilePosition {
        self.committed
    }

    pub(crate) const fn staged_position(self) -> TilePosition {
        self.staged
    }

    pub(crate) const fn is_active(self) -> bool {
        self.active
    }

    pub(crate) const fn route_replacement(self) -> RouteReplacement {
        if self.active {
            RouteReplacement {
                origin: self.staged,
                activation: RouteActivation::AwaitStepCompletion,
            }
        } else {
            RouteReplacement {
                origin: self.committed,
                activation: RouteActivation::AdvanceNow,
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ConfirmedLocation {
    pub(crate) map_id: u32,
    pub(crate) position: TilePosition,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ExactRouteState {
    pub(crate) route_map_id: u32,
    pub(crate) native_map_id: Option<u32>,
    pub(crate) confirmed: Option<ConfirmedLocation>,
    pub(crate) transition: Option<LocalMovementTransition>,
    pub(crate) map_transition_pending: bool,
    pub(crate) route_mode: Option<WalkMode>,
    pub(crate) current_destination: Option<TilePosition>,
}

impl ExactRouteState {
    pub(crate) fn validate(self) -> Result<RouteReplacement, ExactRouteInvalidState> {
        if self.map_transition_pending {
            return Err(self.invalid(ExactRouteInvalidStateReason::MapTransitionPending));
        }
        let Some(native_map_id) = self.native_map_id else {
            return Err(self.invalid(ExactRouteInvalidStateReason::NativeMapUnavailable));
        };
        if native_map_id != self.route_map_id {
            return Err(self.invalid(ExactRouteInvalidStateReason::NativeMapMismatch));
        }
        let Some(transition) = self.transition else {
            return Err(self.invalid(ExactRouteInvalidStateReason::NativeTransitionUnavailable));
        };
        let replacement = transition.route_replacement();
        if let Some(confirmed) = self.confirmed {
            if confirmed.map_id != self.route_map_id {
                return Err(self.invalid(ExactRouteInvalidStateReason::ConfirmedMapMismatch));
            }
            if confirmed.position != replacement.origin {
                return Err(self.invalid(ExactRouteInvalidStateReason::ConfirmedPositionMismatch));
            }
        }
        Ok(replacement)
    }

    pub(crate) const fn invalid(
        self,
        reason: ExactRouteInvalidStateReason,
    ) -> ExactRouteInvalidState {
        ExactRouteInvalidState {
            reason,
            route_map_id: self.route_map_id,
            packet_map_id: match self.confirmed {
                Some(confirmed) => Some(confirmed.map_id),
                None => None,
            },
            native_map_id: self.native_map_id,
            packet_position: match self.confirmed {
                Some(confirmed) => Some(confirmed.position),
                None => None,
            },
            native_position: match self.transition {
                Some(transition) => Some(transition.committed_position()),
                None => None,
            },
            staged_position: match self.transition {
                Some(transition) => Some(transition.staged_position()),
                None => None,
            },
            transition_active: match self.transition {
                Some(transition) => Some(transition.is_active()),
                None => None,
            },
            route_mode: self.route_mode,
            current_destination: self.current_destination,
        }
    }
}

pub(crate) fn transactional_replace<T, E>(
    install: impl FnOnce() -> Result<T, E>,
    commit: impl FnOnce(&T),
) -> Result<T, E> {
    let installed = install()?;
    commit(&installed);
    Ok(installed)
}

pub(crate) fn commit_route_replacement(
    stop_previous: impl FnOnce(),
    install_tracking: impl FnOnce(),
    publish_route: impl FnOnce(),
) {
    stop_previous();
    install_tracking();
    publish_route();
}

pub(crate) const fn position_correction_clears_route(route_mode: Option<WalkMode>) -> bool {
    matches!(route_mode, Some(WalkMode::ExactRoute))
}

#[cfg(test)]
mod tests {
    use super::{
        ConfirmedLocation, ExactRouteInvalidStateReason, ExactRouteState, LocalMovementTransition,
        RouteActivation, commit_route_replacement, position_correction_clears_route,
        transactional_replace,
    };
    use darpc_model::{TilePosition, WalkMode};

    const COMMITTED: TilePosition = TilePosition { x: 10, y: 20 };
    const STAGED: TilePosition = TilePosition { x: 11, y: 20 };

    #[test]
    fn walk_to_replacement_while_idle_advances_from_committed_position() {
        let replacement =
            LocalMovementTransition::new(COMMITTED, STAGED, false).route_replacement();

        assert_eq!(replacement.origin, COMMITTED);
        assert_eq!(replacement.activation, RouteActivation::AdvanceNow);
    }

    #[test]
    fn walk_to_replacement_mid_step_waits_at_staged_position() {
        let replacement = LocalMovementTransition::new(COMMITTED, STAGED, true).route_replacement();

        assert_eq!(replacement.origin, STAGED);
        assert_eq!(replacement.activation, RouteActivation::AwaitStepCompletion);
    }

    #[test]
    fn repeated_mid_step_replacements_remain_deferred() {
        let transition = LocalMovementTransition::new(COMMITTED, STAGED, true);

        for _ in 0..32 {
            let replacement = transition.route_replacement();
            assert_eq!(replacement.origin, STAGED);
            assert_eq!(replacement.activation, RouteActivation::AwaitStepCompletion);
        }
    }

    #[test]
    fn direct_walk_mid_step_is_rejected() {
        assert!(!LocalMovementTransition::new(COMMITTED, STAGED, true).direct_walk_allowed());
    }

    #[test]
    fn direct_walk_while_idle_remains_allowed() {
        assert!(LocalMovementTransition::new(COMMITTED, STAGED, false).direct_walk_allowed());
    }

    #[test]
    fn committed_position_remains_available_for_exact_route_validation() {
        let transition = LocalMovementTransition::new(COMMITTED, STAGED, true);

        assert_eq!(transition.committed_position(), COMMITTED);
    }

    #[test]
    fn active_route_origin_uses_staged_destination_not_committed_tile() {
        let replacement = LocalMovementTransition::new(COMMITTED, STAGED, true).route_replacement();

        assert_ne!(replacement.origin, COMMITTED);
        assert_eq!(replacement.origin, STAGED);
    }

    #[test]
    fn active_replacement_does_not_pop_or_advance_the_queued_route() {
        let replacement = LocalMovementTransition::new(COMMITTED, STAGED, true).route_replacement();

        assert_eq!(replacement.activation, RouteActivation::AwaitStepCompletion);
    }

    fn exact_route_state(transition: LocalMovementTransition) -> ExactRouteState {
        ExactRouteState {
            route_map_id: 500,
            native_map_id: Some(500),
            confirmed: Some(ConfirmedLocation {
                map_id: 500,
                position: transition.route_replacement().origin,
            }),
            transition: Some(transition),
            map_transition_pending: false,
            route_mode: Some(WalkMode::ExactRoute),
            current_destination: Some(TilePosition { x: 30, y: 20 }),
        }
    }

    #[test]
    fn exact_route_while_idle_uses_committed_origin() {
        let state = exact_route_state(LocalMovementTransition::new(COMMITTED, STAGED, false));

        assert_eq!(state.validate().unwrap().origin, COMMITTED);
    }

    #[test]
    fn exact_route_keeps_c15e3f9_fail_open_when_packet_position_is_unavailable() {
        let state = ExactRouteState {
            confirmed: None,
            ..exact_route_state(LocalMovementTransition::new(COMMITTED, STAGED, false))
        };

        assert_eq!(state.validate().unwrap().origin, COMMITTED);
    }

    #[test]
    fn exact_route_mid_step_uses_staged_origin_and_defers_activation() {
        let state = exact_route_state(LocalMovementTransition::new(COMMITTED, STAGED, true));

        let replacement = state.validate().unwrap();
        assert_eq!(replacement.origin, STAGED);
        assert_eq!(replacement.activation, RouteActivation::AwaitStepCompletion);
    }

    #[test]
    fn exact_route_rejects_unconfirmed_staged_step_with_complete_diagnostics() {
        let transition = LocalMovementTransition::new(COMMITTED, STAGED, true);
        let state = ExactRouteState {
            confirmed: Some(ConfirmedLocation {
                map_id: 500,
                position: COMMITTED,
            }),
            ..exact_route_state(transition)
        };

        let diagnostic = state.validate().unwrap_err();
        assert_eq!(
            diagnostic.reason,
            ExactRouteInvalidStateReason::ConfirmedPositionMismatch
        );
        assert_eq!(diagnostic.route_map_id, 500);
        assert_eq!(diagnostic.packet_map_id, Some(500));
        assert_eq!(diagnostic.native_map_id, Some(500));
        assert_eq!(diagnostic.packet_position, Some(COMMITTED));
        assert_eq!(diagnostic.native_position, Some(COMMITTED));
        assert_eq!(diagnostic.staged_position, Some(STAGED));
        assert_eq!(diagnostic.transition_active, Some(true));
        assert_eq!(diagnostic.route_mode, Some(WalkMode::ExactRoute));
        assert_eq!(
            diagnostic.current_destination,
            Some(TilePosition { x: 30, y: 20 })
        );
    }

    #[test]
    fn unresolved_local_object_rejects_without_inventing_native_state() {
        let state = ExactRouteState {
            transition: None,
            ..exact_route_state(LocalMovementTransition::new(COMMITTED, STAGED, false))
        };

        let diagnostic = state.validate().unwrap_err();
        assert_eq!(
            diagnostic.reason,
            ExactRouteInvalidStateReason::NativeTransitionUnavailable
        );
        assert_eq!(diagnostic.packet_position, Some(COMMITTED));
        assert_eq!(diagnostic.native_position, None);
        assert_eq!(diagnostic.transition_active, None);
    }

    #[test]
    fn rejected_replacement_preserves_the_current_route_and_tracking() {
        let mut destination = Some(TilePosition { x: 30, y: 20 });
        let mut stopped = 0;

        let result = transactional_replace(
            || Err::<TilePosition, _>("invalid_state"),
            |replacement| {
                stopped += 1;
                destination = Some(*replacement);
            },
        );

        assert_eq!(result, Err("invalid_state"));
        assert_eq!(destination, Some(TilePosition { x: 30, y: 20 }));
        assert_eq!(stopped, 0);
    }

    #[test]
    fn spammy_rejected_replacements_never_commit_tracking_changes() {
        let destination = TilePosition { x: 30, y: 20 };
        let mut current = destination;
        let mut commits = 0;

        for _ in 0..32 {
            let _ = transactional_replace(
                || Err::<TilePosition, _>("invalid_state"),
                |replacement| {
                    current = *replacement;
                    commits += 1;
                },
            );
        }

        assert_eq!(current, destination);
        assert_eq!(commits, 0);
    }

    #[test]
    fn obstructed_replacement_preserves_the_route_that_is_still_executing() {
        let old_destination = TilePosition { x: 30, y: 20 };
        let mut current_destination = old_destination;
        let mut lifecycle_events = Vec::new();

        let result = transactional_replace(
            || Err::<TilePosition, _>("obstructed"),
            |replacement| {
                lifecycle_events.push("walking.stopped:replaced");
                current_destination = *replacement;
                lifecycle_events.push("walking.route_changed");
            },
        );

        assert_eq!(result, Err("obstructed"));
        assert_eq!(current_destination, old_destination);
        assert!(lifecycle_events.is_empty());
    }

    #[test]
    fn successful_replacement_commits_once_after_installation() {
        let replacement = TilePosition { x: 40, y: 20 };
        let mut current = TilePosition { x: 30, y: 20 };
        let mut commits = 0;

        let installed = transactional_replace(
            || Ok::<_, &str>(replacement),
            |installed| {
                current = *installed;
                commits += 1;
            },
        )
        .unwrap();

        assert_eq!(installed, replacement);
        assert_eq!(current, replacement);
        assert_eq!(commits, 1);
    }

    #[test]
    fn successful_replacement_orders_stopped_before_route_changed() {
        let lifecycle_events = std::cell::RefCell::new(Vec::new());

        commit_route_replacement(
            || {
                lifecycle_events
                    .borrow_mut()
                    .push("walking.stopped:replaced")
            },
            || lifecycle_events.borrow_mut().push("tracking.installed"),
            || lifecycle_events.borrow_mut().push("walking.route_changed"),
        );

        assert_eq!(
            *lifecycle_events.borrow(),
            [
                "walking.stopped:replaced",
                "tracking.installed",
                "walking.route_changed"
            ]
        );
    }

    #[test]
    fn server_position_correction_discards_only_external_exact_routes() {
        assert!(position_correction_clears_route(Some(WalkMode::ExactRoute)));
        assert!(!position_correction_clears_route(Some(
            WalkMode::NativeRoute
        )));
        assert!(!position_correction_clears_route(Some(WalkMode::Direct)));
        assert!(!position_correction_clears_route(None));
    }
}
