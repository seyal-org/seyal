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
        var bindings: [(paneLo: UInt64, paneHi: UInt64, displayHandle: UInt64, focused: Bool, live: Bool, recoveryTarget: SeyalAppPaneRecoveryTarget)] = []
        for region in centerColumn.regions {
            let binding = seyal_app_pane_binding(handle, UInt32(bindings.count))
            bindings.append(
                (
                    paneLo: region.paneLo,
                    paneHi: region.paneHi,
                    displayHandle: binding.display_handle,
                    focused: region.focused,
                    live: region.live,
                    recoveryTarget: region.recoveryTarget
                )
            )
        }
        centerColumn.onRetryPaneRecovery = { [weak self] target in
            guard let self,
                target.flags == UInt16(SEYAL_APP_PANE_RECOVERY_RETRYABLE),
                seyal_app_retry_pane_recovery(self.pane.appHandle, target) == 0
            else { return }
            self.driveRecovery()
            self.reconcilePaneHosts(paneCount: Int(seyal_app_shell(self.pane.appHandle).pane_count))
        }
        centerColumn.reconcileSecondaryLiveHosts(bindings: bindings)
        let focusedDisplayHandle =
            bindings.first { $0.focused && $0.displayHandle != 0 }?.displayHandle ?? 0
        wireRegistryFrameRouting(focusedDisplayHandle: focusedDisplayHandle)
    }

    /// Primary bridge polls every Controller; secondary hosts present via
    /// `seyal_bridge_frame_for` without stealing keyboard focus.
    func wireRegistryFrameRouting(focusedDisplayHandle: UInt64) {
        guard let bridge = pane.inputSurface.bridge else { return }
        bridge.onRegistryHandlePolled = { [weak self] displayHandle in
            self?.centerColumn.secondaryHost(displayHandle: displayHandle)?.noteRegistryPolled()
        }
        guard bridge.windowFocusedDisplayHandle != focusedDisplayHandle else { return }
        bridge.windowFocusedDisplayHandle = focusedDisplayHandle
        bridge.publishCurrentFrame()
        bridge.refreshPresentedTerminalTimeline()
    }
}
