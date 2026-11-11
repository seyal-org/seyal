import AppKit

private struct PaneBindingIdentity: Hashable {
    let lo: UInt64
    let hi: UInt64
}

@MainActor
extension ProductChromeHostView {
    /// Join shell bindings to visual regions by stable PaneId. Ambiguity fails closed.
    static func bindingsInRegionOrder(
        regions: [PaneLayoutView.Region],
        shellBindings: [SeyalAppPaneBinding]
    ) -> [SeyalAppPaneBinding?] {
        let byPane = Dictionary(grouping: shellBindings) {
            PaneBindingIdentity(lo: $0.pane_lo, hi: $0.pane_hi)
        }
        return regions.map {
            let matches = byPane[PaneBindingIdentity(lo: $0.paneLo, hi: $0.paneHi)]
            return matches?.count == 1 ? matches?.first : nil
        }
    }

    func reconcilePaneHosts(paneCount: Int) {
        centerColumn.reconcile(paneCount: paneCount)
        reconcileSecondaryLiveSurfaces(paneCount: paneCount)
    }

    /// Project durable multipane Flow hosts (#936 / #1424).
    ///
    /// `reconcileSecondaryLiveHosts` applies focus/input ownership. Host
    /// `reconcileFlowPresentation` rebuilds Blocks only when that pane's
    /// projection fingerprint changes — focus churn must not recreate views.
    func reconcileSecondaryLiveSurfaces(paneCount: Int) {
        let handle = pane.appHandle
        let shell = seyal_app_shell(handle)
        let shellBindings = (0..<Int(shell.pane_count)).map { index in
            seyal_app_pane_binding(handle, UInt32(index))
        }
        let bindingsInRegionOrder = Self.bindingsInRegionOrder(
            regions: centerColumn.regions,
            shellBindings: shellBindings
        )
        var bindings: [(paneLo: UInt64, paneHi: UInt64, displayHandle: UInt64, focused: Bool, live: Bool, recoveryTarget: SeyalAppPaneRecoveryTarget)] = []
        for (region, binding) in zip(centerColumn.regions, bindingsInRegionOrder) {
            guard let binding else { continue }
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
        let sharedBridge = pane.inputSurface.bridge
        let focusedDisplayHandle =
            bindings.first { $0.focused && $0.displayHandle != 0 }?.displayHandle ?? 0
        // Publish window focus before leaf DirectInput sync so Metal
        // `allowsDirectTerminalKeys` sees the matching Controller handle.
        wireRegistryFrameRouting(focusedDisplayHandle: focusedDisplayHandle)
        centerColumn.reconcileSecondaryLiveHosts(
            bindings: bindings,
            historyBridge: sharedBridge
        )
        for host in centerColumn.secondaryHosts {
            // Bridgeless Metal surfaces borrow the window bridge for history.
            host.historyBridge = sharedBridge
            // Focus/binding reconcile must not rebuild Blocks; content updates
            // arrive through noteRegistryPolled → reconcileFlowPresentation.
            host.reconcileFocusOwnershipPresentation()
        }
        // Multipane Flow: only leaf hosts may advertise `seyal-composer`. The
        // hidden liveContent dock kept the same AX id and stole XCUI submit
        // after optimistic accept was removed (#1474 / #1424).
        restoreLegacyComposerAccessibilityIfNeeded()
        // After leaf ownership + forced composer status republish, reclaim Metal
        // FR for Block-scoped DirectInput (generation-stable status alone will
        // not call performChromeReconcile's eligibility routeFocus).
        routeFocus()
    }

    /// Primary bridge polls every Controller; secondary hosts present via
    /// `seyal_bridge_frame_for` without stealing keyboard focus.
    func wireRegistryFrameRouting(focusedDisplayHandle: UInt64) {
        guard let bridge = pane.inputSurface.bridge else { return }
        bridge.onRegistryHandlePolled = { [weak self] displayHandle in
            guard let self else { return }
            if let host = self.centerColumn.secondaryHost(displayHandle: displayHandle) {
                host.noteRegistryPolled(clientHandle: displayHandle)
                return
            }
            // Tab switches can leave host.displayHandle briefly stale while the
            // shell binding already names the polled Controller (#1474).
            for host in self.centerColumn.secondaryHosts
            where host.liveBindingDisplayHandleForRegistry() == displayHandle {
                // updateDisplayHandle syncs from the new Controller.
                _ = host.updateDisplayHandle(displayHandle)
                return
            }
        }
        guard bridge.windowFocusedDisplayHandle != focusedDisplayHandle else { return }
        bridge.windowFocusedDisplayHandle = focusedDisplayHandle
        bridge.publishCurrentFrame()
        bridge.refreshPresentedTerminalTimeline()
        // Quiet REPLs produce no poll after FOCUS_PANE. Force this Controller's
        // composer eligibility/target back into ApplicationRoot so Block-scoped
        // DirectInput survives a sibling Available clearing the shared target
        // (#1467 / #1468 multipane).
        bridge.republishFocusedComposerStatus()
    }
}
