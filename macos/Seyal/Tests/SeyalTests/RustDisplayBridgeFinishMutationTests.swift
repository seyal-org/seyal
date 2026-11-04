import XCTest

@testable import Seyal

/// Issue #1433 adversarial finding — `finishMutation` treated `-18`
/// (BlockMetadataConflict) as warranting a connection teardown, calling
/// `stop(reconnect: true)` on this window's *primary* `clientHandle`
/// whenever the mutation's resolved target (`presentedClientHandle`) fell
/// back to it. That fallback fires whenever `windowFocusedDisplayHandle ==
/// 0`, which is exactly the state right after Split Right while the newly
/// focused leaf has no bound Controller yet.
///
/// Rust's own registry (`is_terminal_registry_loss` in
/// `crates/seyal-client/src/app/session.rs`) never unregisters a client for
/// `BlockMetadataConflict` -- only Io(-3)/NoRunningExecution(-6)/
/// Disconnected(-10) are terminal there. A stray history/block mutation
/// that does not match current Block state is retryable, not evidence the
/// transport died. Tearing down the primary Controller on this benign
/// rejection disconnected a live, untouched sibling Pane and triggered an
/// unbounded disconnect -> recover -> retry storm: Rust's own
/// RecoveryCoordinator reconnected the torn-down primary, and the next
/// reconcile pass hit the exact same false classification again, producing
/// a monotonically increasing registry handle and never draining the
/// create-admission response for the split's new leaf.
///
/// This test calls the real, unmodified `finishMutation` on a `connected`
/// bridge with a fake (unregistered) primary handle. `seyal_bridge_*`
/// FFI entry points fail closed to a documented no-op for an unknown
/// handle, so this exercises `finishMutation`'s own decision logic
/// deterministically, with no Runtime process, no socket, and no timing
/// dependence -- unlike the Split Right XCUIAutomation click test, which
/// could not reliably land inside the real race window.
@MainActor
final class RustDisplayBridgeFinishMutationTests: XCTestCase {
    private func makeConnectedBridge(clientHandle: UInt64) -> RustDisplayBridge {
        let bridge = RustDisplayBridge(
            onFrame: { _ in },
            onError: { _ in }
        )
        bridge.isConnected = true
        bridge.clientHandle = clientHandle
        bridge.handleBox.value = clientHandle
        // windowFocusedDisplayHandle defaults to 0, so presentedClientHandle
        // falls back to clientHandle -- exactly the post-Split-Right state
        // this regression covers (the newly focused leaf has no bound
        // Controller yet).
        return bridge
    }

    func testBlockMetadataConflictDoesNotDisconnectThePrimaryController() {
        let bridge = makeConnectedBridge(clientHandle: 0xFEED_0001)

        let result = bridge.finishMutation(-18)

        XCTAssertEqual(result, -18, "the caller must still see the rejection code")
        XCTAssertTrue(
            bridge.isConnected,
            "BlockMetadataConflict is not one of Rust's is_terminal_registry_loss "
                + "codes (-3/-6/-10 only) and must not disconnect the primary Controller"
        )
        XCTAssertEqual(
            bridge.clientHandle, 0xFEED_0001,
            "the primary clientHandle must survive a benign block-metadata rejection "
                + "(this is what the #1433 reconnect storm tore down on every reconcile pass)"
        )
    }

    func testIoAndDisconnectedStillDisconnectThePrimaryController() {
        for code: Int32 in [-3, -10] {
            let bridge = makeConnectedBridge(clientHandle: 0xFEED_0002)

            _ = bridge.finishMutation(code)

            XCTAssertFalse(
                bridge.isConnected,
                "code \(code) is one of Rust's genuinely terminal registry-loss codes "
                    + "and must still disconnect the primary Controller"
            )
            XCTAssertEqual(
                bridge.clientHandle, 0,
                "a genuinely terminal mutation result must still drop the handle"
            )
        }
    }

    func testTerminalLossOnHistorySiblingHandleLeavesPrimaryConnected() {
        let bridge = makeConnectedBridge(clientHandle: 0xFEED_0001)
        let result = bridge.finishMutation(-10, targetHandle: 0xFEED_00AA)
        XCTAssertEqual(result, -10)
        XCTAssertTrue(
            bridge.isConnected,
            "a history mutation aimed at a sibling Controller must not stop this window"
        )
        XCTAssertEqual(bridge.clientHandle, 0xFEED_0001)
    }
}
