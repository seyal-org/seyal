import AppKit

/// Pane-owned Flow presentation host for a non-focused live Pane leaf (#936 / #1424).
///
/// The ApplicationRoot already owns the Controller in the bridge registry
/// (ADR-017 create→attach→bind). This view never opens or invents executions;
/// it presents `seyal_bridge_frame_for(displayHandle)` into a production
/// `MetalSurfaceView` and, in Flow mode, owns that Pane's Block transcript +
/// subdued composer so focus changes never replace Blocks with a raw Metal
/// surface.
@MainActor
final class SecondaryLivePaneHost: NSView {
    let appHandle: UInt64
    let paneLo: UInt64
    let paneHi: UInt64
    var onFocus: (() -> Void)?
    var onRetryRecovery: ((SeyalAppPaneRecoveryTarget) -> Void)?
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
    private let statusLabel = NSTextField(labelWithString: "")
    private let retryButton = NSButton(title: "Retry", target: nil, action: nil)
    private var surfaceTopToHost: NSLayoutConstraint!
    private var surfaceTopToTranscript: NSLayoutConstraint!
    private var surfaceBottomToHost: NSLayoutConstraint!
    private var surfaceBottomToComposer: NSLayoutConstraint!
    private var flowConstraints: [NSLayoutConstraint] = []

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

    /// A secondary pane has no keyboard authority, but it is the visible
    /// hit target for moving Rust-owned focus to that pane. Composer and
    /// recovery Retry keep their own hit targets.
    override func hitTest(_ point: NSPoint) -> NSView? {
        let localPoint = convert(point, from: superview)
        guard bounds.contains(localPoint) else { return nil }
        let hit = super.hitTest(point)
        if let hit, hit === retryButton || hit.isDescendant(of: composer) || hit === composer {
            return hit
        }
        if displayHandle == 0, hit === retryButton {
            return retryButton
        }
        return self
    }

    override func mouseDown(with event: NSEvent) {
        onFocus?()
    }

    override func accessibilityPerformPress() -> Bool {
        onFocus?()
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

    override func layout() {
        super.layout()
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

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("SecondaryLivePaneHost is programmatic")
    }

    func updateDisplayHandle(_ handle: UInt64) {
        guard handle != displayHandle else { return }
        displayHandle = handle
        updateAvailabilityPresentation()
        needsLayout = true
        publishFrame()
        reconcileFlowPresentation()
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

    func reconcileFlowPresentation() {
        applyTranscriptPresentation()
        composer.reconcile()
        composer.alphaValue = composer.isHidden ? 0 : 0.78
        rebuildBlocks()
    }

    func publishFrame() {
        guard displayHandle != 0, window != nil, !isHidden else { return }
        guard seyal_bridge_ensure_prepared_for(displayHandle) == 0 else { return }
        let frame = seyal_bridge_frame_for(displayHandle)
        guard frame.cells != nil, frame.cell_count > 0 else { return }
        surface.consumeBridgeFrame(frame)
        setAccessibilityValue("rows=\(frame.rows) columns=\(frame.columns)")
    }

    private func updateAvailabilityPresentation() {
        let available = displayHandle != 0
        surface.isHidden = !available
        statusLabel.isHidden = available
        retryButton.isHidden = available
            || recoveryTarget.flags != UInt16(SEYAL_APP_PANE_RECOVERY_RETRYABLE)
            || recoveryTarget.size == 0
        guard !available else {
            setAccessibilityRole(.button)
            setAccessibilityLabel("Focus secondary terminal pane")
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
            NSLayoutConstraint.deactivate(flowConstraints)
            NSLayoutConstraint.activate([surfaceTopToHost, surfaceBottomToHost])
            let mode: TerminalPresentationMode =
                eligibility == UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue) ? .tui : .raw
            surface.applyRendererPresentation(.fullPane(mode))
            surface.setLiveTailBlocks([:])
            surface.removeTranscriptRegions(except: [])
            composer.isHidden = true
        } else {
            NSLayoutConstraint.deactivate([surfaceTopToHost, surfaceBottomToHost])
            NSLayoutConstraint.activate(flowConstraints)
            surface.applyRendererPresentation(.flow())
            // Composer visibility still follows pane-scoped reconcile().
            composer.reconcile()
            composer.alphaValue = composer.isHidden ? 0 : 0.78
        }
        needsLayout = true
    }
}
