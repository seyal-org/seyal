import AppKit
import Foundation

@MainActor
extension ProductChromeHostView {
    /// Apply a Runtime ComposerResult to the Pane that owns the matching
    /// pending request. Multipane must not fan-out through focused fences —
    /// that stole focus via `prepareFocusedMutation` after the first sibling
    /// submit and dropped pane-local markers (#1474 / #1424).
    func distributeComposerResult(
        requestID: UInt64,
        accepted: Bool,
        failureCode: NativeComposerResult.Code?,
        sourceDisplayHandle: UInt64 = 0,
        blockID: UInt64 = 0
    ) {
        if centerColumn.secondaryHosts.isEmpty {
            composer.applyComposerResult(requestID: requestID, accepted: accepted)
            if let failureCode {
                composer.presentAdmissionFailure(failureCode)
            }
            return
        }
        // Retire the legacy dock from AX while pane hosts own Flow so XCUI and
        // hit-testing cannot submit through a hidden competing `seyal-composer`.
        composer.retirePrimaryAccessibilityIdentity()
        let appHandle = pane.appHandle
        // Bind Accept to the Controller that produced it. Never fall through to
        // a sibling that happens to share the same pending request_id (#1474
        // pane-local tabs: Pane 2's Accept must not clear Pane 1's draft).
        let hosts: [SecondaryLivePaneHost]
        if sourceDisplayHandle != 0 {
            guard let owner = centerColumn.secondaryHost(displayHandle: sourceDisplayHandle)
                ?? centerColumn.secondaryHosts.first(where: {
                    $0.liveBindingDisplayHandleForRegistry() == sourceDisplayHandle
                })
            else { return }
            if owner.displayHandle != sourceDisplayHandle {
                _ = owner.updateDisplayHandle(sourceDisplayHandle)
            }
            hosts = [owner]
        } else {
            hosts = centerColumn.secondaryHosts
        }
        for host in hosts {
            let snap = seyal_app_composer_for_pane(appHandle, host.paneLo, host.paneHi)
            guard snap.request_id == requestID, requestID != 0 else { continue }
            let applied = seyal_app_apply_composer_result_for_pane(
                appHandle,
                host.paneLo,
                host.paneHi,
                requestID,
                accepted ? 1 : 0
            )
            guard applied == 0 else { continue }
            host.composer.reconcile()
            // Accept can land before the next registry poll republishes the
            // Candidate-D timeline. Force a pane-local sync from the producing
            // Controller so XCUI sees the new Block without waiting on PTY wake
            // (#1474).
            if accepted {
                syncAcceptedComposerBlocks(
                    host: host,
                    appHandle: appHandle,
                    sourceDisplayHandle: sourceDisplayHandle,
                    requestID: requestID,
                    blockID: blockID
                )
            }
            if let failureCode {
                host.composer.presentAdmissionFailure(failureCode)
            }
            return
        }
    }

    /// Drain the accepting Controller until Candidate-D records exist, then
    /// project them into the Pane. Never apply an empty timeline while waiting
    /// for trusted C after Accept (#1474 pane-local tabs).
    private func syncAcceptedComposerBlocks(
        host: SecondaryLivePaneHost,
        appHandle: UInt64,
        sourceDisplayHandle: UInt64,
        requestID: UInt64,
        blockID: UInt64
    ) {
        // Prefer the selected terminal Controller. Accept is pinned to the
        // submit handle; never sync an unselected tab's timeline into this
        // Pane while another tab is selected (#1474).
        let live = host.liveBindingDisplayHandleForRegistry()
        let client: UInt64 = {
            if sourceDisplayHandle != 0, sourceDisplayHandle == live || live == 0 {
                return sourceDisplayHandle
            }
            if live != 0 { return live }
            return host.displayHandle
        }()
        host.invalidateProjectionCache()
        // BlockTimeline publishes Running only after trusted C. Accept can beat
        // that frame — do not apply an empty replacement and wipe a sibling
        // tab's cards while waiting (#1474).
        if blockID != 0 {
            var after = seyal_app_composer_for_pane(appHandle, host.paneLo, host.paneHi)
            if client != 0 {
                var turns = 0
                while after.block_count == 0, turns < 48 {
                    let polled = seyal_bridge_poll_for(client)
                    let clientBlocks = seyal_bridge_block_count_for(client)
                    if clientBlocks > 0 {
                        host.invalidateProjectionCache()
                        host.noteRegistryPolled(clientHandle: client)
                        after = seyal_app_composer_for_pane(
                            appHandle, host.paneLo, host.paneHi
                        )
                        if after.block_count > 0 { break }
                    }
                    turns += 1
                    if polled < 0 { break }
                    if polled == 0 {
                        Thread.sleep(forTimeInterval: 0.01)
                    }
                }
            }
            if after.block_count == 0 {
                let clientBlocks = client == 0 ? 0 : seyal_bridge_block_count_for(client)
                host.blocks.setAccessibilityLabel(
                    "sync-missed-blocks request=\(requestID) block=\(blockID) "
                        + "handle=\(client) client-blocks=\(clientBlocks)"
                )
            } else {
                host.blocks.setAccessibilityLabel(nil)
            }
        } else {
            if client != 0 {
                host.noteRegistryPolled(clientHandle: client)
            }
            host.blocks.setAccessibilityLabel(
                "relay-accept request=\(requestID) handle=\(client)"
            )
        }
    }

    /// Restore single-pane dock AX when multipane hosts collapse.
    func restoreLegacyComposerAccessibilityIfNeeded() {
        guard centerColumn.secondaryHosts.isEmpty else {
            composer.retirePrimaryAccessibilityIdentity()
            return
        }
        composer.restorePrimaryAccessibilityIdentity()
    }
}
