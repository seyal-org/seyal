import AppKit

/// Structural identity of a pane's Block/composer projection. Live output
/// ranges are refreshed in place so streaming cannot recreate cards or reset
/// a scrolled transcript (#1424).
struct PaneBlockProjectionFingerprint: Equatable {
    struct Row: Equatable {
        let idLo: UInt64
        let idHi: UInt64
        let flags: UInt16
    }

    let eligibility: UInt16
    /// A Pane can switch between Runtime Controllers without changing its local
    /// Block ids or projection rows. Include the selected Controller so a tab
    /// switch rebuilds the pane's view from that execution's timeline.
    let displayHandle: UInt64
    let blockCount: UInt32
    let floorLo: UInt64
    let floorHi: UInt64
    let rows: [Row]
}

struct PaneTranscriptScrollState: Equatable {
    let offsetY: CGFloat
    let documentHeight: CGFloat
    let followsLiveEnd: Bool
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
    /// Product shortcut consumed in this pane's composer (e.g. ⌘D); host must
    /// reconcile chrome and re-route first responder (#1464).
    var onCommandConsumed: (() -> Void)?
    /// Shared window bridge for completed-Block history (surfaces are bridgeless).
    weak var historyBridge: RustDisplayBridge?
    private(set) var displayHandle: UInt64
    private var recoveryTarget: SeyalAppPaneRecoveryTarget
    let surface: InteractiveMetalSurfaceView
    private(set) var composer: ComposerBridgeView!
    let transcript = NSScrollView()
    let blocks = NSStackView()
    /// Leading filler so sparse Flow content hugs the bottom, just above the
    /// composer, instead of top-packing with a dead zone below it (#1426
    /// follow-up / Warp-like Block stacking). Height is driven explicitly by
    /// `syncTranscriptDocumentSize`, never by NSStackView distribution.
    let transcriptLeadingSpacer = NSView()
    var transcriptLeadingSpacerHeight: NSLayoutConstraint!
    var blockCards: [UInt64: CommandBlockView] = [:]
    var lastBlockCount = 0
    var followingLiveEnd = true
    var isProgrammaticTranscriptScroll = false
    var transcriptFrameRevision: UInt64 = 0
    private var lastTranscriptViewportSize: CGSize = .zero
    private var hasPendingTranscriptViewportSync = false
    var blockOutputZeroClipRetries = 0
    var lastPublishedTranscriptRegions: [NativeTranscriptRegion] = []
    var lastVisibleHistoryPixelCount = 0
    var lastHistoryPixelSampleAt = Date.distantPast
    var historyProjectionRequests = HistoryProjectionRequestBook()
    var transcriptScrollStates: [UInt64: PaneTranscriptScrollState] = [:]
    var restoresScrollForDisplayHandleChange = false
    static let blockOutputZeroClipRetryLimit = 4
    /// When true, this host owns keyboard/composer interaction for its Pane.
    private(set) var inputOwned = false
    private let statusLabel = NSTextField(labelWithString: "")
    private let retryButton = NSButton(title: "Retry", target: nil, action: nil)
    private var surfaceLeadingToHost: NSLayoutConstraint!
    private var surfaceLeadingToTranscript: NSLayoutConstraint!
    private var surfaceTrailingToHost: NSLayoutConstraint!
    private var surfaceTrailingToTranscript: NSLayoutConstraint!
    private var surfaceTopToHost: NSLayoutConstraint!
    private var surfaceTopToTranscript: NSLayoutConstraint!
    private var surfaceBottomToHost: NSLayoutConstraint!
    private var surfaceBottomToTranscript: NSLayoutConstraint!
    private var transcriptBottomToComposer: NSLayoutConstraint!
    private var transcriptBottomToHost: NSLayoutConstraint!
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
        surface = InteractiveMetalSurfaceView(
            frame: .zero,
            appHandle: appHandle,
            paneID: String(format: "live-%016llx%016llx", paneHi, paneLo)
        )
        composer = ComposerBridgeView(appHandle: appHandle, paneLo: paneLo, paneHi: paneHi)
        composer.boundDisplayHandle = displayHandle
        super.init(frame: .zero)
        surface.activateTranscriptDisplayHandle(displayHandle)
        surface.onEnsureDirectInputBridge = { [weak self] in
            self?.ensureDirectInputBridgeBorrow()
        }
        // Unfocused Flow↔TUI from Candidate-D alt-screen (#867 / #1474).
        surface.onAlternateScreenChanged = { [weak self] _ in
            self?.reconcileFlowPresentation()
            self?.onCommandConsumed?()
        }
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
            self?.onCommandConsumed?()
        }

        blocks.orientation = .vertical
        blocks.alignment = .width
        blocks.spacing = 10
        blocks.translatesAutoresizingMaskIntoConstraints = false
        blocks.setAccessibilityElement(true)
        blocks.setAccessibilityRole(.list)
        blocks.setAccessibilityIdentifier("seyal-secondary-blocks")
        transcriptLeadingSpacer.translatesAutoresizingMaskIntoConstraints = false
        transcriptLeadingSpacer.setAccessibilityElement(false)
        transcriptLeadingSpacerHeight = transcriptLeadingSpacer.heightAnchor.constraint(
            equalToConstant: 0
        )
        transcriptLeadingSpacerHeight.isActive = true
        blocks.insertArrangedSubview(transcriptLeadingSpacer, at: 0)
        let clip = TranscriptClipView()
        clip.drawsBackground = false
        clip.copiesOnScroll = false
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

        surfaceLeadingToHost = surface.leadingAnchor.constraint(equalTo: leadingAnchor)
        surfaceLeadingToTranscript = surface.leadingAnchor.constraint(equalTo: transcript.leadingAnchor)
        surfaceTrailingToHost = surface.trailingAnchor.constraint(equalTo: trailingAnchor)
        surfaceTrailingToTranscript = surface.trailingAnchor.constraint(equalTo: transcript.trailingAnchor)
        surfaceTopToHost = surface.topAnchor.constraint(equalTo: topAnchor)
        // Flow Block history is painted by Metal into the card body clips.
        // Keep the surface coextensive with the transcript viewport so those
        // overlay regions use valid surface coordinates, as on the primary
        // Flow pane. Placing Metal below the transcript left command headers
        // visible while every completed output body stayed blank (#1426).
        surfaceTopToTranscript = surface.topAnchor.constraint(equalTo: transcript.topAnchor)
        surfaceBottomToHost = surface.bottomAnchor.constraint(equalTo: bottomAnchor)
        surfaceBottomToTranscript = surface.bottomAnchor.constraint(equalTo: transcript.bottomAnchor)
        transcriptBottomToComposer = transcript.bottomAnchor.constraint(
            equalTo: composer.topAnchor,
            constant: -12
        )
        transcriptBottomToComposer.priority = .defaultHigh
        transcriptBottomToHost = transcript.bottomAnchor.constraint(
            equalTo: bottomAnchor,
            constant: -10
        )
        flowConstraints = [
            transcript.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 12),
            transcript.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -12),
            transcript.topAnchor.constraint(equalTo: topAnchor, constant: 10),
            transcript.heightAnchor.constraint(greaterThanOrEqualToConstant: 80),
            transcriptBottomToComposer,
            composer.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 16),
            composer.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -16),
            composer.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -10),
            surfaceLeadingToTranscript,
            surfaceTrailingToTranscript,
            surfaceTopToTranscript,
            surfaceBottomToTranscript,
        ]
        NSLayoutConstraint.activate([
            surfaceLeadingToHost,
            surfaceTrailingToHost,
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
        NSLayoutConstraint.deactivate([
            surfaceLeadingToHost,
            surfaceTrailingToHost,
            surfaceTopToHost,
            surfaceBottomToHost,
        ])

        updateRecoveryTarget(recoveryTarget)
        updateAvailabilityPresentation()
        // Do not rebuild Blocks in init at a zero frame — that trips AutoresizingMask
        // conflicts and can stall the main thread during multipane install (#1424).
        // PaneLayoutView frames the host, then noteRegistryPolled syncs projection.
        applyTranscriptPresentation()
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
            let hit = super.hitTest(point)
            // Match ThinPaneHostView: Flow Metal returns nil, but the host must
            // not swallow the click or XCUI/`seyal-block-*-body` stay obscured.
            if hit === self,
                !surface.inspectRendererPresentation().drawsLiveGrid,
                surface.rawScrollOffset == 0
            {
                return nil
            }
            return hit
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
        rememberTranscriptScrollState()
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
        // History instance origins are baked in surface pixels; scroll must
        // re-publish clips/origins even for programmatic follow-to-live-end /
        // offset restore, or Metal history sticks at the pre-scroll geometry
        // (#1379 / stuck glyphs). Only the follow-tail latch is gated.
        if !isProgrammaticTranscriptScroll {
            followingLiveEnd = isNearLiveEnd()
            rememberTranscriptScrollState()
        }
        publishBlockOutputFrame()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("SecondaryLivePaneHost is programmatic")
    }

    @discardableResult
    func updateDisplayHandle(_ handle: UInt64) -> Bool {
        guard handle != displayHandle else { return false }
        rememberTranscriptScrollState()
        historyProjectionRequests.discard(forDisplayHandle: displayHandle)
        surface.activateTranscriptDisplayHandle(handle)
        displayHandle = handle
        composer.boundDisplayHandle = handle
        restoresScrollForDisplayHandleChange = true
        updateAvailabilityPresentation()
        needsLayout = true
        publishFrame()
        // Both split-time binding (0→bound) and same-pane terminal-tab changes
        // select a different Runtime timeline. Reconcile against that
        // Controller so local Block ids cannot retain another tab's card/body.
        noteRegistryPolled()
        return true
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
        // Focused: Metal/composer FR. Unfocused: still retract Busy dock while
        // this Controller's REPL owns DirectInput (#1467 multipane).
        focusBlockScopedDirectInputIfEligible()
        // Retag retained cards without rebuilding so focus ownership swaps do
        // not drop XCUI Block identity or force a transcript reflow (#1424).
        retagArrangedBlockAccessibilityIdentifiers()
    }

    /// Status / focus path: re-hide the Busy dock and reclaim Metal without a
    /// Block rebuild (DirectInput does not bump snapshot generation).
    func refreshBlockScopedDirectInputPresentation() {
        composer.reconcile()
        focusBlockScopedDirectInputIfEligible()
    }

    /// This pane owns Flow Block-scoped DirectInput. Prefer the per-pane
    /// composer eligibility flag (survives sibling focus); fall back to the
    /// Controller target. Never the shared ApplicationRoot snapshot flag.
    private var panePublishesBlockScopedDirectInput: Bool {
        let eligibility = seyal_app_eligibility_for_pane(appHandle, paneLo, paneHi)
        guard eligibility == UInt16(SEYAL_APP_ELIGIBILITY_FLOW.rawValue) else { return false }
        let paneComposer = seyal_app_composer_for_pane(appHandle, paneLo, paneHi)
        guard displayHandle != 0 else {
            return seyalComposerFlagsPublishBlockScopedDirectInput(paneComposer.flags)
        }
        return seyalPublishesBlockScopedDirectInput(
            flags: paneComposer.flags,
            target: seyal_bridge_composer_target_for_handle(displayHandle)
        )
    }

    /// Re-arm borrowed bridge + DirectInput flag before AX focus / keyDown.
    private func ensureDirectInputBridgeBorrow() {
        let direct = panePublishesBlockScopedDirectInput
        surface.panePublishesBlockScopedDirectInput = direct
        guard inputOwned else { return }
        if surface.bridge == nil, let historyBridge {
            surface.bridge = historyBridge
        }
    }

    /// Focused Flow + DirectInput: keys go to this pane's live surface (#1468).
    /// When DirectInput ends, return keys to this pane's composer (Warp-like).
    /// Unfocused panes still retract the Busy dock while their REPL owns mode 3.
    private func focusBlockScopedDirectInputIfEligible() {
        let direct = panePublishesBlockScopedDirectInput
        applyDirectInputMergedLayout(direct)
        ensureDirectInputBridgeBorrow()
        guard inputOwned else {
            // Never stop the shared Controller bridge; only drop the borrow.
            if surface.bridge === historyBridge {
                surface.bridge = nil
            }
            surface.syncInputRoutePresentation()
            return
        }
        surface.syncInputRoutePresentation()
        guard direct else {
            _ = composer.focusEditor()
            return
        }
        window?.makeFirstResponder(surface)
    }

    /// Warp-like: retract the Busy dock and fill the transcript while DirectInput
    /// owns the Running Block; restore the dock when DirectInput ends.
    private func applyDirectInputMergedLayout(_ directInput: Bool) {
        if directInput {
            if transcriptBottomToComposer.isActive {
                transcriptBottomToComposer.isActive = false
            }
            if !transcriptBottomToHost.isActive {
                transcriptBottomToHost.isActive = true
            }
            composer.isHidden = true
        } else {
            if transcriptBottomToHost.isActive {
                transcriptBottomToHost.isActive = false
            }
            if !transcriptBottomToComposer.isActive {
                transcriptBottomToComposer.isActive = true
            }
            composer.reconcile()
        }
        needsLayout = true
        // Focus-switch DirectInput must not leave Metal at a zero frame —
        // XCUI/AppKit then synthesize keys into an empty target.
        layoutSubtreeIfNeeded()
        publishFrame()
    }

    private func retagArrangedBlockAccessibilityIdentifiers() {
        let prefix = inputOwned ? "seyal-block-" : "seyal-secondary-block-"
        for (visibleIndex, card) in arrangedBlockViews.enumerated() {
            card.setAccessibilityIdentifier("\(prefix)\(visibleIndex)")
            card.setBodyAccessibilityIdentifier("\(prefix)\(visibleIndex)-body")
        }
    }

    func publishFrame() {
        // Keep consuming bridge frames while this host is covered by focused
        // Raw/TUI `liveContent` (`isHidden == true`). Otherwise unfocus reveals
        // a stale Metal buffer and drops lines typed/drawn while focused
        // (grid-first multipane headed: vim/`took 2s` vanishing on focus out).
        guard displayHandle != 0, window != nil else { return }
        guard seyal_bridge_ensure_prepared_for(displayHandle) == 0 else { return }
        let frame = seyal_bridge_frame_for(displayHandle)
        guard frame.cells != nil, frame.cell_count > 0 else { return }
        surface.consumeBridgeFrame(frame)
        // Include this Pane's binding identity so multipane XCUI and recovery
        // diagnostics never label a sibling's frame through the process-wide
        // focused-display selector (#1424).
        let execution = paneExecutionIdentity()
        let paint = surface.renderer.inspectFlowPaint()
        let surfaceBounds = surface.bounds
        let visibleHistoryClips = lastPublishedTranscriptRegions.filter {
            $0.clip.intersects(surfaceBounds)
        }.count
        let clipSummary = lastPublishedTranscriptRegions.first.map {
            String(
                format: "clip=%.1f,%.1f,%.1f,%.1f",
                $0.clip.minX,
                $0.clip.minY,
                $0.clip.width,
                $0.clip.height
            )
        } ?? "clip=none"
        setAccessibilityValue(
            "rows=\(frame.rows) columns=\(frame.columns) execution=\(execution) "
                + "flow-paint=\(paint.accessibilityToken) "
                + "history-instances=\(paint.historyInstanceCount) "
                + "history-regions-painted=\(paint.paintedHistoryRegionCount) "
                + "history-clips=\(paint.historyClipsDistinct ? "ok" : "collapse") "
                + "history-visible-pixels=\(lastVisibleHistoryPixelCount) "
                + "surface-bounds=\(NSStringFromRect(surfaceBounds)) visible-history-clips=\(visibleHistoryClips) "
                + clipSummary
        )
    }

    private func paneExecutionIdentity() -> String {
        let paneCount = Int(seyal_app_shell(appHandle).pane_count)
        for index in 0..<paneCount {
            let binding = seyal_app_pane_binding(appHandle, UInt32(index))
            guard binding.pane_lo == paneLo, binding.pane_hi == paneHi else { continue }
            return String(format: "%016llx%016llx", binding.execution_hi, binding.execution_lo)
        }
        return String(repeating: "0", count: 32)
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
            if mode == .tui {
                surface.clearRawScrollbackFollowTail()
            }
            surface.applyRendererPresentation(.fullPane(mode))
            surface.setLiveTailBlocks([:])
            surface.removeTranscriptRegions(except: [])
            // Wiping Metal/history caches must also clear the settled request
            // book. Otherwise return-to-Flow rebuilds skip HistoryRangeRequest
            // for already-settled Block keys and leave permanently blank bodies
            // (#1379 adversarial review B2).
            historyProjectionRequests.discard(forDisplayHandle: displayHandle)
            composer.isHidden = true
            transcriptBottomToHost.isActive = false
            clearPreservedDocumentHeight()
            surface.preserveRawScrollbackAfterPresentationChange(eligibility: eligibility)
        } else {
            // Same as primary: Raw scrollback must not survive into Flow Metal.
            surface.clearRawScrollbackFollowTail()
            activateFlowConstraintsIfSized()
            surface.applyRendererPresentation(.flow())
            // Composer visibility still follows pane-scoped reconcile().
            composer.reconcile()
            refreshInputOwnershipChrome()
            focusBlockScopedDirectInputIfEligible()
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
            // Omit SEYAL_APP_BLOCK_SELECTED — focus/selection must not force a
            // Block view rebuild (#1424). Projection ranges and live-tail rows
            // also change during streaming and are updated in place below.
            let structuralFlags = row.flags & ~UInt16(SEYAL_APP_BLOCK_SELECTED)
            rows.append(
                PaneBlockProjectionFingerprint.Row(
                    idLo: row.id_lo,
                    idHi: row.id_hi,
                    flags: structuralFlags
                )
            )
        }
        // Empty canvas: ignore floor ids so focus/sync noise cannot force a
        // rebuild that would wipe stable host chrome (#1424).
        let floorLo = composerSnap.block_count == 0 ? 0 : composerSnap.canvas_floor_block_id_lo
        let floorHi = composerSnap.block_count == 0 ? 0 : composerSnap.canvas_floor_block_id_hi
        return PaneBlockProjectionFingerprint(
            eligibility: eligibility,
            displayHandle: displayHandle,
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
            NSLayoutConstraint.deactivate([
                surfaceLeadingToHost,
                surfaceTrailingToHost,
                surfaceTopToHost,
                surfaceBottomToHost,
            ])
            NSLayoutConstraint.activate(flowConstraints)
        } else {
            // Benign interim: fill with Metal surface until PaneLayoutView
            // publishes a non-zero region frame in the same reconcile turn.
            transcriptBottomToHost.isActive = false
            NSLayoutConstraint.deactivate(flowConstraints)
            NSLayoutConstraint.activate([
                surfaceLeadingToHost,
                surfaceTrailingToHost,
                surfaceTopToHost,
                surfaceBottomToHost,
            ])
        }
    }

    private func activateDirectSurfaceConstraints() {
        NSLayoutConstraint.deactivate(flowConstraints)
        NSLayoutConstraint.activate([
            surfaceLeadingToHost,
            surfaceTrailingToHost,
            surfaceTopToHost,
            surfaceBottomToHost,
        ])
    }

    override func layout() {
        super.layout()
        // Re-evaluate Flow pins once the region frame is non-zero.
        let eligibility = seyal_app_eligibility_for_pane(appHandle, paneLo, paneHi)
        let direct = eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue)
            || eligibility == UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue)
        if !direct {
            activateFlowConstraintsIfSized()
            let viewportSize = transcript.contentView.bounds.size
            if viewportSize.width > 8, viewportSize.height > 8,
               abs(viewportSize.width - lastTranscriptViewportSize.width) > 0.5
                || abs(viewportSize.height - lastTranscriptViewportSize.height) > 0.5
            {
                lastTranscriptViewportSize = viewportSize
                if !hasPendingTranscriptViewportSync {
                    hasPendingTranscriptViewportSync = true
                    // A split or divider drag changes the visible transcript
                    // viewport without changing the Block projection. Refit
                    // its leading spacer after Auto Layout settles so the
                    // live Block stays just above this Pane's composer and
                    // each leaf keeps its own scroll extent (#1426).
                    DispatchQueue.main.async { [weak self] in
                        guard let self else { return }
                        self.hasPendingTranscriptViewportSync = false
                        guard !self.isHidden else { return }
                        self.syncTranscriptDocumentSize()
                        self.publishBlockOutputFrame()
                    }
                }
            }
        } else {
            lastTranscriptViewportSize = .zero
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
        // Pick up the resized prepared frame for AX/PTY convergence after
        // window or divider geometry changes (#1424).
        publishFrame()
        publishBlockOutputFrame()
    }
}
