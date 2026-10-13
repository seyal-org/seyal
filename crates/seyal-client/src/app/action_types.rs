//! Public application actions accepted by the portable product-state root.

use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppAction {
    Focus {
        fence: AppFence,
    },
    Bind {
        fence: AppFence,
        evidence: BindingEvidence,
    },
    Refresh {
        fence: AppFence,
        alternate_screen: bool,
    },
    SubmitInput {
        fence: AppFence,
        text: String,
    },
    Quit,
    AckEffect,
    BeginRecovery {
        now: Duration,
    },
    CompleteRecovery {
        generation: u64,
        outcome: AttemptOutcome,
        now: Duration,
        launch: Option<LaunchResult>,
    },
    FireScheduledRecovery {
        generation: u64,
        now: Duration,
    },
    AckRecoveryEffect,
    CancelRecovery,
    /// Host presentation progress after connect (`RestoringInteraction` / `Usable`).
    AdvanceRecoveryStage {
        stage: RecoveryStage,
    },
    /// Begin a continuity-identity commit attempt (pins retained across episodes).
    BeginReconstructionAttempt,
    /// Commit Runtime/execution continuity and a fresh attachment. Rust is the
    /// sole fencing authority (ADR-015 / #1065).
    CommitReconstruction {
        runtime: ContinuityIdentity,
        execution: ContinuityIdentity,
        attachment: ContinuityIdentity,
        controller_authority_committed: bool,
        authoritative_snapshot_committed: bool,
    },
    /// Mark reconstruction disconnected after the host drops the live client.
    DisconnectReconstruction,
    SetComposerDraft {
        fence: AppFence,
        text: String,
        composer_epoch: u64,
    },
    SubmitComposer {
        fence: AppFence,
        composer_epoch: u64,
    },
    ApplyComposerResult {
        fence: AppFence,
        request_id: u64,
        accepted: bool,
    },
    ApplyRuntimeBlocks {
        fence: AppFence,
        records: Vec<RuntimeBlockRecord>,
    },
    /// Relay the attached client's Runtime-published composer eligibility
    /// (ADR-009 invariant 7). `None` clears it when the transport is lost.
    ApplyRuntimeComposerStatus {
        fence: AppFence,
        eligibility: Option<RuntimeComposerEligibility>,
        revision: u64,
    },
    /// User choice of the non-TUI presentation (#867).
    /// `raw` keeps Raw across TUI entry and exit. Clearing it follows
    /// structured eligibility again.
    SelectRestingPresentation {
        fence: AppFence,
        raw: bool,
    },
    SetLeftPanel {
        mode: LeftPanelMode,
    },
    SetInspectorMode {
        mode: InspectorMode,
    },
    SelectAgent {
        fence: AppFence,
        id: AgentId,
    },
    OpenAttention {
        fence: AppFence,
        id: AttentionId,
    },
    ReplaceChrome {
        fence: AppFence,
        agents: Vec<crate::chrome::AgentRecord>,
        attention: Vec<crate::chrome::AttentionItem>,
    },
    SelectWorkspace {
        id: WorkspaceId,
    },
    SelectTab {
        id: TabId,
    },
    CreateTab,
    CloseWindow {
        id: WindowId,
        containment_generation: u64,
    },
    CloseTab {
        id: TabId,
        containment_generation: u64,
    },
    TerminateExecution {
        fence: AppFence,
    },
    /// ADR-018 §3.3: adopt a live-unpresented execution with Runtime evidence.
    Adopt {
        fence: AppFence,
        evidence: BindingEvidence,
    },
    /// Test/Runtime sync of the live-unpresented catalog.
    #[cfg(test)]
    SyncLiveUnpresented {
        entries: Vec<(ExecutionId, WorkspaceId)>,
    },
    /// Record one live-unpresented execution (tests construct Unpresented directly).
    #[cfg(test)]
    RecordUnpresented {
        execution: ExecutionId,
        workspace: WorkspaceId,
    },
    /// Explicit terminate of a live-unpresented execution (ADR-018 §3.3).
    /// Distinct from [`AppAction::TerminateExecution`] (P4 fenced Controller dispose).
    TerminateUnpresented {
        execution: ExecutionId,
    },
    SplitFocused {
        axis: SplitAxis,
    },
    ClosePane {
        id: PaneId,
        containment_generation: u64,
    },
    FocusPane {
        id: PaneId,
    },
    /// Drag the divider that `pane` leads to a pointer position (#928).
    MoveSplitDivider {
        pane: PaneId,
        position: SplitPosition,
    },
    SetShellVisibility {
        left: bool,
        inspector: bool,
        tab_strip: bool,
    },
    OpenComposerHistory {
        fence: AppFence,
    },
    SetComposerHistoryFilter {
        fence: AppFence,
        query: String,
    },
    MoveComposerHistorySelection {
        fence: AppFence,
        delta: i32,
    },
    SelectComposerHistory {
        fence: AppFence,
        composer_epoch: u64,
    },
    CloseComposerHistory {
        fence: AppFence,
    },
    /// Global keyboard-first command palette (#932). Rows/selection are
    /// derived fresh from Shell/Chrome; no command is ever fabricated.
    OpenPalette {
        fence: AppFence,
    },
    SetPaletteQuery {
        fence: AppFence,
        query: String,
    },
    MovePaletteSelection {
        fence: AppFence,
        delta: i32,
    },
    /// Run the selected palette row. When `address` is `Some`, Navigate that
    /// host-echoed address (SPEC-022 R7.2). When `None`, run the frozen
    /// selected verb/chrome command. Never re-resolves by ordinal.
    RunPalette {
        fence: AppFence,
        address: Option<ResourceAddress>,
    },
    /// Atomic Navigate(address) commit (SPEC-022 §4).
    Navigate {
        fence: AppFence,
        address: ResourceAddress,
    },
    /// Focus-history Back (SPEC-022 §6 / N3). `observed` must match cursor.
    HistoryBack {
        fence: AppFence,
        observed: FocusSeq,
    },
    /// Focus-history Forward (SPEC-022 §6 / N3). `observed` must match cursor.
    HistoryForward {
        fence: AppFence,
        observed: FocusSeq,
    },
    ClosePalette {
        fence: AppFence,
    },
    /// Navigation-only goto / quick-switcher (SPEC-022 §7 / N4).
    OpenGoto {
        fence: AppFence,
        scope: GotoScope,
    },
    SetGotoScope {
        fence: AppFence,
        scope: GotoScope,
    },
    CycleGotoScope {
        fence: AppFence,
    },
    SetGotoQuery {
        fence: AppFence,
        query: String,
    },
    MoveGotoSelection {
        fence: AppFence,
        delta: i32,
    },
    /// Run the selected goto row by stored/host-echoed address.
    RunGoto {
        fence: AppFence,
        address: Option<ResourceAddress>,
    },
    CloseGoto {
        fence: AppFence,
    },
    /// Bind the inspector to one Block of the focused Pane (#935).
    SelectBlock {
        fence: AppFence,
        id: BlockId,
    },
    ClearBlockSelection {
        fence: AppFence,
    },
}
