use darpc_model::TilePosition;

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

#[cfg(test)]
mod tests {
    use super::{LocalMovementTransition, RouteActivation};
    use darpc_model::TilePosition;

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
}
