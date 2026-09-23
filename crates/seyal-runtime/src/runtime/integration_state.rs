//! Per-execution shell-integration state machine (ADR-009, 2026-09-16
//! amendment, mechanism 5). Pure: it consumes already-trusted markers and
//! Runtime admission events and returns the state plus bounded effects; it
//! never touches the PTY, the parser, or Block storage.

use seyal_exec::LineId;

use crate::command_block_timeline::CommandBlockId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IntegrationState {
    /// Spawned; no trusted prompt-start observed yet.
    Unproven,
    /// A trusted `A` observed, and neither a `C` nor any direct input admitted since.
    AtPrompt,
    /// Composer bytes written; no trusted `C` since.
    Pending,
    /// A `C` observed or direct input admitted; next prompt not yet announced.
    Running { block: Option<CommandBlockId> },
    /// Primary child exited or PTY reached EOF.
    Terminated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IntegrationEvent {
    /// Trusted `A;<nonce>`.
    PromptStarted,
    /// Trusted `C;<nonce>`. `line` is the cursor's logical line stamped by
    /// the parser when it recognized the marker (never re-sampled later —
    /// see `ShellIntegrationEvent::CommandStarted`).
    CommandStarted { line: LineId },
    /// Trusted `D;<nonce>;<status>`. `line` is likewise parser-stamped.
    CommandFinished { exit_status: i32, line: LineId },
    /// Runtime admitted bytes that did not come from the composer.
    DirectInputAdmitted,
    /// Canonical state entered the alternate screen.
    AlternateScreenEntered,
    /// Primary child exited or the PTY reached EOF.
    ExecutionEnded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BlockExit {
    Code(i32),
    /// The finishing marker was never observed; never reported as `0`.
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Effect {
    /// Start the Block for the pending composer submission, at the given
    /// parser-stamped line.
    StartPendingBlock { line: LineId },
    /// Forget the pending composer submission; no Block is created.
    DropPending,
    /// Complete the given Block. `line` is the parser-stamped line from a
    /// trusted `D` marker when one triggered this completion; `None` for a
    /// recovery/fallback completion (lost `D`, or execution ended) that has
    /// no trusted marker to stamp a line from — the caller then falls back
    /// to sampling the current cursor itself.
    Complete(CommandBlockId, BlockExit, Option<LineId>),
}

/// Result of one transition: the new state and at most two ordered effects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Transition {
    pub(crate) next: IntegrationState,
    pub(crate) effects: [Option<Effect>; 2],
}

impl Transition {
    const fn to(next: IntegrationState) -> Self {
        Self {
            next,
            effects: [None, None],
        }
    }

    const fn with(next: IntegrationState, effect: Effect) -> Self {
        Self {
            next,
            effects: [Some(effect), None],
        }
    }
}

impl IntegrationState {
    /// Composer eligibility as far as integration state is concerned. The
    /// caller additionally requires the primary screen and no unwritten input.
    pub(crate) fn composer_eligible(self) -> bool {
        matches!(self, Self::AtPrompt)
    }

    /// The transition the composer takes when its bytes are admitted. Callers
    /// must have checked `composer_eligible` first.
    pub(crate) fn submitted(self) -> Self {
        debug_assert!(self.composer_eligible());
        Self::Pending
    }

    pub(crate) fn on(self, event: IntegrationEvent) -> Transition {
        use IntegrationEvent as E;
        use IntegrationState as S;
        match (self, event) {
            (S::Terminated, _) => Transition::to(S::Terminated),

            (S::Running { block: Some(b) }, E::ExecutionEnded) => {
                Transition::with(S::Terminated, Effect::Complete(b, BlockExit::Unknown, None))
            }
            (S::Pending, E::ExecutionEnded) => Transition::with(S::Terminated, Effect::DropPending),
            (_, E::ExecutionEnded) => Transition::to(S::Terminated),

            (S::Unproven, E::PromptStarted) => Transition::to(S::AtPrompt),
            (S::Unproven, _) => Transition::to(S::Unproven),

            (S::AtPrompt, E::PromptStarted) => Transition::to(S::AtPrompt),
            (S::AtPrompt, E::CommandFinished { .. }) => Transition::to(S::AtPrompt),
            (
                S::AtPrompt,
                E::CommandStarted { .. } | E::DirectInputAdmitted | E::AlternateScreenEntered,
            ) => Transition::to(S::Running { block: None }),

            (S::Pending, E::CommandStarted { line }) => {
                // The pending Block id is supplied by the caller when it
                // applies `StartPendingBlock`; state carries the running Block
                // once the caller reports it via `block_started`.
                Transition::with(
                    S::Running { block: None },
                    Effect::StartPendingBlock { line },
                )
            }
            (S::Pending, E::PromptStarted) => Transition::with(S::AtPrompt, Effect::DropPending),
            (S::Pending, E::AlternateScreenEntered) => {
                Transition::with(S::Running { block: None }, Effect::DropPending)
            }
            (S::Pending, E::CommandFinished { .. } | E::DirectInputAdmitted) => {
                Transition::to(S::Pending)
            }

            (S::Running { block: Some(b) }, E::CommandFinished { exit_status, line }) => {
                Transition::with(
                    S::Running { block: None },
                    Effect::Complete(b, BlockExit::Code(exit_status), Some(line)),
                )
            }
            (S::Running { block: Some(b) }, E::PromptStarted) => {
                Transition::with(S::AtPrompt, Effect::Complete(b, BlockExit::Unknown, None))
            }
            (S::Running { block: Some(b) }, E::CommandStarted { .. }) => Transition::with(
                S::Running { block: None },
                Effect::Complete(b, BlockExit::Unknown, None),
            ),
            (S::Running { block: Some(_) }, E::DirectInputAdmitted | E::AlternateScreenEntered) => {
                Transition::to(self)
            }

            (S::Running { block: None }, E::PromptStarted) => Transition::to(S::AtPrompt),
            (S::Running { block: None }, _) => Transition::to(S::Running { block: None }),
        }
    }

    /// Record the Block that `StartPendingBlock` produced.
    pub(crate) fn block_started(self, block: CommandBlockId) -> Self {
        match self {
            Self::Running { block: None } => Self::Running { block: Some(block) },
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(raw: u64) -> CommandBlockId {
        CommandBlockId::from_raw(raw)
    }

    fn line(raw: u64) -> LineId {
        LineId(raw)
    }

    use IntegrationEvent as E;
    use IntegrationState as S;

    #[test]
    fn unproven_only_prompt_start_promotes() {
        for event in [
            E::CommandStarted { line: line(1) },
            E::CommandFinished {
                exit_status: 0,
                line: line(1),
            },
            E::DirectInputAdmitted,
            E::AlternateScreenEntered,
        ] {
            assert_eq!(
                S::Unproven.on(event),
                Transition::to(S::Unproven),
                "{event:?}"
            );
        }
        assert_eq!(
            S::Unproven.on(E::PromptStarted),
            Transition::to(S::AtPrompt)
        );
        assert!(!S::Unproven.composer_eligible());
    }

    #[test]
    fn at_prompt_closes_on_admission_not_on_marker() {
        assert!(S::AtPrompt.composer_eligible());
        assert_eq!(
            S::AtPrompt.on(E::DirectInputAdmitted),
            Transition::to(S::Running { block: None })
        );
        assert_eq!(
            S::AtPrompt.on(E::CommandStarted { line: line(1) }),
            Transition::to(S::Running { block: None })
        );
        assert_eq!(
            S::AtPrompt.on(E::AlternateScreenEntered),
            Transition::to(S::Running { block: None })
        );
        assert_eq!(
            S::AtPrompt.on(E::PromptStarted),
            Transition::to(S::AtPrompt)
        );
        assert_eq!(
            S::AtPrompt.on(E::CommandFinished {
                exit_status: 3,
                line: line(1)
            }),
            Transition::to(S::AtPrompt)
        );
        assert_eq!(S::AtPrompt.submitted(), S::Pending);
    }

    #[test]
    fn pending_resolves_only_through_c_or_a() {
        assert_eq!(
            S::Pending.on(E::CommandStarted { line: line(5) }),
            Transition::with(
                S::Running { block: None },
                Effect::StartPendingBlock { line: line(5) }
            )
        );
        assert_eq!(
            S::Pending.on(E::PromptStarted),
            Transition::with(S::AtPrompt, Effect::DropPending)
        );
        assert_eq!(
            S::Pending.on(E::CommandFinished {
                exit_status: 0,
                line: line(1)
            }),
            Transition::to(S::Pending)
        );
        // The Raw escape for an unmatched quote: direct input keeps Pending.
        assert_eq!(
            S::Pending.on(E::DirectInputAdmitted),
            Transition::to(S::Pending)
        );
        assert_eq!(
            S::Pending.on(E::AlternateScreenEntered),
            Transition::with(S::Running { block: None }, Effect::DropPending)
        );
        assert!(!S::Pending.composer_eligible());
        assert_eq!(
            S::Running { block: None }.block_started(block(7)),
            S::Running {
                block: Some(block(7))
            }
        );
    }

    #[test]
    fn running_block_completes_with_status_or_unknown_never_zero() {
        let running = S::Running {
            block: Some(block(9)),
        };
        assert_eq!(
            running.on(E::CommandFinished {
                exit_status: 17,
                line: line(12)
            }),
            Transition::with(
                S::Running { block: None },
                Effect::Complete(block(9), BlockExit::Code(17), Some(line(12)))
            )
        );
        // Lost D: the next prompt completes with an unknown status and no
        // trusted line (no D marker was observed to stamp one from).
        assert_eq!(
            running.on(E::PromptStarted),
            Transition::with(S::AtPrompt, Effect::Complete(block(9), BlockExit::Unknown, None))
        );
        // Lost D and A: the next command completes the previous Block first,
        // again with no trusted line for that completion.
        assert_eq!(
            running.on(E::CommandStarted { line: line(20) }),
            Transition::with(
                S::Running { block: None },
                Effect::Complete(block(9), BlockExit::Unknown, None)
            )
        );
        assert_eq!(running.on(E::DirectInputAdmitted), Transition::to(running));
        assert_eq!(
            running.on(E::AlternateScreenEntered),
            Transition::to(running)
        );
        assert!(!running.composer_eligible());
    }

    #[test]
    fn running_without_block_waits_for_prompt() {
        let idle = S::Running { block: None };
        assert_eq!(idle.on(E::PromptStarted), Transition::to(S::AtPrompt));
        for event in [
            E::CommandStarted { line: line(1) },
            E::CommandFinished {
                exit_status: 0,
                line: line(1),
            },
            E::DirectInputAdmitted,
            E::AlternateScreenEntered,
        ] {
            assert_eq!(idle.on(event), Transition::to(idle), "{event:?}");
        }
    }

    #[test]
    fn execution_end_terminates_from_every_state_with_cleanup() {
        assert_eq!(
            S::Running {
                block: Some(block(2))
            }
            .on(E::ExecutionEnded),
            Transition::with(
                S::Terminated,
                Effect::Complete(block(2), BlockExit::Unknown, None)
            )
        );
        assert_eq!(
            S::Pending.on(E::ExecutionEnded),
            Transition::with(S::Terminated, Effect::DropPending)
        );
        for state in [S::Unproven, S::AtPrompt, S::Running { block: None }] {
            assert_eq!(state.on(E::ExecutionEnded), Transition::to(S::Terminated));
        }
        for event in [
            E::PromptStarted,
            E::CommandStarted { line: line(1) },
            E::CommandFinished {
                exit_status: 0,
                line: line(1),
            },
            E::DirectInputAdmitted,
            E::AlternateScreenEntered,
            E::ExecutionEnded,
        ] {
            assert_eq!(S::Terminated.on(event), Transition::to(S::Terminated));
        }
        assert!(!S::Terminated.composer_eligible());
    }

    #[test]
    fn python_stdin_race_is_closed_at_admission_time() {
        // Flow at prompt; user switches to Raw and types `python\r`.
        let after_raw = S::AtPrompt.on(E::DirectInputAdmitted).next;
        // Back in Flow before `C` is parsed: the composer must not be eligible.
        assert!(!after_raw.composer_eligible());
        // `C` for python arrives later and changes nothing.
        assert_eq!(
            after_raw.on(E::CommandStarted { line: line(1) }).next,
            S::Running { block: None }
        );
    }
}
