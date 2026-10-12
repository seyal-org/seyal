import AppKit

@MainActor
extension ProductChromeHostView {
    func reconcilePaneHosts(paneCount: Int) {
        centerColumn.reconcile(paneCount: paneCount)
        reconcileSecondaryLiveSurfaces(paneCount: paneCount)
    }

    /// Project secondary Metal hosts for non-focused LIVE leaves (#936).
    func reconcileSecondaryLiveSurfaces(paneCount: Int) {
        let handle = pane.appHandle
        var bindings: [(paneLo: UInt64, paneHi: UInt64, displayHandle: UInt64, focused: Bool, live: Bool)] = []
        for index in 0..<paneCount {
            let region = seyal_app_pane_region(handle, UInt32(index))
            guard region.size != 0 else { break }
            let binding = seyal_app_pane_binding(handle, UInt32(index))
            bindings.append(
                (
                    paneLo: region.pane_lo,
                    paneHi: region.pane_hi,
                    displayHandle: binding.display_handle,
                    focused: region.flags & UInt16(SEYAL_APP_PANE_REGION_FOCUSED) != 0,
                    live: region.flags & UInt16(SEYAL_APP_PANE_REGION_LIVE) != 0
                )
            )
        }
        centerColumn.reconcileSecondaryLiveHosts(bindings: bindings)
        wireRegistryFrameRouting()
    }

    /// Primary bridge polls every Controller; secondary hosts present via
    /// `seyal_bridge_frame_for` without stealing keyboard focus.
    func wireRegistryFrameRouting() {
        guard let bridge = pane.inputSurface.bridge else { return }
        bridge.onRegistryHandlePolled = { [weak self] displayHandle in
            self?.centerColumn.secondaryHost(displayHandle: displayHandle)?.noteRegistryPolled()
        }
    }
}
