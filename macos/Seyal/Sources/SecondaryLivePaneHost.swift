import AppKit

/// Identity of a pane's Block/composer projection used to skip Block rebuilds
/// when only focus/input ownership changed (#1424).
struct PaneBlockProjectionFingerprint: Equatable {
    struct Row: Equatable {
        let idLo: UInt64
        let idHi: UInt64
        let flags: UInt16
        let kind: UInt16
        let startLine: UInt64
        let endLine: UInt64
        let reserved0: UInt16
        let reserved1: UInt32
    }

    let eligibility: UInt16
    let blockCount: UInt32
    let floorLo: UInt64
    let floorHi: UInt64
    let rows: [Row]
}

/// Pane-owned Flow presentation host for a visible multipane leaf (#936 / #1424).
///
/// The ApplicationRoot already owns the Controller in the bridge registry
/// (ADR-017 create→attach→bind). This view never opens or invents executions;
/// it presents `seyal_bridge_frame_for(displayHandle)` into a production
/// `MetalSurfaceView` and, in Flow mode, owns that Pane's Block transcript +
/// composer. Hosts stay installed across focus changes; `setInputOwned`
/// toggles interaction/subdued chrome only. History requests borrow the
/// window `RustDisplayBridge` (surfaces keep `bridge == nil` by design).
@MainActor
final class SecondaryLivePaneHost: NSView {
    let appHandle: UInt64
    let paneLo: UInt64
    let paneHi: UInt64
    var onFocus: (() -> Void)?
    var onRetryRecovery: ((SeyalAppPaneRecoveryTarget) -> Void)?
    /// Shared window bridge for completed-Block history (surfaces are bridgeless).
    weak var historyBridge: RustDisplayBridge?
    private(set) var displayHandle: UInt64
    private var recoveryTarget: SeyalAppPaneRecoveryTarget
    let surface: MetalSurfaceView
    private(set) var composer: ComposerBridgeView!
    let transcript = NSScrollView()
    let blocks = NSStackView()
    var blockCards: [UInt64: CommandBlockView] = [:]
    var lastBlockCount = 0
    var followingLiveEnd = true
    var isProgrammaticTranscriptScroll = false
    var transcriptFrameRevision: UInt64 = 0
    var blockOutputZeroClipRetries = 0
    static let blockOutputZeroClipRetryLimit = 4
    /// When true, this host owns keyboard/composer interaction for its Pane.
    private(set) var inputOwned = false
    private let statusLabel = NSTextField(labelWithString: "")
    private let retryButton = NSButton(title: "Retry", target: nil, action: nil)
    private var surfaceTopToHost: NSLayoutConstraint!
    private var surfaceTopToTranscript: NSLayoutConstraint!
    private var surfaceBottomToHost: NSLayoutConstraint!
    private var surfaceBottomToComposer: NSLayoutConstraint!
    private var flowConstraints: [NSLayoutConstraint] = []
    /// Keeps transcript document extent across Block rebuilds when the user has
    /// scrolled away from live end (#1424).
    var preservedDocumentHeightConstraint: NSLayoutConstraint?
    /// Last applied Flow/Raw/TUI eligibility; focus must not re-apply modes.
    var lastAppliedEligibility: UInt16?
    /// Last pane Block/composer projection signature; focus must not rebuild.
    var lastProjectionFingerprint: PaneBlockProjectionFingerprint?

    init(
        appHandle: UInt64,
        paneLo: UInt64,
        paneHi: UInt64,
        displayHandle: UInt64,
        recoveryTarget: SeyalAppPaneRecoveryTarget
    ) {
        self.appHandle = appHandle
        self.paneLo = paneLo
        self.paneHi = paneHi
        self.displayHandle = displayHandle
        self.recoveryTarget = recoveryTarget
        surface = MetalSurfaceView(
            frame: .zero,
            paneID: String(format: "live-%016llx%016llx", paneHi, paneLo),
            allowsImplicitExecutionBootstrap: false
        )
        composer = ComposerBridgeView(appHandle: appHandle, paneLo: paneLo, paneHi: paneHi)
        super.init(frame: .zero)
        translatesAutoresizingMaskIntoConstraints = true
        clipsToBounds = true
        setAccessibilityElement(true)
        setAccessibilityRole(.button)
        setAccessibilityLabel("Focus secondary terminal pane")
        setAccessibilityIdentifier("seyal-secondary-live-pane")
        surface.suppressesAutomaticBridgeRecovery = true
        surface.translatesAutoresizingMaskIntoConstraints = false
        // Registry clients are ApplicationRoot-owned; this leaf must not stop
        // or reconnect them when the host is removed.
        surface.bridge?.stop()
        surface.bridge = nil
        statusLabel.translatesAutoresizingMaskIntoConstraints = false
        statusLabel.alignment = .center
        statusLabel.textColor = .secondaryLabelColor
        statusLabel.maximumNumberOfLines = 2
        statusLabel.setAccessibilityIdentifier("seyal-secondary-pane-recovery-status")
        retryButton.translatesAutoresizingMaskIntoConstraints = false
        retryButton.target = self
        retryButton.action = #selector(retryRecovery)
        retryButton.setAccessibilityIdentifier("seyal-secondary-pane-recovery-retry")
        retryButton.setAccessibilityLabel("Retry reconnecting terminal pane")
        retryButton.setAccessibilityHelp("Starts a bounded reconnect attempt for this pane.")
        surface.renderer.onNeedsCurrentFrame = { [weak self] in
            seyalRunAsMainActorFromMainQueue {
                self?.publishFrame()
            }
        }

        composer.translatesAutoresizingMaskIntoConstraints = false
        composer.alphaValue = 0.78
        composer.onEnsureFocused = { [weak self] in
            self?.onFocus?()
        }
        // Bridgeless surfaces borrow the window bridge; submit must still reach
        // the focused Controller so Flow commands and TUI entry work (#1424).
        composer.onSubmitComposer = { [weak self] command in
            self?.historyBridge?.submitComposerCommand(command) ?? -10
        }
        composer.onSubmitRaw = { [weak self] command in
            self?.historyBridge?.submitCommittedText(command) ?? -10
        }
        composer.onCommandConsumed = { [weak self] in
            self?.reconcileFlowPresentation()
        }

        blocks.orientation = .vertical
        blocks.alignment = .width
        blocks.spacing = 10
        blocks.translatesAutoresizingMaskIntoConstraints = false
        blocks.setAccessibilityElement(true)
        blocks.setAccessibilityRole(.list)
        blocks.setAccessibilityIdentifier("seyal-secondary-blocks")

        let clip = NSClipView()
        clip.drawsBackground = false
        clip.documentView = blocks
        transcript.contentView = clip
        transcript.drawsBackground = false
        transcript.borderType = .noBorder
        transcript.hasVerticalScroller = true
        transcript.hasHorizontalScroller = false
        transcript.autohidesScrollers = true
        transcript.automaticallyAdjustsContentInsets = false
        transcript.translatesAutoresizingMaskIntoConstraints = false
        transcript.documentView = blocks
        transcript.setAccessibilityRole(.scrollArea)
        transcript.setAccessibilityIdentifier("seyal-secondary-blocks-scroll")
        transcript.contentView.postsBoundsChangedNotifications = true
        NotificationCenter.default.addObserver(
            self,
            selector: #selector(transcriptBoundsDidChange(_:)),
            name: NSView.boundsDidChangeNotification,
            object: transcript.contentView
        )

        addSubview(transcript)
        addSubview(surface)
        addSubview(composer)
        addSubview(statusLabel)
        addSubview(retryButton)

        surfaceTopToHost = surface.topAnchor.constraint(equalTo: topAnchor)
        surfaceTopToTranscript = surface.topAnchor.constraint(equalTo: transcript.bottomAnchor, constant: 8)
        surfaceBottomToHost = surface.bottomAnchor.constraint(equalTo: bottomAnchor)
        surfaceBottomToComposer = surface.bottomAnchor.constraint(equalTo: composer.topAnchor, constant: -8)
        flowConstraints = [
            transcript.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 12),
            transcript.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -12),
            transcript.topAnchor.constraint(equalTo: topAnchor, constant: 10),
            transcript.heightAnchor.constraint(greaterThanOrEqualToConstant: 80),
            composer.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 16),
            composer.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -16),
            composer.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -10),
            surfaceTopToTranscript,
            surfaceBottomToComposer,
        ]
        NSLayoutConstraint.activate([
            surface.leadingAnchor.constraint(equalTo: leadingAnchor),
            surface.trailingAnchor.constraint(equalTo: trailingAnchor),
            surfaceTopToHost,
            surfaceBottomToHost,
            statusLabel.centerXAnchor.constraint(equalTo: centerXAnchor),
            statusLabel.centerYAnchor.constraint(equalTo: centerYAnchor, constant: -12),
            retryButton.centerXAnchor.constraint(equalTo: centerXAnchor),
            retryButton.topAnchor.constraint(equalTo: statusLabel.bottomAnchor, constant: 8),
            blocks.leadingAnchor.constraint(equalTo: transcript.contentView.leadingAnchor),
            blocks.trailingAnchor.constraint(equalTo: transcript.contentView.trailingAnchor),
            blocks.topAnchor.constraint(equalTo: transcript.contentView.topAnchor),
            blocks.widthAnchor.constraint(equalTo: transcript.contentView.widthAnchor),
        ] + flowConstraints)
        // Default Flow layout; Raw/TUI flips constraints in applyTranscriptPresentation.
        NSLayoutConstraint.deactivate([surfaceTopToHost, surfaceBottomToHost])

        updateRecoveryTarget(recoveryTarget)
        updateAvailabilityPresentation()
        reconcileFlowPresentation()
        publishFrame()
    }

    deinit {
        NotificationCenter.default.removeObserver(self)
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        needsLayout = true
        publishFrame()
        reconcileFlowPresentation()
    }

    /// Focused (input-owned) hosts use normal hit testing. Unfocused hosts are
    /// the visible hit target for Rust `FOCUS_PANE`; composer and Retry keep
    /// their own targets so drafts remain editable after ensuring focus.
    override func hitTest(_ point: NSPoint) -> NSView? {
        let localPoint = convert(point, from: superview)
        guard bounds.contains(localPoint) else { return nil }
        if inputOwned {
            return super.hitTest(point)
        }
        let hit = super.hitTest(point)
        if let hit, hit === retryButton || hit.isDescendant(of: composer) || hit === composer {
            return hit
        }
        if displayHandle == 0, hit === retryButton {
            return retryButton
        }
        return self
    }

    /// Focus ownership only — never tears down Blocks or scroll state (#1424).
    func setInputOwned(_ owned: Bool) {
        guard inputOwned != owned else {
            refreshInputOwnershipChrome()
            return
        }
        inputOwned = owned
        refreshInputOwnershipChrome()
    }

    /// Document scroll offset of the transcript (for retention tests).
    var transcriptDocumentOffsetY: CGFloat {
        transcript.contentView.bounds.origin.y
    }

    func setTranscriptDocumentOffsetY(_ y: CGFloat) {
        isProgrammaticTranscriptScroll = true
        defer { isProgrammaticTranscriptScroll = false }
        layoutSubtreeIfNeeded()
        let maxY = max(transcriptDocumentScrollHeight() - transcript.contentView.bounds.height, 0)
        let clamped = min(max(y, 0), maxY)
        transcript.contentView.scroll(to: NSPoint(x: 0, y: clamped))
        transcript.reflectScrolledClipView(transcript.contentView)
        // Mid-transcript programmatic scrolls must clear follow-bottom so the
        // next rebuild does not jump to live end (#1424).
        followingLiveEnd = maxY > 0 && clamped >= maxY - 24
    }

    override func mouseDown(with event: NSEvent) {
        if !inputOwned {
            onFocus?()
        }
    }

    override func accessibilityPerformPress() -> Bool {
        if !inputOwned {
            onFocus?()
        }
        return true
    }

    override func isAccessibilityEnabled() -> Bool {
        true
    }

    @objc private func retryRecovery() {
        guard displayHandle == 0,
            recoveryTarget.size != 0,
            recoveryTarget.flags == UInt16(SEYAL_APP_PANE_RECOVERY_RETRYABLE)
        else { return }
        onRetryRecovery?(recoveryTarget)
    }

    @objc private func transcriptBoundsDidChange(_ note: Notification) {
        guard !isProgrammaticTranscriptScroll else { return }
        followingLiveEnd = isNearLiveEnd()
        publishBlockOutputFrame()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("SecondaryLivePaneHost is programmatic")
    }

    func updateDisplayHandle(_ handle: UInt64) {
        guard handle != displayHandle else { return }
        let gainedHandle = displayHandle == 0 && handle != 0
        displayHandle = handle
        updateAvailabilityPresentation()
        needsLayout = true
        publishFrame()
        // A 0→bound transition often lands after the first empty rebuild
        // (split before Controller bind). Sync + fingerprint reconcile so
        // HISTORY spans resize Block bodies; do not leave 1-line stubs (#1424).
        if gainedHandle {
            noteRegistryPolled()
            return
        }
        let eligibility = lastAppliedEligibility
            ?? seyal_app_eligibility_for_pane(appHandle, paneLo, paneHi)
        let direct = eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue)
            || eligibility == UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue)
        if !direct {
            republishExistingBlockProjections()
            publishLiveTailBlocks()
            publishBlockOutputFrame()
        }
    }

    func updateRecoveryTarget(_ target: SeyalAppPaneRecoveryTarget) {
        recoveryTarget = target
        updateAvailabilityPresentation()
    }

    func apply(theme: NativeTheme) {
        composer.apply(theme: theme)
        for case let card as CommandBlockView in blocks.arrangedSubviews {
            card.apply(theme: theme)
        }
    }

    /// Called when the shared primary bridge polls this registry handle.
    func noteRegistryPolled() {
        _ = seyal_app_sync_pane_runtime_blocks(appHandle, paneLo, paneHi)
        publishFrame()
        reconcileFlowPresentation()
    }

    /// Focus / host-binding path: input ownership + composer chrome only.
    /// Does not rebuild Block views (#1424).
    func reconcileFocusOwnershipPresentation() {
        let eligibility = seyal_app_eligibility_for_pane(appHandle, paneLo, paneHi)
        if lastAppliedEligibility != eligibility {
            // Raw/TUI/Flow mode changes require a full projection reconcile.
            lastAppliedEligibility = nil
            lastProjectionFingerprint = nil
            reconcileFlowPresentation()
            return
        }
        composer.reconcile()
        refreshInputOwnershipChrome()
        let direct = eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue)
            || eligibility == UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue)
        if !direct {
            syncBlockSelectionFromProjection()
            publishLiveTailBlocks()
            publishBlockOutputFrame()
        }
    }

    /// Projection / registry-poll path. Rebuilds Block views only when this
    /// pane's Block/composer fingerprint changes (#1424).
    func reconcileFlowPresentation() {
        let eligibility = seyal_app_eligibility_for_pane(appHandle, paneLo, paneHi)
        if lastAppliedEligibility != eligibility {
            applyTranscriptPresentation()
            lastAppliedEligibility = eligibility
        } else {
            composer.reconcile()
            refreshInputOwnershipChrome()
        }

        let fingerprint = makeProjectionFingerprint(eligibility: eligibility)
        guard lastProjectionFingerprint != fingerprint else {
            let direct = eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue)
                || eligibility == UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue)
            if !direct {
                syncBlockSelectionFromProjection()
                // History may arrive after the structural fingerprint settles
                // (async Metal retained ranges). Keep bodies current (#1424).
                republishExistingBlockProjections()
                publishLiveTailBlocks()
                publishBlockOutputFrame()
            }
            return
        }
        lastProjectionFingerprint = fingerprint
        rebuildBlocks()
    }

    /// Test seam: force the next reconcile to rebuild Blocks from projection.
    func invalidateProjectionCacheForTests() {
        lastProjectionFingerprint = nil
        lastAppliedEligibility = nil
    }

    /// Visible `CommandBlockView` cards (for identity retention tests).
    var arrangedBlockViews: [CommandBlockView] {
        blocks.arrangedSubviews.compactMap { $0 as? CommandBlockView }
    }

    private func refreshInputOwnershipChrome() {
        let subdued: CGFloat = inputOwned ? 1.0 : 0.78
        composer.alphaValue = composer.isHidden ? 0 : subdued
        composer.setPrimaryAccessibilityIdentifier(inputOwned)
        if inputOwned {
            blocks.setAccessibilityIdentifier("seyal-blocks")
            transcript.setAccessibilityIdentifier("seyal-blocks-scroll")
            setAccessibilityRole(.group)
            setAccessibilityLabel("Focused terminal pane")
            setAccessibilityIdentifier("seyal-pane-presentation-host")
        } else {
            blocks.setAccessibilityIdentifier("seyal-secondary-blocks")
            transcript.setAccessibilityIdentifier("seyal-secondary-blocks-scroll")
            setAccessibilityRole(.button)
            setAccessibilityLabel("Focus secondary terminal pane")
            setAccessibilityIdentifier("seyal-secondary-live-pane")
        }
        // Retag retained cards without rebuilding so focus ownership swaps do
        // not drop XCUI Block identity or force a transcript reflow (#1424).
        retagArrangedBlockAccessibilityIdentifiers()
    }

    private func retagArrangedBlockAccessibilityIdentifiers() {
        let prefix = inputOwned ? "seyal-block-" : "seyal-secondary-block-"
        for (visibleIndex, card) in arrangedBlockViews.enumerated() {
            card.setAccessibilityIdentifier("\(prefix)\(visibleIndex)")
            card.setBodyAccessibilityIdentifier("\(prefix)\(visibleIndex)-body")
        }
    }

    func publishFrame() {
        guard displayHandle != 0, window != nil, !isHidden else { return }
        guard seyal_bridge_ensure_prepared_for(displayHandle) == 0 else { return }
        let frame = seyal_bridge_frame_for(displayHandle)
        guard frame.cells != nil, frame.cell_count > 0 else { return }
        surface.consumeBridgeFrame(frame)
        // Include execution= so multipane XCUI can identity Controllers without
        // the hidden liveContent `terminal-input` surface (#1424).
        let execution = String(
            format: "%016llx%016llx",
            seyal_bridge_execution_id_high(),
            seyal_bridge_execution_id_low()
        )
        setAccessibilityValue(
            "rows=\(frame.rows) columns=\(frame.columns) execution=\(execution) "
        )
    }

    private func updateAvailabilityPresentation() {
        let available = displayHandle != 0
        surface.isHidden = !available
        statusLabel.isHidden = available
        retryButton.isHidden = available
            || recoveryTarget.flags != UInt16(SEYAL_APP_PANE_RECOVERY_RETRYABLE)
            || recoveryTarget.size == 0
        guard !available else {
            refreshInputOwnershipChrome()
            return
        }
        let state: String
        switch recoveryTarget.flags {
        case UInt16(SEYAL_APP_PANE_RECOVERY_TARGET_ACTIVE):
            state = "Reconnecting terminal…"
        case UInt16(SEYAL_APP_PANE_RECOVERY_RETRYABLE):
            state = "Terminal disconnected"
        default:
            state = "Waiting to reconnect"
        }
        statusLabel.stringValue = state
        setAccessibilityRole(.group)
        setAccessibilityLabel("Secondary terminal pane unavailable")
        setAccessibilityValue(state)
        // Keep Flow Blocks/composer chrome visible while Metal recovers so
        // focus never replaces a Flow leaf with an empty raw surface (#1424).
    }

    func applyTranscriptPresentation() {
        let eligibility = seyal_app_eligibility_for_pane(appHandle, paneLo, paneHi)
        let direct = eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue)
            || eligibility == UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue)
        transcript.isHidden = direct
        if direct {
            activateDirectSurfaceConstraints()
            let mode: TerminalPresentationMode =
                eligibility == UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue) ? .tui : .raw
            surface.applyRendererPresentation(.fullPane(mode))
            surface.setLiveTailBlocks([:])
            surface.removeTranscriptRegions(except: [])
            composer.isHidden = true
            clearPreservedDocumentHeight()
        } else {
            activateFlowConstraintsIfSized()
            surface.applyRendererPresentation(.flow())
            // Composer visibility still follows pane-scoped reconcile().
            composer.reconcile()
            refreshInputOwnershipChrome()
        }
        needsLayout = true
    }

    func makeProjectionFingerprint(eligibility: UInt16) -> PaneBlockProjectionFingerprint {
        let composerSnap = seyal_app_composer_for_pane(appHandle, paneLo, paneHi)
        let count = Int(composerSnap.block_count)
        var rows: [PaneBlockProjectionFingerprint.Row] = []
        rows.reserveCapacity(count)
        for index in 0..<count {
            let row = seyal_app_block_row_for_pane(appHandle, paneLo, paneHi, UInt32(index))
            let projection = seyal_app_block_projection_for_pane(
                appHandle,
                paneLo,
                paneHi,
                UInt32(index)
            )
            // Omit SEYAL_APP_BLOCK_SELECTED — focus/selection must not force a
            // Block view rebuild (#1424).
            let structuralFlags = row.flags & ~UInt16(SEYAL_APP_BLOCK_SELECTED)
            rows.append(
                PaneBlockProjectionFingerprint.Row(
                    idLo: row.id_lo,
                    idHi: row.id_hi,
                    flags: structuralFlags,
                    kind: projection.kind,
                    startLine: projection.start_line,
                    endLine: projection.end_line,
                    reserved0: projection.reserved0,
                    reserved1: projection.reserved1
                )
            )
        }
        // Empty canvas: ignore floor ids so focus/sync noise cannot force a
        // rebuild that would wipe stable host chrome (#1424).
        let floorLo = composerSnap.block_count == 0 ? 0 : composerSnap.canvas_floor_block_id_lo
        let floorHi = composerSnap.block_count == 0 ? 0 : composerSnap.canvas_floor_block_id_hi
        return PaneBlockProjectionFingerprint(
            eligibility: eligibility,
            blockCount: composerSnap.block_count,
            floorLo: floorLo,
            floorHi: floorHi,
            rows: rows
        )
    }

    /// Avoid Auto Layout conflicts while the host frame is still height 0
    /// (autoresizing mask height==0 vs Flow composer/transcript pins).
    private func activateFlowConstraintsIfSized() {
        let sized = bounds.width >= 8 && bounds.height >= 8
        if sized {
            NSLayoutConstraint.deactivate([surfaceTopToHost, surfaceBottomToHost])
            NSLayoutConstraint.activate(flowConstraints)
        } else {
            // Benign interim: fill with Metal surface until PaneLayoutView
            // publishes a non-zero region frame in the same reconcile turn.
            NSLayoutConstraint.deactivate(flowConstraints)
            NSLayoutConstraint.activate([surfaceTopToHost, surfaceBottomToHost])
        }
    }

    private func activateDirectSurfaceConstraints() {
        NSLayoutConstraint.deactivate(flowConstraints)
        NSLayoutConstraint.activate([surfaceTopToHost, surfaceBottomToHost])
    }

    override func layout() {
        super.layout()
        // Re-evaluate Flow pins once the region frame is non-zero.
        let eligibility = seyal_app_eligibility_for_pane(appHandle, paneLo, paneHi)
        let direct = eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue)
            || eligibility == UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue)
        if !direct {
            activateFlowConstraintsIfSized()
        }
        updateAvailabilityPresentation()
        guard displayHandle != 0, window != nil, !isHidden,
              bounds.width > 8, bounds.height > 8 else { return }
        let rounded = bounds.integral
        let cell = surface.terminalPresentationCellSize()
        guard cell.width > 0, cell.height > 0 else { return }
        _ = seyal_app_propose_pane_geometry(
            appHandle,
            paneLo,
            paneHi,
            Double(rounded.width),
            Double(rounded.height),
            0,
            0,
            Double(cell.width),
            Double(cell.height)
        )
        publishBlockOutputFrame()
    }
}
