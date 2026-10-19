import AppKit

struct PaneLayoutGeometrySample: Equatable {
    let viewportSize: CGSize
    let cellSize: CGSize

    func beginsMeaningfulEpoch(after previous: PaneLayoutGeometrySample?) -> Bool {
        guard let previous else { return true }
        return viewportSize != previous.viewportSize || cellSize != previous.cellSize
    }
}

struct PaneLayoutGeometryEpochTracker {
    private struct PaneKey: Hashable {
        let lo: UInt64
        let hi: UInt64
    }

    private struct Observation {
        let displayHandle: UInt64
        let sample: PaneLayoutGeometrySample
    }

    private var observations: [PaneKey: Observation] = [:]

    mutating func observe(
        _ sample: PaneLayoutGeometrySample,
        paneLo: UInt64,
        paneHi: UInt64,
        displayHandle: UInt64
    ) -> Bool {
        let key = PaneKey(lo: paneLo, hi: paneHi)
        let previous = observations[key]
        observations[key] = Observation(displayHandle: displayHandle, sample: sample)
        guard previous?.displayHandle == displayHandle else { return true }
        return sample.beginsMeaningfulEpoch(after: previous?.sample)
    }

    mutating func removePane(paneLo: UInt64, paneHi: UInt64) {
        observations.removeValue(forKey: PaneKey(lo: paneLo, hi: paneHi))
    }
}

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
    var onRetryRecovery: ((SeyalAppPaneRecoveryTarget) -> Void)?
    private(set) var displayHandle: UInt64
    private var recoveryTarget: SeyalAppPaneRecoveryTarget
    private let surface: MetalSurfaceView
    private let statusLabel = NSTextField(labelWithString: "")
    private let retryButton = NSButton(title: "Retry", target: nil, action: nil)
    private var lastObservedLayoutSample: PaneLayoutGeometrySample?
    var beginsLayoutEpoch: ((PaneLayoutGeometrySample, UInt64) -> Bool)?

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
        addSubview(surface)
        addSubview(statusLabel)
        addSubview(retryButton)
        NSLayoutConstraint.activate([
            surface.leadingAnchor.constraint(equalTo: leadingAnchor),
            surface.trailingAnchor.constraint(equalTo: trailingAnchor),
            surface.topAnchor.constraint(equalTo: topAnchor),
            surface.bottomAnchor.constraint(equalTo: bottomAnchor),
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
        if displayHandle == 0 {
            return super.hitTest(localPoint)
        }
        return bounds.contains(localPoint) ? self : nil
    }

    override func mouseDown(with event: NSEvent) {
        if displayHandle != 0 { onFocus?() }
    }

    override func accessibilityPerformPress() -> Bool {
        guard displayHandle != 0 else { return false }
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
        let sample = PaneLayoutGeometrySample(
            viewportSize: CGSize(width: rounded.width, height: rounded.height),
            cellSize: cell
        )
        let beginsMeaningfulEpoch = beginsLayoutEpoch?(sample, displayHandle)
            ?? sample.beginsMeaningfulEpoch(after: lastObservedLayoutSample)
        lastObservedLayoutSample = sample
        // Only point-space viewport and permanent renderer metrics cross the
        // ABI. Rust derives the grid and verifies the Pane → ExecutionId before
        // its own client submits the correlated SPEC-004 resize.
        _ = seyal_app_propose_pane_geometry(
            appHandle,
            paneLo,
            paneHi,
            Double(rounded.width),
            Double(rounded.height),
            0,
            0,
            Double(cell.width),
            Double(cell.height),
            beginsMeaningfulEpoch ? 1 : 0
        )
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("SecondaryLivePaneHost is programmatic")
    }

    func updateDisplayHandle(_ handle: UInt64) {
        guard handle != displayHandle else { return }
        displayHandle = handle
        lastObservedLayoutSample = nil
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
