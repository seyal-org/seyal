//! ADR-026 session overlay (F4) and derived composer input target.
//!
//! The overlay is Runtime product state. It does not own a PTY, VT, or grid.
//! F9 argv sampling is not performed here; Slice B owns the echo-off stager.

use crate::local_ipc::framing::{
    BlockScope, ComposerTargetMode, ComposerTargetReason, InputTargetClass,
};

/// F4 session overlay for one host Block. Depth one: `Trusted` cannot arm.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum SessionOverlay {
    #[default]
    None,
    Candidate {
        host_block: u64,
    },
    Offered {
        host_block: u64,
        offer_generation: u64,
    },
    Declined {
        host_block: u64,
    },
    Armed {
        host_block: u64,
        offer_generation: u64,
    },
    Delivering {
        host_block: u64,
        offer_generation: u64,
    },
    /// Spawn-time delivery (ADR-026 2026-10-31 amendment, Decision item 4).
    /// S and K are armed for the host Block's own launch command. Direct
    /// input is not held: the client may still be authenticating.
    SpawnArmed {
        host_block: u64,
    },
    Trusted {
        host_block: u64,
    },
    Failed {
        host_block: u64,
    },
    Ended {
        host_block: u64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum SessionEvent {
    /// §4.1 candidate. Forbidden from `Trusted`, `Armed`, `Delivering`,
    /// `Offered`, `Candidate`, and `Ended`.
    MarkCandidate {
        host_block: u64,
    },
    Offer {
        offer_generation: u64,
    },
    Decline,
    Accept,
    /// Arm spawn-time delivery for the host Block a composer `ssh` will start.
    SpawnArm {
        host_block: u64,
    },
    BeginDelivering,
    Trust,
    Fail,
    /// Trusted-secret session ended. Candidate and Offered do not use this.
    End,
    /// Drop a non-trusted overlay back to `None` (host completed, or a
    /// different host Block replaced Declined/Failed/Ended).
    Clear,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SessionReject;

impl SessionOverlay {
    pub(crate) fn holds_direct_input(self) -> bool {
        matches!(self, Self::Armed { .. } | Self::Delivering { .. })
    }

    /// A new host Block may arm spawn-time delivery. Same depth-one gate as
    /// §4.1(c); `Ended` belongs to an earlier host.
    pub(crate) fn can_spawn_arm(self) -> bool {
        matches!(
            self,
            Self::None | Self::Declined { .. } | Self::Failed { .. } | Self::Ended { .. }
        )
    }

    pub(crate) fn host_block(self) -> Option<u64> {
        match self {
            Self::None => None,
            Self::Candidate { host_block }
            | Self::Offered { host_block, .. }
            | Self::Declined { host_block }
            | Self::Armed { host_block, .. }
            | Self::Delivering { host_block, .. }
            | Self::SpawnArmed { host_block }
            | Self::Trusted { host_block }
            | Self::Failed { host_block }
            | Self::Ended { host_block } => Some(host_block),
        }
    }

    pub(crate) fn offer_generation(self) -> Option<u64> {
        match self {
            Self::Offered {
                offer_generation, ..
            }
            | Self::Armed {
                offer_generation, ..
            }
            | Self::Delivering {
                offer_generation, ..
            } => Some(offer_generation),
            _ => None,
        }
    }

    pub(crate) fn wire(self) -> crate::local_ipc::framing::SessionOverlayWire {
        use crate::local_ipc::framing::SessionOverlayWire as W;
        match self {
            Self::None => W::None,
            Self::Candidate { .. } => W::Candidate,
            Self::Offered { .. } => W::Offered,
            Self::Declined { .. } => W::Declined,
            Self::Armed { .. } => W::Armed,
            Self::Delivering { .. } => W::Delivering,
            // No client affordance: no banner, and input is not held.
            Self::SpawnArmed { .. } => W::None,
            Self::Trusted { .. } => W::Trusted,
            Self::Failed { .. } => W::Failed,
            Self::Ended { .. } => W::Ended,
        }
    }

    pub(crate) fn on(self, event: SessionEvent) -> Result<Self, SessionReject> {
        use SessionEvent as E;
        use SessionOverlay as S;
        match (self, event) {
            (S::None | S::Declined { .. } | S::Failed { .. }, E::MarkCandidate { host_block })
                if host_block != 0 =>
            {
                Ok(S::Candidate { host_block })
            }
            (S::Candidate { host_block }, E::Offer { offer_generation })
                if offer_generation != 0 =>
            {
                Ok(S::Offered {
                    host_block,
                    offer_generation,
                })
            }
            (S::Offered { host_block, .. }, E::Decline) => Ok(S::Declined { host_block }),
            (
                S::Offered {
                    host_block,
                    offer_generation,
                },
                E::Accept,
            ) => Ok(S::Armed {
                host_block,
                offer_generation,
            }),
            (
                S::Armed {
                    host_block,
                    offer_generation,
                },
                E::BeginDelivering,
            ) => Ok(S::Delivering {
                host_block,
                offer_generation,
            }),
            (from, E::SpawnArm { host_block }) if from.can_spawn_arm() && host_block != 0 => {
                Ok(S::SpawnArmed { host_block })
            }
            (S::SpawnArmed { host_block }, E::BeginDelivering) => Ok(S::Delivering {
                host_block,
                offer_generation: 0,
            }),
            (S::Delivering { host_block, .. }, E::Trust) => Ok(S::Trusted { host_block }),
            (
                S::Candidate { host_block }
                | S::Offered { host_block, .. }
                | S::Armed { host_block, .. }
                | S::Delivering { host_block, .. }
                | S::SpawnArmed { host_block },
                E::Fail,
            ) => Ok(S::Failed { host_block }),
            (S::Trusted { host_block }, E::End) => Ok(S::Ended { host_block }),
            (
                S::Candidate { .. }
                | S::Offered { .. }
                | S::Declined { .. }
                | S::SpawnArmed { .. }
                | S::Failed { .. }
                | S::Ended { .. },
                E::Clear,
            ) => Ok(S::None),
            // Depth one: a trusted session is not a candidate and cannot arm.
            (S::Trusted { .. }, E::MarkCandidate { .. } | E::Accept | E::Offer { .. }) => {
                Err(SessionReject)
            }
            _ => Err(SessionReject),
        }
    }
}

/// Mechanism-5 fact for one scope. While the overlay is `Trusted`, composer
/// derivation uses the session fact; otherwise the local fact is innermost.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShellFact {
    Unproven,
    AtPrompt,
    Pending,
    Running { block: Option<u64> },
    Terminated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InputTargetFact {
    None,
    TrustedShell,
    Foreground(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ComposerFacts {
    pub(crate) integration: ShellFact,
    pub(crate) primary_screen: bool,
    pub(crate) flow: bool,
    pub(crate) controller: bool,
    pub(crate) overlay: SessionOverlay,
    /// Mechanism-5 state of the session scope. Used only while `overlay` is `Trusted`.
    pub(crate) session_integration: ShellFact,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DerivedComposer {
    Submit,
    Relay { block_id: u64 },
    Unavailable(ComposerTargetReason),
}

impl DerivedComposer {
    pub(crate) fn mode(self) -> ComposerTargetMode {
        match self {
            Self::Submit => ComposerTargetMode::Submit,
            Self::Relay { .. } => ComposerTargetMode::Relay,
            Self::Unavailable(_) => ComposerTargetMode::Unavailable,
        }
    }

    pub(crate) fn reason(self) -> ComposerTargetReason {
        match self {
            Self::Unavailable(reason) => reason,
            Self::Submit | Self::Relay { .. } => ComposerTargetReason::None,
        }
    }

    pub(crate) fn block_id(self) -> u64 {
        match self {
            Self::Relay { block_id } => block_id,
            Self::Submit | Self::Unavailable(_) => 0,
        }
    }
}

pub(crate) fn input_target(integration: ShellFact) -> InputTargetFact {
    match integration {
        ShellFact::AtPrompt => InputTargetFact::TrustedShell,
        ShellFact::Running { block: Some(block) } => InputTargetFact::Foreground(block),
        ShellFact::Unproven
        | ShellFact::Pending
        | ShellFact::Running { block: None }
        | ShellFact::Terminated => InputTargetFact::None,
    }
}

pub(crate) fn input_target_class(target: InputTargetFact) -> InputTargetClass {
    match target {
        InputTargetFact::None => InputTargetClass::None,
        InputTargetFact::TrustedShell => InputTargetClass::TrustedShell,
        InputTargetFact::Foreground(_) => InputTargetClass::ForegroundProgram,
    }
}

pub(crate) fn input_target_scope(overlay: SessionOverlay) -> BlockScope {
    if matches!(overlay, SessionOverlay::Trusted { .. }) {
        BlockScope::Session
    } else {
        BlockScope::Local
    }
}

/// ADR-026 §3.2. Without F9, local-scope Relay opens only after Decline or
/// Failed on that same Running host Block. A Trusted session may Relay to
/// its own Running Block. Armed/Delivering holds every direct route.
pub(crate) fn derive_composer(facts: ComposerFacts) -> DerivedComposer {
    if facts.overlay.holds_direct_input() {
        return DerivedComposer::Unavailable(ComposerTargetReason::BootstrapHold);
    }
    if !facts.controller {
        return DerivedComposer::Unavailable(ComposerTargetReason::NotController);
    }
    if !facts.flow {
        return DerivedComposer::Unavailable(ComposerTargetReason::NotFlow);
    }
    if !facts.primary_screen {
        return DerivedComposer::Unavailable(ComposerTargetReason::AlternateScreen);
    }
    let trusted = matches!(facts.overlay, SessionOverlay::Trusted { .. });
    let integration = if trusted {
        facts.session_integration
    } else {
        facts.integration
    };
    match integration {
        ShellFact::AtPrompt => DerivedComposer::Submit,
        ShellFact::Running {
            block: Some(block_id),
        } => {
            let relay_host = facts.overlay.host_block() == Some(block_id);
            if trusted
                || (relay_host
                    && matches!(
                        facts.overlay,
                        SessionOverlay::Declined { .. } | SessionOverlay::Failed { .. }
                    ))
            {
                DerivedComposer::Relay { block_id }
            } else {
                // F9 unavailable: Busy until Decline/Failed enables Relay (§8.4).
                DerivedComposer::Unavailable(ComposerTargetReason::NotAtPrompt)
            }
        }
        ShellFact::Running { block: None } => {
            DerivedComposer::Unavailable(ComposerTargetReason::NoKnownBlock)
        }
        ShellFact::Unproven | ShellFact::Pending => {
            DerivedComposer::Unavailable(ComposerTargetReason::NotAtPrompt)
        }
        ShellFact::Terminated => DerivedComposer::Unavailable(ComposerTargetReason::Terminated),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(block: u64) -> SessionOverlay {
        SessionOverlay::Offered {
            host_block: block,
            offer_generation: 1,
        }
    }

    #[test]
    fn overlay_transitions_match_the_depth_one_table() {
        use SessionEvent as E;
        use SessionOverlay as S;
        let cases: &[(S, E, Result<S, SessionReject>)] = &[
            (
                S::None,
                E::MarkCandidate { host_block: 4 },
                Ok(S::Candidate { host_block: 4 }),
            ),
            (
                S::Declined { host_block: 4 },
                E::MarkCandidate { host_block: 9 },
                Ok(S::Candidate { host_block: 9 }),
            ),
            (
                S::Failed { host_block: 4 },
                E::MarkCandidate { host_block: 4 },
                Ok(S::Candidate { host_block: 4 }),
            ),
            (
                S::Candidate { host_block: 4 },
                E::Offer {
                    offer_generation: 3,
                },
                Ok(S::Offered {
                    host_block: 4,
                    offer_generation: 3,
                }),
            ),
            (host(4), E::Decline, Ok(S::Declined { host_block: 4 })),
            (
                host(4),
                E::Accept,
                Ok(S::Armed {
                    host_block: 4,
                    offer_generation: 1,
                }),
            ),
            (
                S::Armed {
                    host_block: 4,
                    offer_generation: 1,
                },
                E::BeginDelivering,
                Ok(S::Delivering {
                    host_block: 4,
                    offer_generation: 1,
                }),
            ),
            (
                S::Delivering {
                    host_block: 4,
                    offer_generation: 1,
                },
                E::Trust,
                Ok(S::Trusted { host_block: 4 }),
            ),
            (
                S::Armed {
                    host_block: 4,
                    offer_generation: 1,
                },
                E::Fail,
                Ok(S::Failed { host_block: 4 }),
            ),
            (
                S::Trusted { host_block: 4 },
                E::End,
                Ok(S::Ended { host_block: 4 }),
            ),
            (
                S::Trusted { host_block: 4 },
                E::MarkCandidate { host_block: 8 },
                Err(SessionReject),
            ),
            (S::Trusted { host_block: 4 }, E::Accept, Err(SessionReject)),
            (
                S::Ended { host_block: 4 },
                E::MarkCandidate { host_block: 4 },
                Err(SessionReject),
            ),
            (
                S::Offered {
                    host_block: 4,
                    offer_generation: 1,
                },
                E::MarkCandidate { host_block: 4 },
                Err(SessionReject),
            ),
            (S::None, E::Accept, Err(SessionReject)),
            (S::Candidate { host_block: 4 }, E::End, Err(SessionReject)),
            (S::Candidate { host_block: 4 }, E::Clear, Ok(S::None)),
            (
                S::Offered {
                    host_block: 4,
                    offer_generation: 1,
                },
                E::Clear,
                Ok(S::None),
            ),
            (S::Declined { host_block: 4 }, E::Clear, Ok(S::None)),
            (S::Failed { host_block: 4 }, E::Clear, Ok(S::None)),
            (S::Ended { host_block: 4 }, E::Clear, Ok(S::None)),
            (S::Trusted { host_block: 4 }, E::Clear, Err(SessionReject)),
            (
                S::Candidate { host_block: 4 },
                E::Accept,
                Err(SessionReject),
            ),
            (
                S::None,
                E::SpawnArm { host_block: 6 },
                Ok(S::SpawnArmed { host_block: 6 }),
            ),
            (
                S::Ended { host_block: 4 },
                E::SpawnArm { host_block: 6 },
                Ok(S::SpawnArmed { host_block: 6 }),
            ),
            (
                S::Failed { host_block: 4 },
                E::SpawnArm { host_block: 6 },
                Ok(S::SpawnArmed { host_block: 6 }),
            ),
            (S::None, E::SpawnArm { host_block: 0 }, Err(SessionReject)),
            (
                S::Trusted { host_block: 4 },
                E::SpawnArm { host_block: 6 },
                Err(SessionReject),
            ),
            (
                S::Candidate { host_block: 4 },
                E::SpawnArm { host_block: 6 },
                Err(SessionReject),
            ),
            (
                S::SpawnArmed { host_block: 6 },
                E::BeginDelivering,
                Ok(S::Delivering {
                    host_block: 6,
                    offer_generation: 0,
                }),
            ),
            (
                S::SpawnArmed { host_block: 6 },
                E::Fail,
                Ok(S::Failed { host_block: 6 }),
            ),
            (S::SpawnArmed { host_block: 6 }, E::Clear, Ok(S::None)),
            (
                S::SpawnArmed { host_block: 6 },
                E::MarkCandidate { host_block: 6 },
                Err(SessionReject),
            ),
            (
                S::SpawnArmed { host_block: 6 },
                E::Trust,
                Err(SessionReject),
            ),
            (
                S::SpawnArmed { host_block: 6 },
                E::Accept,
                Err(SessionReject),
            ),
        ];
        for (from, event, expected) in cases {
            assert_eq!(from.on(*event), *expected, "{from:?} + {event:?}");
        }
    }

    #[test]
    fn armed_and_delivering_hold_direct_input() {
        assert!(SessionOverlay::Armed {
            host_block: 1,
            offer_generation: 1,
        }
        .holds_direct_input());
        assert!(SessionOverlay::Delivering {
            host_block: 1,
            offer_generation: 1,
        }
        .holds_direct_input());
        assert!(!SessionOverlay::Offered {
            host_block: 1,
            offer_generation: 1,
        }
        .holds_direct_input());
        assert!(!SessionOverlay::Declined { host_block: 1 }.holds_direct_input());
        assert!(!SessionOverlay::Trusted { host_block: 1 }.holds_direct_input());
        assert!(!SessionOverlay::SpawnArmed { host_block: 1 }.holds_direct_input());
    }

    #[test]
    fn spawn_armed_host_shows_no_banner_and_no_relay_before_trust() {
        use crate::local_ipc::framing::SessionOverlayWire;
        let overlay = SessionOverlay::SpawnArmed { host_block: 5 };
        assert_eq!(overlay.wire(), SessionOverlayWire::None);
        assert_eq!(overlay.offer_generation(), None);
        assert_eq!(
            derive_composer(ComposerFacts {
                integration: ShellFact::Running { block: Some(5) },
                primary_screen: true,
                flow: true,
                controller: true,
                overlay,
                session_integration: ShellFact::Unproven,
            }),
            DerivedComposer::Unavailable(ComposerTargetReason::NotAtPrompt)
        );
    }

    #[test]
    fn running_known_block_is_relay_not_unavailable() {
        let derived = derive_composer(ComposerFacts {
            integration: ShellFact::Running { block: Some(11) },
            primary_screen: true,
            flow: true,
            controller: true,
            overlay: SessionOverlay::Declined { host_block: 11 },
            session_integration: ShellFact::Unproven,
        });
        assert_eq!(derived, DerivedComposer::Relay { block_id: 11 });
        assert_eq!(derived.mode(), ComposerTargetMode::Relay);
        assert_eq!(
            derive_composer(ComposerFacts {
                integration: ShellFact::Running { block: Some(12) },
                primary_screen: true,
                flow: true,
                controller: true,
                overlay: SessionOverlay::Declined { host_block: 11 },
                session_integration: ShellFact::Unproven,
            }),
            DerivedComposer::Unavailable(ComposerTargetReason::NotAtPrompt),
            "Declined Relay stays on the declined host Block"
        );
        assert_eq!(
            derive_composer(ComposerFacts {
                integration: ShellFact::Running { block: Some(12) },
                primary_screen: true,
                flow: true,
                controller: true,
                overlay: SessionOverlay::Failed { host_block: 11 },
                session_integration: ShellFact::Unproven,
            }),
            DerivedComposer::Unavailable(ComposerTargetReason::NotAtPrompt)
        );
    }

    #[test]
    fn at_prompt_submits_and_hold_overrides_relay() {
        assert_eq!(
            derive_composer(ComposerFacts {
                integration: ShellFact::AtPrompt,
                primary_screen: true,
                flow: true,
                controller: true,
                overlay: SessionOverlay::None,
                session_integration: ShellFact::Unproven,
            }),
            DerivedComposer::Submit
        );
        assert_eq!(
            derive_composer(ComposerFacts {
                integration: ShellFact::Running { block: Some(3) },
                primary_screen: true,
                flow: true,
                controller: true,
                overlay: SessionOverlay::Armed {
                    host_block: 3,
                    offer_generation: 1,
                },
                session_integration: ShellFact::Unproven,
            }),
            DerivedComposer::Unavailable(ComposerTargetReason::BootstrapHold)
        );
    }

    #[test]
    fn running_without_a_block_does_not_relay() {
        assert_eq!(
            derive_composer(ComposerFacts {
                integration: ShellFact::Running { block: None },
                primary_screen: true,
                flow: true,
                controller: true,
                overlay: SessionOverlay::None,
                session_integration: ShellFact::Unproven,
            }),
            DerivedComposer::Unavailable(ComposerTargetReason::NoKnownBlock)
        );
    }

    #[test]
    fn trusted_session_at_prompt_submits_while_the_host_stays_running() {
        assert_eq!(
            derive_composer(ComposerFacts {
                integration: ShellFact::Running { block: Some(7) },
                primary_screen: true,
                flow: true,
                controller: true,
                overlay: SessionOverlay::Trusted { host_block: 7 },
                session_integration: ShellFact::AtPrompt,
            }),
            DerivedComposer::Submit
        );
        assert_eq!(
            derive_composer(ComposerFacts {
                integration: ShellFact::Running { block: Some(7) },
                primary_screen: true,
                flow: true,
                controller: true,
                overlay: SessionOverlay::Trusted { host_block: 7 },
                session_integration: ShellFact::Running { block: Some(9) },
            }),
            DerivedComposer::Relay { block_id: 9 }
        );
    }

    #[test]
    fn running_without_explicit_relay_enable_stays_unavailable() {
        assert_eq!(
            derive_composer(ComposerFacts {
                integration: ShellFact::Running { block: Some(5) },
                primary_screen: true,
                flow: true,
                controller: true,
                overlay: SessionOverlay::None,
                session_integration: ShellFact::Unproven,
            }),
            DerivedComposer::Unavailable(ComposerTargetReason::NotAtPrompt)
        );
        assert_eq!(
            derive_composer(ComposerFacts {
                integration: ShellFact::Running { block: Some(5) },
                primary_screen: true,
                flow: true,
                controller: true,
                overlay: SessionOverlay::Candidate { host_block: 5 },
                session_integration: ShellFact::Unproven,
            }),
            DerivedComposer::Unavailable(ComposerTargetReason::NotAtPrompt)
        );
    }

    #[test]
    fn input_target_changes_only_with_the_reader() {
        assert_eq!(
            input_target(ShellFact::AtPrompt),
            InputTargetFact::TrustedShell
        );
        assert_eq!(
            input_target(ShellFact::Running { block: Some(2) }),
            InputTargetFact::Foreground(2)
        );
        assert_eq!(
            input_target(ShellFact::Running { block: None }),
            InputTargetFact::None
        );
    }
}
