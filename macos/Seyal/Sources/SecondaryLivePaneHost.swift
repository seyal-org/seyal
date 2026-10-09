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
    private(set) var displayHandle: UInt64
    private let surface: MetalSurfaceView
    private var lastProposedGeometry = CGRect.null
    private var lastProposedCellSize = CGSize.zero

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
        setAccessibilityRole(.group)
        setAccessibilityLabel("Secondary terminal presentation")
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

    /// Secondary surfaces are presentation only. Let the pane-region view
    /// underneath receive clicks and dispatch the Rust-owned FocusPane action.
    override func hitTest(_ point: NSPoint) -> NSView? {
        nil
    }

    override func layout() {
        super.layout()
        guard displayHandle != 0, window != nil, !isHidden,
              bounds.width > 8, bounds.height > 8 else { return }
        let rounded = bounds.integral
        let cell = surface.terminalPresentationCellSize()
        guard cell.width > 0, cell.height > 0,
              rounded != lastProposedGeometry || cell != lastProposedCellSize else { return }
        // Only point-space viewport and permanent renderer metrics cross the
        // ABI. Rust derives the grid and verifies the Pane → ExecutionId before
        // its own client submits the correlated SPEC-004 resize.
        let result = seyal_app_propose_pane_geometry(
            appHandle,
            paneLo,
            paneHi,
            Double(rounded.width),
            Double(rounded.height),
            0,
            0,
            Double(cell.width),
            Double(cell.height),
            1
        )
        if result == 0 {
            lastProposedGeometry = rounded
            lastProposedCellSize = cell
        }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("SecondaryLivePaneHost is programmatic")
    }

    func updateDisplayHandle(_ handle: UInt64) {
        guard handle != 0, handle != displayHandle else { return }
        displayHandle = handle
        lastProposedGeometry = .null
        lastProposedCellSize = .zero
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
    }
}
