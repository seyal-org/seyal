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
        XCTAssertEqual(reloadUiConfig(path: missing.path), 0)
        NativeThemeRealization.resetColdDiagnosticsSurfacedForTests()
    }

    func testTerminateLaterRepliesOnceOnCleanupComplete() {
        var replies = 0
        let coordinator = ApplicationQuitCoordinator()
        var backstop: (() -> Void)?
        let reply = coordinator.beginForTest(
            forwardFailed: false,
            scheduleBackstop: { fire in backstop = fire },
            performNativeCleanup: {},
            ackUntilCleanupComplete: { true },
            reply: { replies += 1 }
        )
        XCTAssertEqual(reply, .terminateLater)
        XCTAssertEqual(replies, 1)
        XCTAssertEqual(coordinator.replyCount, 1)
        // Late backstop must not reply twice.
        backstop?()
        XCTAssertEqual(replies, 1)
    }

    func testBackstopRepliesOnceWhenCleanupNeverCompletes() {
        var replies = 0
        let coordinator = ApplicationQuitCoordinator()
        var backstop: (() -> Void)?
        let reply = coordinator.beginForTest(
            forwardFailed: false,
            scheduleBackstop: { fire in backstop = fire },
            performNativeCleanup: {},
            ackUntilCleanupComplete: { false },
            reply: { replies += 1 }
        )
        XCTAssertEqual(reply, .terminateLater)
        XCTAssertEqual(replies, 0)
        backstop?()
        XCTAssertEqual(replies, 1)
        coordinator.signalCleanupComplete()
        XCTAssertEqual(replies, 1)
    }

    func testForwardingFailureRepliesImmediately() {
        var replies = 0
        let coordinator = ApplicationQuitCoordinator()
        var backstopArmed = false
        let reply = coordinator.beginForTest(
            forwardFailed: true,
            scheduleBackstop: { _ in backstopArmed = true },
            performNativeCleanup: {},
            ackUntilCleanupComplete: { false },
            reply: { replies += 1 }
        )
        XCTAssertEqual(reply, .terminateLater)
        XCTAssertEqual(replies, 1)
        XCTAssertFalse(backstopArmed)
        coordinator.signalCleanupComplete()
        XCTAssertEqual(replies, 1)
    }

    func testBootstrapRealizesOneWindowWithTabbingDisallowed() {
        let host = MultiWindowHostController()
        host.bootstrapAfterLaunch()
        XCTAssertEqual(host.orderedKeys.count, 1)
        XCTAssertEqual(host.realizedTabbingModes(), [.disallowed])
        XCTAssertEqual(host.snapshotOrderedWindowKeys().count, 1)
        XCTAssertEqual(host.orderedKeys, host.snapshotOrderedWindowKeys())
    }

    func testUIOrderFollowsSnapshotNotNSAppWindows() {
        let host = MultiWindowHostController()
        host.bootstrapAfterLaunch()
        createExtraWindows(on: host, count: 2)
        let snapshotOrder = host.snapshotOrderedWindowKeys()
        XCTAssertEqual(snapshotOrder.count, 3)
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
        host.bootstrapAfterLaunch()
        createExtraWindows(on: host, count: 1)
        XCTAssertEqual(host.orderedKeys.count, 2)
        // Destroy-realization: quit cleanup applies destroy for every WindowId.
        host.performQuitCleanup()
        XCTAssertTrue(host.orderedKeys.isEmpty)
    }

    func testEventForwardingReportsBecameKey() {
        let host = MultiWindowHostController()
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
        XCTAssertTrue(source.contains("keyEquivalent: \"w\""))
        XCTAssertTrue(source.contains("hierarchicalClose"))
        XCTAssertTrue(source.contains("[.command, .option]"))
        XCTAssertFalse(source.contains("keyDown"))
        // N5: Go to… must target the ProductChromeHostView that implements openGoto.
        XCTAssertTrue(source.contains("gotoItem.target = host.liveHost"))
        XCTAssertFalse(source.contains("gotoItem.target = host\n"))
        let hostSource = try! String(
            contentsOf: URL(fileURLWithPath: #filePath)
                .deletingLastPathComponent()
                .deletingLastPathComponent()
                .deletingLastPathComponent()
                .appendingPathComponent("Sources/MultiWindowHostController.swift"),
            encoding: .utf8
        )
        XCTAssertFalse(hostSource.contains("func keyDown"), "⌘W must not use keyDown")
    }

    func testWindowShouldCloseForwardsCloseWindowAndDestroysViaEffect() {
        let host = MultiWindowHostController()
        host.bootstrapAfterLaunch()
        XCTAssertEqual(host.orderedKeys.count, 1)
        guard let key = host.orderedKeys.first,
              let window = host.realizedWindow(for: key)
        else {
            return XCTFail("expected bootstrap window")
        }
        // Refuse local destroy; Rust DestroyWindowRealization removes it.
        XCTAssertFalse(host.windowShouldClose(window))
        XCTAssertTrue(host.orderedKeys.isEmpty)
        XCTAssertEqual(seyal_app_shell(host.appHandle).window_count, 0)
        XCTAssertNil(host.realizedWindow(for: key))
        XCTAssertEqual(
            seyal_app_snapshot(host.appHandle).eligibility,
            UInt16(SEYAL_APP_ELIGIBILITY_UNBOUND.rawValue),
            "B1: CloseWindow must leave Unbound so recovery cannot open_first"
        )
        let effect = seyal_app_native_effect(host.appHandle, 0)
        XCTAssertNotEqual(
            effect.kind,
            UInt16(SEYAL_APP_EFFECT_BOUNDED_DETACH_THEN_TERMINATE)
        )
        XCTAssertNotEqual(effect.kind, UInt16(SEYAL_APP_EFFECT_TERMINATE_EXECUTION))
    }

    func testLastWindowCloseLeavesZeroWindowsWithoutQuit() {
        let host = MultiWindowHostController()
        host.bootstrapAfterLaunch()
        XCTAssertEqual(host.orderedKeys.count, 1)
        guard let key = host.orderedKeys.first else {
            return XCTFail("expected bootstrap window")
        }
        host.forwardCloseWindow(key)
        XCTAssertTrue(host.orderedKeys.isEmpty)
        XCTAssertEqual(seyal_app_shell(host.appHandle).window_count, 0)
        let delegate = AppDelegate()
        XCTAssertFalse(delegate.applicationShouldTerminateAfterLastWindowClosed(NSApp))
        XCTAssertEqual(host.quitReplyCount, 0)
    }

    func testZeroWindowReentryViaNewWindowAndDockReopen() {
        let host = MultiWindowHostController()
        host.bootstrapAfterLaunch()
        guard let key = host.orderedKeys.first else {
            return XCTFail("expected bootstrap window")
        }
        host.forwardCloseWindow(key)
        XCTAssertTrue(host.orderedKeys.isEmpty)

        // File → New Window from zero windows → ActivateWorkspace create path.
        host.createWindow(nil)
        XCTAssertEqual(host.orderedKeys.count, 1)
        XCTAssertEqual(seyal_app_shell(host.appHandle).window_count, 1)

        guard let again = host.orderedKeys.first else {
            return XCTFail("expected re-entered window")
        }
        host.forwardCloseWindow(again)
        XCTAssertTrue(host.orderedKeys.isEmpty)

        // Dock reopen path: same typed re-entry as AppDelegate when
        // hasVisibleWindows == false.
        host.handleDockReopen()
        XCTAssertEqual(host.orderedKeys.count, 1)

        let delegateSource = try! String(
            contentsOf: URL(fileURLWithPath: #filePath)
                .deletingLastPathComponent()
                .deletingLastPathComponent()
                .deletingLastPathComponent()
                .appendingPathComponent("Sources/AppDelegate.swift"),
            encoding: .utf8
        )
        XCTAssertTrue(delegateSource.contains("applicationShouldHandleReopen"))
        XCTAssertTrue(delegateSource.contains("handleDockReopen"))
        XCTAssertTrue(delegateSource.contains("hasVisibleWindows"))
    }

    func testTabCreationAdmittedPaneSplittingOff() {
        let host = MultiWindowHostController()
        host.bootstrapAfterLaunch()
        let shell = seyal_app_shell(host.appHandle)
        XCTAssertNotEqual(shell.flags & UInt16(SEYAL_APP_SHELL_ALLOWS_TAB_CREATION), 0)
        XCTAssertEqual(shell.flags & UInt16(SEYAL_APP_SHELL_ALLOWS_PANE_SPLITTING), 0)
        host.createTab(nil)
        XCTAssertEqual(seyal_app_shell(host.appHandle).tab_count, 2)
    }

    func testHierarchicalCloseMenuForwardsClosePane() {
        let host = MultiWindowHostController()
        host.bootstrapAfterLaunch()
        createExtraWindows(on: host, count: 1)
        XCTAssertEqual(host.orderedKeys.count, 2)
        // Sole pane in the active window → ClosePane peels to CloseWindow.
        host.hierarchicalClose(nil)
        XCTAssertEqual(host.orderedKeys.count, 1)
        XCTAssertEqual(seyal_app_shell(host.appHandle).window_count, 1)
    }

    func testQuitWithThreeWindowsFollowsReplyRule() {
        let host = MultiWindowHostController()
        host.bootstrapAfterLaunch()
        createExtraWindows(on: host, count: 2)
        XCTAssertEqual(host.orderedKeys.count, 3)
        switch host.forwardRequestQuit() {
        case .failure(let error):
            XCTFail("RequestQuit failed: \(error)")
        case .success(let deadline):
            XCTAssertEqual(deadline, 500)
        }
        var replies = 0
        let result = host.quitCoordinatorForTests.beginForTest(
            forwardFailed: false,
            scheduleBackstop: { _ in },
            performNativeCleanup: { host.performQuitCleanup() },
            ackUntilCleanupComplete: { host.ackUntilQuitCleanupComplete() },
            reply: { replies += 1 }
        )
        XCTAssertEqual(result, .terminateLater)
        XCTAssertEqual(replies, 1)
        XCTAssertTrue(host.orderedKeys.isEmpty)
    }

    func testApplicationShouldTerminateAfterLastWindowClosedIsFalse() {
        let delegate = AppDelegate()
        XCTAssertFalse(delegate.applicationShouldTerminateAfterLastWindowClosed(NSApp))
    }

    private func createExtraWindows(on host: MultiWindowHostController, count: Int) {
        for _ in 0..<count {
            var action = SeyalAppAction()
            action.version = UInt16(SEYAL_APP_ABI_VERSION)
            action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
            action.kind = UInt16(SEYAL_APP_ACTION_CREATE_WINDOW.rawValue)
            XCTAssertEqual(seyal_app_apply(host.appHandle, &action), 0)
            host.applyPendingEffectsAndReconcile()
        }
    }
}
