import AppKit
import XCTest

@testable import Seyal

/// Keeps XCTest alive when W4a tests order out / release Seyal windows.
@MainActor
private final class MultiWindowTestAppDelegate: NSObject, NSApplicationDelegate {
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        false
    }
}

@MainActor
final class MultiWindowHostTests: XCTestCase {
    private static let terminateGuard = MultiWindowTestAppDelegate()

    override func setUp() {
        super.setUp()
        MainActor.assumeIsolated {
            if NSApp.delegate == nil {
                NSApp.delegate = Self.terminateGuard
            }
        }
        let missing = FileManager.default.temporaryDirectory
            .appendingPathComponent("seyal-test-missing-\(UUID().uuidString).toml")
        XCTAssertEqual(reloadUiConfigForMultiWindowTests(path: missing.path), 0)
        NativeThemeRealization.resetColdDiagnosticsSurfacedForTests()
    }

    func testSeedWindowsOnlyInstallsSnapshotWindows() {
        let handle = seyal_app_create()
        defer { _ = seyal_app_destroy(handle) }
        // Headed XCTest shares the process CLIENTS TLS with the live AppDelegate
        // host; probe-client registration is covered by Rust
        // `install_quit_fixture_attaches_probe_clients` instead.
        XCTAssertEqual(seyal_app_test_seed_windows_only(handle, 3), 0)
        XCTAssertEqual(seyal_app_shell(handle).window_count, 3)
    }

    func testTerminateLaterRepliesOnceOnCleanupComplete() {
        let host = seededQuitHost(windows: 3)
        defer { host.performQuitCleanup() }
        XCTAssertEqual(host.orderedKeys.count, 3)
        XCTAssertEqual(
            host.realizedTabbingModes(),
            [.disallowed, .disallowed, .disallowed]
        )
        // Class box: escaping reply must not mutate a stack `var` under Swift
        // exclusivity while XCTest pumps the main queue (process abort).
        let replies = QuitReplyCounter()
        let reply = host.quitCoordinatorForTests.beginTerminateLater(
            forwardRequestQuit: { host.forwardRequestQuit() },
            performNativeCleanup: { host.performQuitCleanup() },
            ackUntilCleanupComplete: { host.ackUntilQuitCleanupComplete() },
            reply: { replies.increment() }
        )
        XCTAssertEqual(reply, .terminateLater)
        XCTAssertEqual(host.quitCoordinatorForTests.armedDeadlineMs, 500)
        XCTAssertTrue(host.quitCoordinatorForTests.didArmBackstop)
        XCTAssertEqual(replies.count, 1)
        XCTAssertEqual(host.quitCoordinatorForTests.replyCount, 1)
        XCTAssertTrue(host.orderedKeys.isEmpty)
        // Late cleanup after the first reply is ignored (ADR-018 §4).
        host.quitCoordinatorForTests.signalCleanupComplete()
        XCTAssertEqual(replies.count, 1)
    }

    func testBackstopRepliesOnceWhenCleanupNeverCompletes() {
        let host = MultiWindowHostController()
        defer { host.performQuitCleanup() }
        host.bootstrapAfterLaunch()
        let replies = QuitReplyCounter()
        let fired = expectation(description: "backstop reply")
        let reply = host.quitCoordinatorForTests.beginTerminateLater(
            forwardRequestQuit: { host.forwardRequestQuit() },
            performNativeCleanup: {},
            ackUntilCleanupComplete: { false },
            reply: {
                replies.increment()
                fired.fulfill()
            }
        )
        XCTAssertEqual(reply, .terminateLater)
        XCTAssertEqual(replies.count, 0)
        XCTAssertEqual(host.quitCoordinatorForTests.armedDeadlineMs, 500)
        XCTAssertTrue(host.quitCoordinatorForTests.didArmBackstop)
        wait(for: [fired], timeout: 2.0)
        XCTAssertEqual(replies.count, 1)
        host.quitCoordinatorForTests.signalCleanupComplete()
        XCTAssertEqual(replies.count, 1)
    }

    func testForwardingFailureRepliesImmediately() {
        var replies = 0
        let coordinator = ApplicationQuitCoordinator()
        let reply = coordinator.beginTerminateLater(
            forwardRequestQuit: { .failure(QuitForwardError.missingDeadline) },
            performNativeCleanup: {},
            ackUntilCleanupComplete: { false },
            reply: { replies += 1 }
        )
        XCTAssertEqual(reply, .terminateLater)
        XCTAssertEqual(replies, 1)
        XCTAssertFalse(coordinator.didArmBackstop)
        XCTAssertEqual(coordinator.armedDeadlineMs, 0)
        coordinator.signalCleanupComplete()
        XCTAssertEqual(replies, 1)
    }

    func testBootstrapRealizesOneWindowWithTabbingDisallowed() {
        let host = MultiWindowHostController()
        defer { host.performQuitCleanup() }
        host.bootstrapAfterLaunch()
        XCTAssertEqual(host.orderedKeys.count, 1)
        XCTAssertEqual(host.realizedTabbingModes(), [.disallowed])
        XCTAssertEqual(host.snapshotOrderedWindowKeys().count, 1)
        XCTAssertEqual(host.orderedKeys, host.snapshotOrderedWindowKeys())
    }

    func testUIOrderFollowsSnapshotNotNSAppWindows() {
        let host = MultiWindowHostController()
        defer { host.performQuitCleanup() }
        host.bootstrapAfterLaunch()
        createExtraWindows(on: host, count: 2)
        let snapshotOrder = host.snapshotOrderedWindowKeys()
        XCTAssertEqual(snapshotOrder.count, 1)
        XCTAssertEqual(host.orderedKeys, snapshotOrder)
        let source = try! String(
            contentsOf: URL(fileURLWithPath: #filePath)
                .deletingLastPathComponent()
                .deletingLastPathComponent()
                .deletingLastPathComponent()
                .appendingPathComponent("Sources/MultiWindowHostController.swift"),
            encoding: .utf8
        )
        XCTAssertFalse(
            source.contains("NSApp.windows as"),
            "host must not consult AppKit window list as order authority"
        )
        XCTAssertTrue(source.contains("snapshotOrderedWindowKeys"))
    }

    func testDestroyRealizationRemovesWindow() {
        let host = MultiWindowHostController()
        defer { host.performQuitCleanup() }
        host.bootstrapAfterLaunch()
        createExtraWindows(on: host, count: 1)
        XCTAssertEqual(host.orderedKeys.count, 1)
        // Destroy-realization: quit cleanup applies destroy for every WindowId.
        host.performQuitCleanup()
        XCTAssertTrue(host.orderedKeys.isEmpty)
    }

    func testEventForwardingReportsBecameKey() {
        let host = MultiWindowHostController()
        defer { host.performQuitCleanup() }
        host.bootstrapAfterLaunch()
        guard let key = host.orderedKeys.first else {
            return XCTFail("expected bootstrap window")
        }
        // Drive the typed event path directly (same as NSWindowDelegate).
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_REPORT_WINDOW_EVENT.rawValue)
        action.target_execution_lo = key.lo
        action.target_execution_hi = key.hi
        action.reserved = SEYAL_APP_WINDOW_EVENT_BECAME_KEY
        XCTAssertEqual(seyal_app_apply(host.appHandle, &action), 0)
        // Host derives no product state: window count unchanged.
        XCTAssertEqual(seyal_app_shell(host.appHandle).window_count, 1)
    }

    func testHostHasNoWritableWindowModelTypes() {
        let sourceRoot = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .appendingPathComponent("Sources", isDirectory: true)
        let enumerator = FileManager.default.enumerator(
            at: sourceRoot,
            includingPropertiesForKeys: nil
        )!
        var hits: [String] = []
        let forbidden = [
            "var windows: [WindowId",
            "var tabs: [TabId",
            "class SeyalWindowModel",
            "class WritableWindowStore",
            "keyDown(",
        ]
        for case let file as URL in enumerator where file.pathExtension == "swift" {
            let name = file.lastPathComponent
            // Input surfaces may implement keyDown for terminal bytes; product
            // window shortcuts must not.
            if name.contains("Metal") || name.contains("Composer") || name.contains("Interactive")
                || name.contains("CommandPalette") || name.contains("History")
            {
                continue
            }
            let text = (try? String(contentsOf: file, encoding: .utf8)) ?? ""
            if forbidden.contains(where: { text.contains($0) }) {
                hits.append(name)
            }
        }
        XCTAssertTrue(hits.isEmpty, "writable model or keyDown interception in \(hits)")
    }

    func testMenuKeyEquivalentsNotKeyDown() {
        let source = try! String(
            contentsOf: URL(fileURLWithPath: #filePath)
                .deletingLastPathComponent()
                .deletingLastPathComponent()
                .deletingLastPathComponent()
                .appendingPathComponent("Sources/AppDelegate.swift"),
            encoding: .utf8
        )
        XCTAssertTrue(source.contains("keyEquivalent: \"n\""))
        XCTAssertTrue(source.contains("keyEquivalent: \"t\""))
        XCTAssertTrue(source.contains("[.command, .option]"))
        XCTAssertFalse(source.contains("keyDown"))
    }

    func testQuitReplyRuleAfterRejectedExtraWindows() {
        let host = MultiWindowHostController()
        defer { host.performQuitCleanup() }
        host.bootstrapAfterLaunch()
        createExtraWindows(on: host, count: 2)
        XCTAssertEqual(host.orderedKeys.count, 1)
        var replies = 0
        let result = host.quitCoordinatorForTests.beginTerminateLater(
            forwardRequestQuit: { host.forwardRequestQuit() },
            performNativeCleanup: { host.performQuitCleanup() },
            ackUntilCleanupComplete: { host.ackUntilQuitCleanupComplete() },
            reply: { replies += 1 }
        )
        XCTAssertEqual(result, .terminateLater)
        XCTAssertEqual(host.quitCoordinatorForTests.armedDeadlineMs, 500)
        XCTAssertEqual(replies, 1)
        XCTAssertTrue(host.orderedKeys.isEmpty)
    }

    func testApplicationShouldTerminateAfterLastWindowClosedIsFalse() {
        let delegate = AppDelegate()
        XCTAssertFalse(delegate.applicationShouldTerminateAfterLastWindowClosed(NSApp))
    }

    func testWindowActivationRaisesOnlyTheNamedWindow() {
        let host = seededQuitHost(windows: 2)
        defer { host.performQuitCleanup() }
        let target = seyal_app_window(host.appHandle, 1)
        XCTAssertEqual(navigateToActiveTab(of: target, handle: host.appHandle), 0)
        let expectedId = "seyal-window-\(target.window_lo)-\(target.window_hi)"
        // XCTest's hosted app is not foregrounded, so supply the key-state
        // acknowledgement while still asserting the requested WindowId.
        host.activationRequestForTests = { window in
            XCTAssertEqual(window.identifier?.rawValue, expectedId)
        }
        host.keyWindowStatusForTests = { $0.identifier?.rawValue == expectedId }
        host.applyPendingEffectsAndReconcile()
        XCTAssertEqual(host.liveHost.window?.identifier?.rawValue, expectedId)
        let windows = host.snapshotOrderedWindowKeys()
        XCTAssertEqual(windows.count, 2)
        XCTAssertEqual(windows[1].lo, target.window_lo)
        XCTAssertEqual(windows[1].hi, target.window_hi)
    }

    func testWindowActivationFailureRetriesBoundedTimesAndKeepsFocus() {
        let host = seededQuitHost(windows: 2)
        defer { host.performQuitCleanup() }
        let target = seyal_app_window(host.appHandle, 1)
        let liveBefore = host.liveHost.window
        XCTAssertEqual(navigateToActiveTab(of: target, handle: host.appHandle), 0)
        host.activationFailBudget = 8
        host.applyPendingEffects()
        XCTAssertEqual(host.lastActivationAttempts, Int(SEYAL_APP_WINDOW_ACTIVATION_MAX_ATTEMPTS))
        XCTAssertEqual(seyal_app_last_error(host.appHandle), 53)
        let after = seyal_app_window(host.appHandle, 1)
        XCTAssertNotEqual(after.flags & UInt16(SEYAL_APP_WINDOW_PRODUCT_ACTIVE), 0)
        XCTAssertTrue(host.liveHost.window === liveBefore)
        XCTAssertEqual(host.activationFailBudget, 8 - Int(SEYAL_APP_WINDOW_ACTIVATION_MAX_ATTEMPTS))
    }

    func testWindowActivationDetectsAppKitRefusingToKeyNamedWindow() {
        let host = seededQuitHost(windows: 2)
        defer { host.performQuitCleanup() }
        let target = seyal_app_window(host.appHandle, 1)
        let sourceWindow = host.liveHost.window
        XCTAssertNotNil(sourceWindow)
        let keyWindowBefore = NSApp.keyWindow

        XCTAssertEqual(navigateToActiveTab(of: target, handle: host.appHandle), 0)
        var requests = 0
        var targetWasKeyAfterRequest: Bool?
        host.activationRequestForTests = { requestedWindow in
            XCTAssertEqual(
                requestedWindow.identifier?.rawValue,
                "seyal-window-\(target.window_lo)-\(target.window_hi)"
            )
            requests += 1
            // Model AppKit refusing the request: the named window stays non-key.
            targetWasKeyAfterRequest = requestedWindow.isKeyWindow
        }
        host.applyPendingEffects()

        XCTAssertEqual(requests, Int(SEYAL_APP_WINDOW_ACTIVATION_MAX_ATTEMPTS))
        XCTAssertEqual(host.lastActivationAttempts, Int(SEYAL_APP_WINDOW_ACTIVATION_MAX_ATTEMPTS))
        XCTAssertEqual(seyal_app_last_error(host.appHandle), 53)
        XCTAssertEqual(targetWasKeyAfterRequest, false)
        XCTAssertTrue(NSApp.keyWindow === keyWindowBefore)
        XCTAssertTrue(host.liveHost.window === sourceWindow)
        XCTAssertNotEqual(
            seyal_app_window(host.appHandle, 1).flags & UInt16(SEYAL_APP_WINDOW_PRODUCT_ACTIVE),
            0,
            "portable focus remains committed after native activation failure"
        )
    }

    func testCreateWindowRejectedDoesNotMoveLiveHost() {
        let host = MultiWindowHostController()
        defer { host.performQuitCleanup() }
        host.bootstrapAfterLaunch()
        let beforeCount = seyal_app_shell(host.appHandle).window_count
        let liveWindow = host.liveHost.window
        XCTAssertNotNil(liveWindow)
        host.createWindow(nil)
        XCTAssertEqual(seyal_app_last_error(host.appHandle), 52, "WindowCreationUnavailable")
        XCTAssertEqual(seyal_app_shell(host.appHandle).window_count, beforeCount)
        XCTAssertTrue(host.liveHost.window === liveWindow)
        XCTAssertEqual(host.orderedKeys.count, Int(beforeCount))
    }

    private func seededQuitHost(windows: UInt32) -> MultiWindowHostController {
        let host = MultiWindowHostController()
        XCTAssertEqual(seyal_app_test_seed_windows_only(host.appHandle, windows), 0)
        host.applyPendingEffectsAndReconcile()
        return host
    }

    private func createExtraWindows(on host: MultiWindowHostController, count: Int) {
        for _ in 0..<count {
            let beforeCount = seyal_app_shell(host.appHandle).window_count
            let liveWindow = host.liveHost.window
            var action = SeyalAppAction()
            action.version = UInt16(SEYAL_APP_ABI_VERSION)
            action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
            action.kind = UInt16(SEYAL_APP_ACTION_CREATE_WINDOW.rawValue)
            XCTAssertEqual(seyal_app_apply(host.appHandle, &action), -4)
            XCTAssertEqual(seyal_app_last_error(host.appHandle), 52, "WindowCreationUnavailable")
            host.applyPendingEffectsAndReconcile()
            XCTAssertEqual(seyal_app_shell(host.appHandle).window_count, beforeCount)
            XCTAssertTrue(host.liveHost.window === liveWindow)
            XCTAssertEqual(host.orderedKeys.count, Int(beforeCount))
        }
    }

    @discardableResult
    private func navigateToActiveTab(of window: SeyalAppWindow, handle: UInt64) -> Int32 {
        var payload = [UInt8](repeating: 0, count: 36)
        payload[0] = 1
        payload[2] = 2
        writeId(&payload, offset: 4, lo: window.workspace_lo, hi: window.workspace_hi)
        writeId(&payload, offset: 20, lo: window.active_tab_lo, hi: window.active_tab_hi)
        let snapshot = seyal_app_snapshot(handle)
        return payload.withUnsafeBufferPointer { buffer in
            var action = SeyalAppAction()
            action.version = UInt16(SEYAL_APP_ABI_VERSION)
            action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
            action.kind = UInt16(SEYAL_APP_ACTION_NAVIGATE.rawValue)
            action.applySnapshotFence(snapshot)
            action.payload = buffer.baseAddress
            action.payload_len = 36
            return seyal_app_apply(handle, &action)
        }
    }

    private func writeId(_ payload: inout [UInt8], offset: Int, lo: UInt64, hi: UInt64) {
        var lo = lo.littleEndian
        var hi = hi.littleEndian
        withUnsafeBytes(of: &lo) { payload.replaceSubrange(offset..<(offset + 8), with: $0) }
        withUnsafeBytes(of: &hi) { payload.replaceSubrange((offset + 8)..<(offset + 16), with: $0) }
    }
}


@MainActor
private final class QuitReplyCounter {
    private(set) var count = 0
    func increment() { count += 1 }
}


@discardableResult
private func reloadUiConfigForMultiWindowTests(path: String) -> Int32 {
    let bytes = Array(path.utf8)
    return bytes.withUnsafeBufferPointer { buffer in
        seyal_app_test_reload_ui_configuration(buffer.baseAddress, bytes.count)
    }
}
