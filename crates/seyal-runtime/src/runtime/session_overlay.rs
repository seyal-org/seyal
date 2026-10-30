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
    BeginDelivering,
    Trust,
    Fail,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SessionReject;

impl SessionOverlay {
    pub(crate) fn holds_direct_input(self) -> bool {
        matches!(self, Self::Armed { .. } | Self::Delivering { .. })
    }

    pub(crate) fn host_block(self) -> Option<u64> {
        match self {
            Self::None => None,
            Self::Candidate { host_block }
            | Self::Offered { host_block, .. }
            | Self::Declined { host_block }
            | Self::Armed { host_block, .. }
            | Self::Delivering { host_block, .. }
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
            (S::Delivering { host_block, .. }, E::Trust) => Ok(S::Trusted { host_block }),
            (
                S::Candidate { host_block }
                | S::Offered { host_block, .. }
                | S::Armed { host_block, .. }
                | S::Delivering { host_block, .. },
                E::Fail,
            ) => Ok(S::Failed { host_block }),
            (
                S::Candidate { host_block }
                | S::Offered { host_block, .. }
                | S::Declined { host_block }
                | S::Armed { host_block, .. }
                | S::Delivering { host_block, .. }
                | S::Trusted { host_block }
                | S::Failed { host_block },
                E::End,
            ) => Ok(S::Ended { host_block }),
            // Depth one: a trusted session is not a candidate and cannot arm.
            (S::Trusted { .. }, E::MarkCandidate { .. } | E::Accept | E::Offer { .. }) => {
                Err(SessionReject)
            }
            _ => Err(SessionReject),
        }
    }
}

/// Local integration fact used to derive the input target. Session-scope
/// `Trusted(s)` integration is Slice B; until then the local scope is innermost.
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

pub(crate) fn input_target_scope(_target: InputTargetFact) -> BlockScope {
    // Session scope is Slice B (Trusted session shell). Local scope is innermost.
    BlockScope::Local
}

/// ADR-026 §3.2, MVP relay rule: a known Running Block in Flow on the primary
/// screen for the Controller is Relay, not Busy. Armed/Delivering holds every
/// direct route, including relay.
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
    match facts.integration {
        ShellFact::AtPrompt => DerivedComposer::Submit,
        ShellFact::Running {
            block: Some(block_id),
        } => DerivedComposer::Relay { block_id },
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
            (
                S::Candidate { host_block: 4 },
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
    }

    #[test]
    fn running_known_block_is_relay_not_unavailable() {
        let derived = derive_composer(ComposerFacts {
            integration: ShellFact::Running { block: Some(11) },
            primary_screen: true,
            flow: true,
            controller: true,
            overlay: SessionOverlay::Declined { host_block: 11 },
        });
        assert_eq!(derived, DerivedComposer::Relay { block_id: 11 });
        assert_eq!(derived.mode(), ComposerTargetMode::Relay);
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
            }),
            DerivedComposer::Unavailable(ComposerTargetReason::NoKnownBlock)
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
