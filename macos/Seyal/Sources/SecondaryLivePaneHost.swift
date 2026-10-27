import AppKit

/// Thin Metal host for a non-focused live Pane leaf (#936).
///
/// The ApplicationRoot already owns the Controller in the bridge registry
/// (ADR-017 create→attach→bind). This view never opens or invents executions;
/// it presents `seyal_bridge_frame_for(displayHandle)` into a production
/// `MetalSurfaceView` with automatic Runtime recovery suppressed.
@MainActor
final class SecondaryLivePaneHost: NSView {
    let appHandle: UInt64
    let paneLo: UInt64
    let paneHi: UInt64
    var onFocus: (() -> Void)?
    var onClosePane: (() -> Void)?
    var onRetryRecovery: ((SeyalAppPaneRecoveryTarget) -> Void)?
    private(set) var displayHandle: UInt64
    private var recoveryTarget: SeyalAppPaneRecoveryTarget
    private let surface: MetalSurfaceView
    private let chromeBar = PaneChromeBarView()
    private var chromeTitle = "Pane"
    /// Inactive Pane still projects its own C09 composer (ADR-009); clicks focus
    /// the Pane so liveContent becomes the editable authority.
    private(set) var composer: ComposerBridgeView!
    private let statusLabel = NSTextField(labelWithString: "")
    private let retryButton = NSButton(title: "Retry", target: nil, action: nil)

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
        composer.alphaValue = 0.72
        composer.onEnsureFocused = { [weak self] in
            self?.onFocus?()
        }
        chromeBar.onClose = { [weak self] in
            self?.onClosePane?()
        }
        addSubview(chromeBar)
        addSubview(surface)
        addSubview(composer)
        addSubview(statusLabel)
        addSubview(retryButton)
        NSLayoutConstraint.activate([
            chromeBar.leadingAnchor.constraint(equalTo: leadingAnchor),
            chromeBar.trailingAnchor.constraint(equalTo: trailingAnchor),
            chromeBar.topAnchor.constraint(equalTo: topAnchor),
            surface.leadingAnchor.constraint(equalTo: leadingAnchor),
            surface.trailingAnchor.constraint(equalTo: trailingAnchor),
            surface.topAnchor.constraint(equalTo: chromeBar.bottomAnchor),
            surface.bottomAnchor.constraint(equalTo: composer.topAnchor, constant: -8),
            composer.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 16),
            composer.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -16),
            composer.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -12),
            statusLabel.centerXAnchor.constraint(equalTo: centerXAnchor),
            statusLabel.centerYAnchor.constraint(equalTo: centerYAnchor, constant: -12),
            retryButton.centerXAnchor.constraint(equalTo: centerXAnchor),
            retryButton.topAnchor.constraint(equalTo: statusLabel.bottomAnchor, constant: 8),
        ])
        updateRecoveryTarget(recoveryTarget)
        updateAvailabilityPresentation()
        publishFrame()
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        needsLayout = true
        publishFrame()
    }

    /// A secondary pane has no keyboard authority, but it is the visible
    /// hit target for moving Rust-owned focus to that pane.
    override func hitTest(_ point: NSPoint) -> NSView? {
        let localPoint = convert(point, from: superview)
        guard bounds.contains(localPoint) else { return nil }
        let hit = super.hitTest(point)
        // Pane chrome Close and recovery Retry must receive the click.
        if let hit, hit === retryButton || hit.isDescendant(of: chromeBar) || hit === chromeBar {
            return hit
        }
        // Composer/Metal hits focus this Pane so liveContent becomes editable.
        return self
    }

    override func mouseDown(with event: NSEvent) {
        onFocus?()
    }

    override func accessibilityPerformPress() -> Bool {
        onFocus?()
        return true
    }

    /// NSView's custom accessibility element defaults to disabled unless the
    /// enabled attribute is supplied. This visible focus target must advertise
    /// that it responds to mouse and accessibility press actions.
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

    override func layout() {
        super.layout()
        updateAvailabilityPresentation()
        guard displayHandle != 0, window != nil, !isHidden,
              bounds.width > 8, bounds.height > 8 else { return }
        let rounded = bounds.integral
        let cell = surface.terminalPresentationCellSize()
        guard cell.width > 0, cell.height > 0 else { return }
        // Forward native viewport and renderer measurements. Rust derives the
        // grid, detects layout changes, and owns retry authorization.
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
    }

    func updateRecoveryTarget(_ target: SeyalAppPaneRecoveryTarget) {
        recoveryTarget = target
        updateAvailabilityPresentation()
    }

    /// Called when the shared primary bridge polls this registry handle.
    func noteRegistryPolled() {
        publishFrame()
    }

    func reconcileComposer() {
        composer.reconcile()
        // Inactive composers stay visible when Available/Busy; Hidden retracts.
        composer.alphaValue = composer.isHidden ? 0 : 0.72
    }

    func updateChrome(title: String, allowsClose: Bool, theme: NativeTheme?, focused: Bool = false) {
        chromeTitle = title
        chromeBar.apply(title: title, theme: theme, focused: focused)
        chromeBar.setCloseVisible(allowsClose)
    }

    func apply(theme: NativeTheme) {
        composer.apply(theme: theme)
        chromeBar.apply(title: chromeTitle, theme: theme, focused: false)
    }

    private func publishFrame() {
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
    }
}
