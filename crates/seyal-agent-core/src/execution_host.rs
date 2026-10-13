//! Typed ExecutionHost seam (SPEC-018 §2, SPEC-027 §9).
//!
//! Concrete hosts and observation payload types live beside the Agent Backend
//! observation path. This crate owns only the domain discriminator, the
//! resolved launch descriptor value, and the lifecycle trait so adapters
//! cannot invent a second lifecycle authority.

use std::path::PathBuf;

use crate::{AgentRunId, BindingGeneration};

/// SPEC-018 §2 ExecutionHost family discriminator.
///
/// `SeyalTerminalExecutionHost` and `FutureRemoteExecutionHost` are accepted
/// family members but out of scope for this crate's composed implementors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionHostKind {
    Fake,
    StandaloneProcess,
}

/// Manifest-owned, WorkScope-resolved launch input (SPEC-027 §5.1).
///
/// Produced only by the Agent Backend from an installed+enabled adapter
/// manifest at dispatch. Never constructed from client-supplied spawn
/// fields (SPEC-027 §4.1 / §5.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchDescriptor {
    pub program: String,
    pub argv: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: PathBuf,
}

/// Typed child-exit evidence (SPEC-027 §9.3). Never fabricated from I/O loss.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostExitKind {
    Completed,
    Failed,
    Crashed,
    /// Reap could not establish a definite outcome (e.g. forced kill after a
    /// bounded wait with no exit status observed).
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostExitEvidence {
    pub kind: HostExitKind,
}

/// Why `start` did not produce spawn evidence (SPEC-027 §9.3 typed not-started).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostStartFailure {
    /// Offering requires a TTY; `StandaloneProcessHost` is pipe-safe only
    /// (SPEC-027 §9.5 / fixture 15).
    TtyRequired,
    SpawnFailed,
}

/// Typed host seam: adapters produce observations; backend/domain commits.
///
/// Implementations must not fabricate run termination solely from observation
/// I/O loss. Crash and disconnect map to honest liveness classifications at
/// the observation authority (UnknownAfterCrash / ObservationLost), not
/// KnownTerminated, unless the host has authoritative exit evidence.
///
/// SPEC-027 §9.1: `start` returns after spawn/admission and MUST NOT wait for
/// child exit; `observe` is a non-blocking poll of accumulated observations;
/// `signal_cancel` is best-effort; `reap` is bounded (AGENTS.md termination
/// invariant). Implementors take `&self` so a shared handle can be invoked
/// without holding any caller-side service mutex across process I/O (§9.2).
pub trait ExecutionHost {
    type Observation;
    type Error;
    type Handle;

    fn kind(&self) -> ExecutionHostKind;

    /// Admit/spawn one AgentRun under the presented binding generation.
    /// Returns `Ok(handle)` once spawn evidence exists, or a typed
    /// not-started failure. MUST NOT block on child exit.
    fn start(
        &self,
        run_id: AgentRunId,
        binding_generation: BindingGeneration,
        descriptor: LaunchDescriptor,
    ) -> Result<Self::Handle, HostStartFailure>;

    /// Non-blocking drain of observations accumulated since the last call.
    fn observe(&self, handle: &Self::Handle) -> Result<Vec<Self::Observation>, Self::Error>;

    /// Best-effort cancel signal to a live child. Not proof of termination.
    fn signal_cancel(&self, handle: &Self::Handle) -> Result<(), Self::Error>;

    /// Bounded wait for the child to exit (forcing termination if needed),
    /// returning authoritative exit evidence bound to this exact handle.
    fn reap(&self, handle: &Self::Handle) -> Result<HostExitEvidence, Self::Error>;
}
