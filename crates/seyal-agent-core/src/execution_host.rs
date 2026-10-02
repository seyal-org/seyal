//! Typed ExecutionHost seam (SPEC-018 §2).
//!
//! Concrete hosts and observation payload types live beside the Agent Backend
//! observation path. This crate owns only the domain discriminator and trait
//! so adapters cannot invent a second lifecycle authority.

use crate::{AgentRunId, BindingGeneration};

/// SPEC-018 §2 ExecutionHost family discriminator.
///
/// `SeyalTerminalExecutionHost` and `FutureRemoteExecutionHost` are accepted
/// family members but out of AB-1.4 scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionHostKind {
    Fake,
    StandaloneProcess,
}

/// Typed host seam: adapters produce observations; backend/domain commits.
///
/// Implementations must not fabricate run termination solely from observation
/// I/O loss. Crash and disconnect map to honest liveness classifications at
/// the observation authority (UnknownAfterCrash / ObservationLost), not
/// KnownTerminated, unless the host has authoritative exit evidence.
pub trait ExecutionHost {
    type Observation;
    type Error;

    fn kind(&self) -> ExecutionHostKind;

    /// Collect observations for one AgentRun under the presented binding generation.
    fn collect_observations(
        &mut self,
        run_id: AgentRunId,
        binding_generation: BindingGeneration,
    ) -> Result<Vec<Self::Observation>, Self::Error>;
}
