import XCTest

@testable import Seyal

/// #1433 — `armAuxiliaryReadSources`/`drainReadyDisplayWork` used to treat
/// *any* negative `seyal_bridge_poll`/`seyal_bridge_poll_for` result as a
/// dead Controller and disconnect it, even though Rust's own registry
/// (`is_terminal_client_loss` in `crates/seyal-client/src/ffi/display.rs`)
/// only unregisters the client for `Io`/`NoRunningExecution`/`Disconnected`.
/// A transient `Protocol`/`ClientBackpressure`/`CommitTooLarge`/
/// `BlockMetadataConflict`/`Server(_)` code on a live sibling (e.g. right
/// after Split Right, while the new leaf's attach is still in flight) tore
/// the sibling's connection down over a benign hiccup Rust itself did not
/// consider fatal. `isTerminalPollResult` must mirror Rust's classification
/// exactly: only `-3` (Io), `-6` (NoRunningExecution / shell exit), and
/// `-10` (Disconnected) are terminal.
final class RustDisplayBridgePollClassificationTests: XCTestCase {
    func testOnlyRustsTerminalClientLossCodesAreTerminal() {
        for code: Int32 in [-3, -6, -10] {
            XCTAssertTrue(
                RustDisplayBridge.isTerminalPollResult(code),
                "code \(code) is a Rust is_terminal_client_loss code and must disconnect"
            )
        }
    }

    func testNonTerminalErrorCodesAreNotTreatedAsFatal() {
        // -2 Discovery, -4 Protocol, -5 UnsupportedDisplayCapability,
        // -7 InvalidAttachment, -8 Display, -9 Prepare, -11 Capacity,
        // -12 UnsupportedInteractiveCapability, -13 ClientBackpressure,
        // -14 CommitTooLarge, -15 LostController, -16 ResizeProtocolFailure,
        // -17 InvalidGeometry, -18 BlockMetadataConflict, -19
        // AmbiguousExecutions, -20 StartupDeadlineExceeded, and a
        // `Server(code)` diagnostic (<= -1000) — none of these cause Rust to
        // unregister the client (`error_code` in
        // `crates/seyal-client/src/ffi/errors.rs`).
        let nonTerminal: [Int32] = [
            -2, -4, -5, -7, -8, -9, -11, -12, -13, -14, -15, -16, -17, -18, -19, -20, -1000, -1255,
        ]
        for code in nonTerminal {
            XCTAssertFalse(
                RustDisplayBridge.isTerminalPollResult(code),
                "code \(code) is not one of Rust's is_terminal_client_loss codes and must not disconnect"
            )
        }
    }

    func testSuccessCodesAreNotTerminal() {
        XCTAssertFalse(RustDisplayBridge.isTerminalPollResult(0))
        XCTAssertFalse(RustDisplayBridge.isTerminalPollResult(1))
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
