import XCTest

@testable import Seyal

/// #1433 — flush/write paths use a narrow socket-loss set; poll paths use a
/// wider set that includes Protocol/`-4` and BlockMetadataConflict/`-18`
/// (SPEC-007 §12: those frames are never consumed, so Rust unregisters).
final class RustDisplayBridgePollClassificationTests: XCTestCase {
    func testFlushPathOnlyTreatsSocketLossAsTerminal() {
        for code: Int32 in [-3, -6, -10] {
            XCTAssertTrue(
                RustDisplayBridge.isTerminalPollResult(code),
                "code \(code) means the mutation target's socket is gone"
            )
        }
        // Keep Protocol/BlockMetadataConflict off the flush path so sibling
        // write handlers cannot window-wide stop() on -4.
        for code: Int32 in [-4, -18] {
            XCTAssertFalse(
                RustDisplayBridge.isTerminalPollResult(code),
                "code \(code) must not widen flush-path stop()"
            )
        }
    }

    func testPollPathClosesOnUnconsumedFatalFrames() {
        for code: Int32 in [-3, -4, -6, -10, -18] {
            XCTAssertTrue(
                RustDisplayBridge.pollResultClosesConnection(code),
                "code \(code) must disarm that handle's read source"
            )
        }
        let nonClosing: [Int32] = [
            -2, -5, -7, -8, -9, -11, -12, -13, -14, -15, -16, -17, -19, -20, -1000, -1255,
        ]
        for code in nonClosing {
            XCTAssertFalse(
                RustDisplayBridge.pollResultClosesConnection(code),
                "code \(code) must leave the fd armed for retry"
            )
        }
    }

    func testSuccessCodesAreNotTerminal() {
        XCTAssertFalse(RustDisplayBridge.isTerminalPollResult(0))
        XCTAssertFalse(RustDisplayBridge.isTerminalPollResult(1))
        XCTAssertFalse(RustDisplayBridge.pollResultClosesConnection(0))
        XCTAssertFalse(RustDisplayBridge.pollResultClosesConnection(1))
    }
}

/// Adjacent regression, found by adversarial testing after this fix landed:
/// `finishMutation` (the shared terminal-code handler for every
/// `submit*`/`retryResize` mutation in `RustDisplayBridge+Input.swift` and
/// `RustDisplayBridge+History.swift`) called the window-wide `stop()`
/// unconditionally for `-3`/`-10`/`-18`, regardless of which Controller the
/// mutation that produced that code actually targeted. Every mutation routes
/// through `selectPresentedClient()`, which selects `windowFocusedDisplayHandle`
/// (a focused *sibling* Pane) when one is bound, not necessarily this
/// bridge's own `clientHandle`. `stop()` cancels every auxiliary read/write
/// source via `cancelAuxiliaryReadSources()` and tears down the primary
/// connection too, so a genuinely terminal code on one Pane's own mutation
/// (e.g. that Pane's shell actually exiting) cascaded into disconnecting
/// Panes the user never touched — surfacing as "Waiting to reconnect" on a
/// just-split sibling Pane and a sustained Rust-side pane-recovery retry
/// storm (`ProductChromeHostView.driveRecovery()` /
/// `seyal_app_begin_next_pane_recovery` spinning at ~100% CPU on the main
/// thread), since each repair attempt re-triggered the same window-wide
/// teardown.
///
/// `finishMutation` must scope teardown to `presentedClientHandle` — the
/// exact handle `selectPresentedClient()` selected for that mutation — and
/// only fall through to the window-wide `stop()` when that handle is this
/// bridge's own primary `clientHandle`.
final class RustDisplayBridgePresentedClientHandleTests: XCTestCase {
    @MainActor
    private func makeBridge() -> RustDisplayBridge {
        RustDisplayBridge(
            onFrame: { _ in },
            onError: { _ in }
        )
    }

    @MainActor
    func testPresentedClientHandleFallsBackToPrimaryWhenNoSiblingFocused() {
        let bridge = makeBridge()
        bridge.clientHandle = 11
        bridge.windowFocusedDisplayHandle = 0
        XCTAssertEqual(
            bridge.presentedClientHandle, 11,
            "with no focused sibling, a mutation must still target this bridge's own primary Controller"
        )
    }

    @MainActor
    func testPresentedClientHandleTargetsFocusedSiblingOverPrimary() {
        let bridge = makeBridge()
        bridge.clientHandle = 11
        bridge.windowFocusedDisplayHandle = 22
        XCTAssertEqual(
            bridge.presentedClientHandle, 22,
            "a focused sibling Pane's own handle must be the mutation target, not this bridge's primary Controller"
        )
        XCTAssertNotEqual(
            bridge.presentedClientHandle, bridge.clientHandle,
            "finishMutation's handle-scoped branch depends on these being distinguishable"
        )
    }
}
