//! Portable Flow/Raw/TUI presentation for one TerminalExecution.
//!
//! This module owns mode, identity/epoch fencing, input-route policy, and
//! renderer presentation intent. It does not own PTY, VT, Metal, AppKit
//! first-responder, or hit-testing. Hosts dispatch [`PresentationAction`]
//! and render [`PresentationSnapshot`].

use seyal_core::ExecutionId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresentationMode {
    Flow,
    Raw,
    Tui,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PresentationIdentity {
    pub execution_id: ExecutionId,
    pub pty_generation: u64,
}

impl PresentationIdentity {
    pub fn new(execution_id: ExecutionId, pty_generation: u64) -> Option<Self> {
        if pty_generation == 0 {
            None
        } else {
            Some(Self {
                execution_id,
                pty_generation,
            })
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputRoute {
    Composer,
    DirectTerminal,
    Frozen,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RendererPlan {
    pub mode: PresentationMode,
    pub draws_full_grid_background: bool,
    pub draws_live_grid: bool,
    pub draws_cursor_outside_block_regions: bool,
}

impl RendererPlan {
    pub fn flow() -> Self {
        Self {
            mode: PresentationMode::Flow,
            draws_full_grid_background: false,
            draws_live_grid: false,
            draws_cursor_outside_block_regions: false,
        }
    }

    pub fn full_pane(mode: PresentationMode) -> Self {
        debug_assert_ne!(mode, PresentationMode::Flow);
        Self {
            mode,
            draws_full_grid_background: true,
            draws_live_grid: true,
            draws_cursor_outside_block_regions: true,
        }
    }

    pub fn for_mode(mode: PresentationMode) -> Self {
        match mode {
            PresentationMode::Flow => Self::flow(),
            PresentationMode::Raw | PresentationMode::Tui => Self::full_pane(mode),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlowPaintInspection {
    pub mode: PresentationMode,
    pub live_grid_submitted: bool,
    pub full_grid_background_submitted: bool,
    pub history_instance_count: u32,
    pub instances_outside_clips: u32,
    pub opaque_pixels_outside_clips: u32,
    pub opaque_pixels_inside_clips: u32,
}

impl FlowPaintInspection {
    pub fn is_clean(self) -> bool {
        if self.mode != PresentationMode::Flow {
            return true;
        }
        !self.live_grid_submitted
            && !self.full_grid_background_submitted
            && self.instances_outside_clips == 0
            && self.opaque_pixels_outside_clips == 0
    }

    pub fn accessibility_token(self) -> &'static str {
        if self.mode != PresentationMode::Flow {
            "n/a"
        } else if self.is_clean() {
            "ok"
        } else {
            "leak"
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresentationError {
    ZeroPtyGeneration,
    IdentityMismatch,
    StaleEpoch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresentationAction {
    BindIdentity(PresentationIdentity),
    /// Drop the bound identity after presentation detach (pane/tab close).
    /// Bumps the epoch so stale native transitions fail closed.
    ClearIdentity,
    Transition {
        mode: PresentationMode,
        identity: PresentationIdentity,
        explicit: bool,
        /// Must match the session epoch. Stale native callbacks fail closed.
        epoch: u64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PresentationSnapshot {
    pub mode: PresentationMode,
    pub identity: Option<PresentationIdentity>,
    pub epoch: u64,
    pub input_route: InputRoute,
    pub last_transition_was_explicit: bool,
    pub allows_direct_terminal_first_responder: bool,
    pub allows_empty_canvas_terminal_hit_test: bool,
    pub renderer_plan: RendererPlan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresentationSession {
    mode: PresentationMode,
    identity: Option<PresentationIdentity>,
    epoch: u64,
    input_route: InputRoute,
    last_transition_was_explicit: bool,
}

impl PresentationSession {
    pub fn new(identity: Option<PresentationIdentity>, mode: PresentationMode) -> Self {
        Self {
            identity,
            mode,
            epoch: 1,
            last_transition_was_explicit: false,
            input_route: Self::route(mode),
        }
    }

    pub fn apply(&mut self, action: PresentationAction) -> Result<(), PresentationError> {
        match action {
            PresentationAction::BindIdentity(identity) => self.bind_identity(identity),
            PresentationAction::ClearIdentity => {
                self.clear_identity();
                Ok(())
            }
            PresentationAction::Transition {
                mode,
                identity,
                explicit,
                epoch,
            } => self.transition(mode, identity, explicit, epoch),
        }
    }

    pub fn snapshot(&self) -> PresentationSnapshot {
        let allows_direct =
            self.mode != PresentationMode::Flow && self.input_route == InputRoute::DirectTerminal;
        PresentationSnapshot {
            mode: self.mode,
            identity: self.identity,
            epoch: self.epoch,
            input_route: self.input_route,
            last_transition_was_explicit: self.last_transition_was_explicit,
            allows_direct_terminal_first_responder: allows_direct,
            allows_empty_canvas_terminal_hit_test: allows_direct,
            renderer_plan: RendererPlan::for_mode(self.mode),
        }
    }

    fn bind_identity(&mut self, identity: PresentationIdentity) -> Result<(), PresentationError> {
        if identity.pty_generation == 0 {
            return Err(PresentationError::ZeroPtyGeneration);
        }
        match self.identity {
            None => {
                self.identity = Some(identity);
                Ok(())
            }
            Some(current) if current == identity => Ok(()),
            Some(_) => Err(PresentationError::IdentityMismatch),
        }
    }

    fn clear_identity(&mut self) {
        self.identity = None;
        self.mode = PresentationMode::Flow;
        self.input_route = Self::route(PresentationMode::Flow);
        self.last_transition_was_explicit = false;
        self.epoch = self.epoch.checked_add(1).unwrap_or(1);
    }

    fn transition(
        &mut self,
        next: PresentationMode,
        identity: PresentationIdentity,
        explicit: bool,
        epoch: u64,
    ) -> Result<(), PresentationError> {
        if identity.pty_generation == 0 {
            return Err(PresentationError::ZeroPtyGeneration);
        }
        if epoch != self.epoch {
            return Err(PresentationError::StaleEpoch);
        }
        match self.identity {
            None => self.identity = Some(identity),
            Some(current)
                if current.execution_id != identity.execution_id
                    || current.pty_generation != identity.pty_generation =>
            {
                return Err(PresentationError::IdentityMismatch);
            }
            Some(_) => {}
        }
        if next == self.mode {
            self.last_transition_was_explicit = explicit;
            return Ok(());
        }
        self.input_route = InputRoute::Frozen;
        self.epoch = self.epoch.wrapping_add(1);
        self.mode = next;
        self.last_transition_was_explicit = explicit;
        self.input_route = Self::route(next);
        Ok(())
    }

    fn route(mode: PresentationMode) -> InputRoute {
        match mode {
            PresentationMode::Flow => InputRoute::Composer,
            PresentationMode::Raw | PresentationMode::Tui => InputRoute::DirectTerminal,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(tag: u8, pty: u64) -> PresentationIdentity {
        PresentationIdentity::new(ExecutionId::from_bytes([tag; 16]), pty).expect("pty")
    }

    fn trans(
        session: &PresentationSession,
        mode: PresentationMode,
        identity: PresentationIdentity,
        explicit: bool,
    ) -> PresentationAction {
        PresentationAction::Transition {
            mode,
            identity,
            explicit,
            epoch: session.snapshot().epoch,
        }
    }

    #[test]
    fn flow_raw_tui_are_mutually_exclusive_modes() {
        let bound = identity(1, 7);
        let mut session = PresentationSession::new(Some(bound), PresentationMode::Flow);
        let snap = session.snapshot();
        assert_eq!(snap.mode, PresentationMode::Flow);
        assert_eq!(snap.input_route, InputRoute::Composer);
        assert!(!snap.allows_direct_terminal_first_responder);
        assert!(!snap.allows_empty_canvas_terminal_hit_test);

        assert!(session
            .apply(trans(&session, PresentationMode::Raw, bound, true,))
            .is_ok());
        assert_eq!(session.snapshot().mode, PresentationMode::Raw);
        assert_eq!(session.snapshot().input_route, InputRoute::DirectTerminal);

        assert!(session
            .apply(trans(&session, PresentationMode::Tui, bound, false,))
            .is_ok());
        assert_eq!(session.snapshot().mode, PresentationMode::Tui);
        assert_ne!(session.snapshot().mode, PresentationMode::Flow);
        assert_ne!(session.snapshot().mode, PresentationMode::Raw);
    }

    #[test]
    fn flow_to_tui_to_flow_keeps_execution_and_pty_identity() {
        let bound = identity(1, 7);
        let mut session = PresentationSession::new(Some(bound), PresentationMode::Flow);
        let start_epoch = session.snapshot().epoch;
        assert!(session
            .apply(trans(&session, PresentationMode::Tui, bound, false,))
            .is_ok());
        assert_eq!(session.snapshot().identity, Some(bound));
        assert!(session.snapshot().epoch > start_epoch);
        assert!(session
            .apply(trans(&session, PresentationMode::Flow, bound, false,))
            .is_ok());
        let snap = session.snapshot();
        assert_eq!(snap.mode, PresentationMode::Flow);
        assert_eq!(snap.identity, Some(bound));
        assert_eq!(snap.input_route, InputRoute::Composer);
    }

    #[test]
    fn flow_to_raw_records_explicit_named_transition() {
        let bound = identity(1, 7);
        let mut session = PresentationSession::new(Some(bound), PresentationMode::Flow);
        assert!(!session.snapshot().last_transition_was_explicit);
        assert!(session
            .apply(trans(&session, PresentationMode::Raw, bound, true,))
            .is_ok());
        assert!(session.snapshot().last_transition_was_explicit);
        assert!(session
            .apply(trans(&session, PresentationMode::Flow, bound, true,))
            .is_ok());
        assert!(session.snapshot().last_transition_was_explicit);
        assert_eq!(session.snapshot().identity, Some(bound));
    }

    #[test]
    fn transition_rejects_a_different_execution_or_pty_authority() {
        let bound = identity(1, 7);
        let mut session = PresentationSession::new(Some(bound), PresentationMode::Flow);
        assert_eq!(
            session.apply(trans(
                &session,
                PresentationMode::Tui,
                identity(2, 7),
                false,
            )),
            Err(PresentationError::IdentityMismatch)
        );
        assert_eq!(session.snapshot().mode, PresentationMode::Flow);
        assert_eq!(
            session.apply(trans(&session, PresentationMode::Raw, identity(1, 8), true,)),
            Err(PresentationError::IdentityMismatch)
        );
        assert_eq!(session.snapshot().mode, PresentationMode::Flow);
        assert_eq!(session.snapshot().identity, Some(bound));
    }

    #[test]
    fn flow_renderer_plan_never_exposes_full_grid_or_cursor_outside_blocks() {
        let plan = RendererPlan::flow();
        assert!(!plan.draws_full_grid_background);
        assert!(!plan.draws_live_grid);
        assert!(!plan.draws_cursor_outside_block_regions);
        let raw = RendererPlan::full_pane(PresentationMode::Raw);
        assert!(raw.draws_full_grid_background);
        assert!(raw.draws_live_grid);
        assert!(raw.draws_cursor_outside_block_regions);
    }

    #[test]
    fn transition_fences_input_route_before_installing_new_route() {
        let bound = identity(1, 7);
        let mut session = PresentationSession::new(Some(bound), PresentationMode::Flow);
        assert_eq!(session.snapshot().input_route, InputRoute::Composer);
        session
            .apply(trans(&session, PresentationMode::Raw, bound, true))
            .unwrap();
        assert_eq!(session.snapshot().input_route, InputRoute::DirectTerminal);
        assert_ne!(session.snapshot().input_route, InputRoute::Composer);
        assert_ne!(session.snapshot().input_route, InputRoute::Frozen);
    }

    #[test]
    fn bind_identity_accepts_first_bound_identity_without_changing_mode() {
        let mut session = PresentationSession::new(None, PresentationMode::Flow);
        let bound = identity(9, 3);
        session
            .apply(PresentationAction::BindIdentity(bound))
            .unwrap();
        let snap = session.snapshot();
        assert_eq!(snap.identity, Some(bound));
        assert_eq!(snap.mode, PresentationMode::Flow);
        assert_eq!(snap.epoch, 1);
    }

    #[test]
    fn bind_identity_does_not_replace_a_different_bound_identity() {
        let bound = identity(1, 7);
        let mut session = PresentationSession::new(Some(bound), PresentationMode::Raw);
        assert_eq!(
            session.apply(PresentationAction::BindIdentity(identity(2, 7))),
            Err(PresentationError::IdentityMismatch)
        );
        assert_eq!(session.snapshot().identity, Some(bound));
        assert_eq!(session.snapshot().mode, PresentationMode::Raw);
    }

    #[test]
    fn clear_identity_allows_a_later_distinct_bind() {
        let bound = identity(1, 3);
        let mut session = PresentationSession::new(Some(bound), PresentationMode::Raw);
        let before = session.snapshot().epoch;
        session
            .apply(PresentationAction::ClearIdentity)
            .expect("clear");
        let cleared = session.snapshot();
        assert_eq!(cleared.identity, None);
        assert_eq!(cleared.mode, PresentationMode::Flow);
        assert_eq!(cleared.epoch, before + 1);
        session
            .apply(PresentationAction::BindIdentity(identity(2, 7)))
            .expect("rebind");
        assert_eq!(session.snapshot().identity, Some(identity(2, 7)));
    }

    #[test]
    fn unbound_transition_binds_identity() {
        let mut session = PresentationSession::new(None, PresentationMode::Raw);
        let bound = identity(4, 1);
        session
            .apply(trans(&session, PresentationMode::Flow, bound, true))
            .unwrap();
        assert_eq!(session.snapshot().identity, Some(bound));
        assert_eq!(session.snapshot().mode, PresentationMode::Flow);
        assert_eq!(session.snapshot().input_route, InputRoute::Composer);
    }

    #[test]
    fn stale_epoch_fails_closed() {
        let bound = identity(1, 7);
        let mut session = PresentationSession::new(Some(bound), PresentationMode::Flow);
        let stale = session.snapshot().epoch;
        session
            .apply(trans(&session, PresentationMode::Raw, bound, true))
            .unwrap();
        assert_eq!(
            session.apply(PresentationAction::Transition {
                mode: PresentationMode::Tui,
                identity: bound,
                explicit: false,
                epoch: stale,
            }),
            Err(PresentationError::StaleEpoch)
        );
        assert_eq!(session.snapshot().mode, PresentationMode::Raw);
        assert_eq!(session.snapshot().identity, Some(bound));
    }

    #[test]
    fn zero_pty_generation_is_rejected() {
        assert!(PresentationIdentity::new(ExecutionId::from_bytes([1; 16]), 0).is_none());
    }

    #[test]
    fn flow_paint_inspection_is_clean_only_without_grid_leaks() {
        let clean = FlowPaintInspection {
            mode: PresentationMode::Flow,
            live_grid_submitted: false,
            full_grid_background_submitted: false,
            history_instance_count: 0,
            instances_outside_clips: 0,
            opaque_pixels_outside_clips: 0,
            opaque_pixels_inside_clips: 0,
        };
        assert!(clean.is_clean());
        assert_eq!(clean.accessibility_token(), "ok");
        let leak = FlowPaintInspection {
            live_grid_submitted: true,
            ..clean
        };
        assert!(!leak.is_clean());
        assert_eq!(leak.accessibility_token(), "leak");
        let raw = FlowPaintInspection {
            mode: PresentationMode::Raw,
            live_grid_submitted: true,
            ..clean
        };
        assert!(raw.is_clean());
        assert_eq!(raw.accessibility_token(), "n/a");
    }
}
