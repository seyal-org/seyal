import AppKit

/// One-pane AppKit host. Product state is Rust-owned (`seyal_app_*`).
/// Terminal frames stay on the Candidate-D bridge (`seyal_bridge_*`).
@MainActor
final class ThinPaneHostView: NSView {
    let inputSurface: InteractiveMetalSurfaceView
    let appHandle: UInt64
    var onProductChanged: (() -> Void)?
    private var lastBoundExecution = (low: UInt64(0), high: UInt64(0))

    override init(frame frameRect: NSRect) {
        appHandle = seyal_app_create()
        inputSurface = InteractiveMetalSurfaceView(
            frame: frameRect,
            appHandle: appHandle
        )
        super.init(frame: frameRect)
        translatesAutoresizingMaskIntoConstraints = false
        setAccessibilityElement(true)
        setAccessibilityRole(.group)
        setAccessibilityIdentifier("seyal-thin-pane")
        inputSurface.translatesAutoresizingMaskIntoConstraints = false
        addSubview(inputSurface)
        NSLayoutConstraint.activate([
            inputSurface.leadingAnchor.constraint(equalTo: leadingAnchor),
            inputSurface.trailingAnchor.constraint(equalTo: trailingAnchor),
            inputSurface.topAnchor.constraint(equalTo: topAnchor),
            inputSurface.bottomAnchor.constraint(equalTo: bottomAnchor),
        ])
        inputSurface.onAlternateScreenChanged = { [weak self] alternate in
            self?.inputSurface.observedAlternateScreen = alternate
            self?.bindFromBridgeIfNeeded()
            self?.refreshAlternateScreen(alternate)
            self?.onProductChanged?()
        }
        inputSurface.onFrameChanged = { [weak self] _ in
            self?.bindFromBridgeIfNeeded()
            self?.onProductChanged?()
        }
        inputSurface.onBridgeBecameUsable = { [weak self] in
            self?.bindFromBridgeIfNeeded()
            self?.onProductChanged?()
        }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("ThinPaneHostView is programmatic")
    }

    deinit {
        seyal_app_destroy(appHandle)
    }

    override func hitTest(_ point: NSPoint) -> NSView? {
        let hit = super.hitTest(point)
        // Flow paints terminal pixels over Block bodies. Metal returns nil from
        // hitTest, but this pane still sits above the transcript. Default
        // NSView hit-testing would then return `self` and swallow the click
        // so CommandBlockView never sees it. Fall through in Flow; keep the
        // surface in Raw/TUI where drawsLiveGrid is true.
        if hit === self, !inputSurface.inspectRendererPresentation().drawsLiveGrid {
            return nil
        }
        return hit
    }

    func activateAfterWindowPresentation() {
        inputSurface.activateRuntimeAfterWindowPresentation()
        bindFromBridgeIfNeeded()
        announceAccessibility()
    }

    func requestQuit() {
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_QUIT.rawValue)
        _ = seyal_app_apply(appHandle, &action)
        let snapshot = seyal_app_snapshot(appHandle)
        if snapshot.pending_effect != 0 {
            var ack = SeyalAppAction()
            ack.version = UInt16(SEYAL_APP_ABI_VERSION)
            ack.size = UInt16(MemoryLayout<SeyalAppAction>.size)
            ack.kind = UInt16(SEYAL_APP_ACTION_ACK_EFFECT.rawValue)
            _ = seyal_app_apply(appHandle, &ack)
        }
    }

    func detachForTermination() {
        inputSurface.detachRuntimeConnectionForApplicationTermination()
    }

    private func bindFromBridgeIfNeeded() {
        guard inputSurface.terminalBridgeIsConnected else { return }
        let executionLow = seyal_bridge_execution_id_low()
        let executionHigh = seyal_bridge_execution_id_high()
        let attachmentLow = seyal_bridge_attachment_id_low()
        let attachmentHigh = seyal_bridge_attachment_id_high()
        guard executionLow != 0 || executionHigh != 0 else { return }
        if lastBoundExecution == (executionLow, executionHigh) { return }

        let snapshot = seyal_app_snapshot(appHandle)
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_BIND.rawValue)
        action.flags = UInt16(SEYAL_APP_FLAG_TARGET_CONTROLLER)
        if inputSurface.observedAlternateScreen {
            action.flags |= UInt16(SEYAL_APP_FLAG_ALTERNATE_SCREEN)
        }
        action.fence_pane_lo = snapshot.pane_lo
        action.fence_pane_hi = snapshot.pane_hi
        action.fence_epoch = snapshot.epoch
        action.target_execution_lo = executionLow
        action.target_execution_hi = executionHigh
        action.target_attachment_lo = attachmentLow
        action.target_attachment_hi = attachmentHigh
        action.target_pty_generation = 1
        guard seyal_app_apply(appHandle, &action) == 0 else { return }
        lastBoundExecution = (executionLow, executionHigh)
        announceAccessibility()
    }

    /// Candidate-D already observed alternate-screen; Bind only samples it once.
    /// Refresh is the post-bind presentation fence (ADR-009 / M001 TUI takeover).
    private func refreshAlternateScreen(_ alternate: Bool) {
        let snapshot = seyal_app_snapshot(appHandle)
        guard snapshot.flags & UInt16(SEYAL_APP_SNAP_HAS_EXECUTION) != 0 else { return }
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_REFRESH.rawValue)
        action.applySnapshotFence(snapshot)
        if alternate {
            action.flags |= UInt16(SEYAL_APP_FLAG_ALTERNATE_SCREEN)
        }
        _ = seyal_app_apply(appHandle, &action)
    }

    private func announceAccessibility() {
        let tree = seyal_app_accessibility(appHandle)
        guard tree.node_count > 0, let nodes = tree.nodes else { return }
        let node = nodes.pointee
        guard node.label_len > 0, let labelBytes = node.label else { return }
        let label = String(
            decoding: UnsafeBufferPointer(start: labelBytes, count: Int(node.label_len)),
            as: UTF8.self
        )
        SeyalAccessibilityAnnouncement.post(label, element: self)
    }
}

extension SeyalAppAction {
    /// Copy snapshot identity into the action fence. Snapshot flags are not
    /// action flags; SNAP_* must be translated to FLAG_*.
    mutating func applySnapshotFence(_ snapshot: SeyalAppSnapshot) {
        fence_pane_lo = snapshot.pane_lo
        fence_pane_hi = snapshot.pane_hi
        fence_execution_lo = snapshot.execution_lo
        fence_execution_hi = snapshot.execution_hi
        fence_attachment_lo = snapshot.attachment_lo
        fence_attachment_hi = snapshot.attachment_hi
        fence_epoch = snapshot.epoch
        if snapshot.flags & UInt16(SEYAL_APP_SNAP_HAS_EXECUTION) != 0 {
            flags |= UInt16(SEYAL_APP_FLAG_HAS_EXECUTION)
        }
        if snapshot.flags & UInt16(SEYAL_APP_SNAP_HAS_ATTACHMENT) != 0 {
            flags |= UInt16(SEYAL_APP_FLAG_HAS_ATTACHMENT)
        }
        if snapshot.flags & UInt16(SEYAL_APP_SNAP_CONTROLLER) != 0 {
            flags |= UInt16(SEYAL_APP_FLAG_CONTROLLER)
        }
    }
}

