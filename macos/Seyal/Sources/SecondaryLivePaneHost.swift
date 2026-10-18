import AppKit

struct PaneLayoutGeometrySample: Equatable {
    let viewport: CGRect
    let cellSize: CGSize

    func beginsMeaningfulEpoch(after previous: PaneLayoutGeometrySample?) -> Bool {
        guard let previous else { return true }
        return viewport != previous.viewport || cellSize != previous.cellSize
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
    private(set) var displayHandle: UInt64
    private let surface: MetalSurfaceView
    private var lastObservedLayoutSample: PaneLayoutGeometrySample?

    init(appHandle: UInt64, paneLo: UInt64, paneHi: UInt64, displayHandle: UInt64) {
        self.appHandle = appHandle
        self.paneLo = paneLo
        self.paneHi = paneHi
        self.displayHandle = displayHandle
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
        surface.renderer.onNeedsCurrentFrame = { [weak self] in
            seyalRunAsMainActorFromMainQueue {
                self?.publishFrame()
            }
        }
        addSubview(surface)
        NSLayoutConstraint.activate([
            surface.leadingAnchor.constraint(equalTo: leadingAnchor),
            surface.trailingAnchor.constraint(equalTo: trailingAnchor),
            surface.topAnchor.constraint(equalTo: topAnchor),
            surface.bottomAnchor.constraint(equalTo: bottomAnchor),
        ])
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
        return bounds.contains(localPoint) ? self : nil
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

    override func layout() {
        super.layout()
        guard displayHandle != 0, window != nil, !isHidden,
              bounds.width > 8, bounds.height > 8 else { return }
        let rounded = bounds.integral
        let cell = surface.terminalPresentationCellSize()
        guard cell.width > 0, cell.height > 0 else { return }
        let sample = PaneLayoutGeometrySample(viewport: rounded, cellSize: cell)
        let beginsMeaningfulEpoch = sample.beginsMeaningfulEpoch(after: lastObservedLayoutSample)
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
        guard handle != 0, handle != displayHandle else { return }
        displayHandle = handle
        lastObservedLayoutSample = nil
        needsLayout = true
        publishFrame()
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
}
