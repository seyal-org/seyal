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
        host.createWindow(nil)
        host.createWindow(nil)
        let snapshotOrder = host.snapshotOrderedWindowKeys()
        XCTAssertEqual(snapshotOrder.count, 3)
        XCTAssertEqual(host.orderedKeys, snapshotOrder)
    }

    func testDestroyRealizationRemovesWindow() {
        let host = MultiWindowHostController()
        defer { host.performQuitCleanup() }
        host.bootstrapAfterLaunch()
        host.createWindow(nil)
        XCTAssertEqual(host.orderedKeys.count, 2)
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
        let host = MultiWindowHostController()
        defer { host.performQuitCleanup() }
        host.bootstrapAfterLaunch()

        let menu = AppDelegate.makeMainMenu(targeting: host)
        let fileMenu = menu.items.first { $0.submenu?.title == "File" }?.submenu
        let viewMenu = menu.items.first { $0.submenu?.title == "View" }?.submenu
        let windowMenu = menu.items.first { $0.submenu?.title == "Window" }?.submenu
        let projectedItems = [fileMenu, viewMenu, windowMenu]
            .compactMap { $0 }
            .flatMap(\.items)
            .filter {
                $0.representedObject is UInt16
                    && ($0.representedObject as? UInt16) != KeybindingShortcutRealization.windowSelectOrdinal
            }

        XCTAssertEqual(projectedItems.count, 15)
        XCTAssertEqual(
            Set(projectedItems.compactMap { $0.representedObject as? UInt16 }),
            Set((0..<seyal_app_shortcut_count()).compactMap { index in
                let row = seyal_app_shortcut_item(index)
                return row.size == 0 ? nil : row.command_id
            })
        )
        XCTAssertEqual(projectedItems.filter { $0.title == "New Tab" }.count, 1)
        XCTAssertEqual(
            windowMenu?.items.filter {
                $0.representedObject as? UInt16 == KeybindingShortcutRealization.windowSelectOrdinal
            }.count,
            9
        )
        XCTAssertTrue(projectedItems.contains { $0.title == "Split Right" })
        XCTAssertTrue(projectedItems.contains { $0.title == "Split Down" })
        XCTAssertTrue(projectedItems.contains { $0.title == "Toggle Raw" })
        XCTAssertTrue(projectedItems.contains { $0.title == "Toggle TUI" })
        XCTAssertTrue(projectedItems.contains { $0.title == "Go to…" })
        XCTAssertTrue(projectedItems.contains { $0.title == "Previous Tab" })
        XCTAssertTrue(projectedItems.contains { $0.title == "Next Tab" })
        XCTAssertTrue(projectedItems.contains { $0.title == "Close Tab" })
        XCTAssertTrue(projectedItems.allSatisfy { $0.target === host.liveHost })
        XCTAssertTrue(projectedItems.allSatisfy {
            $0.action == #selector(ProductChromeHostView.invokeProjectedWorkspaceCommand(_:))
        })

        let close = projectedItems.first { $0.representedObject as? UInt16 == 22 }
        XCTAssertEqual(close?.keyEquivalent, "w")
        XCTAssertTrue(close?.keyEquivalentModifierMask.contains(.command) == true)
    }

    func testQuitReplyRuleAfterExtraWindows() {
        let host = MultiWindowHostController()
        defer { host.performQuitCleanup() }
        host.bootstrapAfterLaunch()
        host.createWindow(nil)
        host.createWindow(nil)
        XCTAssertEqual(host.orderedKeys.count, 3)
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

    func testCreateWindowAdmissionIsOn() {
        let host = MultiWindowHostController()
        defer { host.performQuitCleanup() }
        host.bootstrapAfterLaunch()
        let beforeCount = seyal_app_shell(host.appHandle).window_count
        host.createWindow(nil)
        XCTAssertEqual(seyal_app_shell(host.appHandle).window_count, beforeCount + 1)
        XCTAssertEqual(host.orderedKeys.count, Int(beforeCount + 1))
    }

    func testWindowShouldCloseForwardsCloseWindowAndDestroysViaEffect() {
        let host = MultiWindowHostController()
        defer { host.performQuitCleanup() }
        host.bootstrapAfterLaunch()
        guard let key = host.orderedKeys.first, let window = host.realizedWindow(for: key) else {
            return XCTFail("expected bootstrap window")
        }
        XCTAssertFalse(
            window.isReleasedWhenClosed,
            "Swift ARC owns realized windows; AppKit must not release them on close"
        )
        XCTAssertFalse(host.windowShouldClose(window))
        XCTAssertTrue(host.orderedKeys.isEmpty)
        XCTAssertEqual(seyal_app_shell(host.appHandle).window_count, 0)
    }

    func testComponentHostDestroysWindowThroughAppKitClosePath() {
        let host = MultiWindowHostController()
        defer { host.performQuitCleanup() }
        host.bootstrapAfterLaunch()
        guard let key = host.orderedKeys.first, let window = host.realizedWindow(for: key) else {
            return XCTFail("expected bootstrap window")
        }

        // Start from AppKit's real close request. The delegate must veto the
        // local close, forward CloseWindow, and let Rust's effect close it.
        window.performClose(nil)

        XCTAssertFalse(NSApp.windows.contains { $0 === window })
        XCTAssertEqual(window.windowNumber, -1)
    }

    func testLastWindowCloseLeavesZeroWindowsWithoutQuit() {
        let host = MultiWindowHostController()
        defer { host.performQuitCleanup() }
        host.bootstrapAfterLaunch()
        guard let key = host.orderedKeys.first else {
            return XCTFail("expected bootstrap window")
        }
        host.forwardCloseWindow(key)
        XCTAssertTrue(host.orderedKeys.isEmpty)
        XCTAssertEqual(seyal_app_shell(host.appHandle).window_count, 0)
        let delegate = AppDelegate()
        XCTAssertFalse(delegate.applicationShouldTerminateAfterLastWindowClosed(NSApp))
    }

    func testZeroWindowReentryViaNewWindowAndDockReopen() {
        let host = MultiWindowHostController()
        defer { host.performQuitCleanup() }
        host.bootstrapAfterLaunch()
        guard let key = host.orderedKeys.first else {
            return XCTFail("expected bootstrap window")
        }
        host.forwardCloseWindow(key)
        XCTAssertEqual(seyal_app_shell(host.appHandle).window_count, 0)
        host.createWindow(nil)
        XCTAssertEqual(seyal_app_shell(host.appHandle).window_count, 1)
        XCTAssertEqual(host.orderedKeys.count, 1)
        guard let again = host.orderedKeys.first else {
            return XCTFail("expected re-created window")
        }
        host.forwardCloseWindow(again)
        XCTAssertEqual(seyal_app_shell(host.appHandle).window_count, 0)
        let delegate = AppDelegate(host: host)
        XCTAssertTrue(delegate.applicationShouldHandleReopen(NSApp, hasVisibleWindows: false))
        XCTAssertEqual(seyal_app_shell(host.appHandle).window_count, 1)
        XCTAssertEqual(host.orderedKeys.count, 1)
    }

    func testHierarchicalCloseMenuForwardsClosePane() {
        let host = MultiWindowHostController()
        defer { host.performQuitCleanup() }
        host.bootstrapAfterLaunch()
        host.hierarchicalClose(nil)
        XCTAssertTrue(host.orderedKeys.isEmpty)
        XCTAssertEqual(seyal_app_shell(host.appHandle).window_count, 0)
    }

    private func seededQuitHost(windows: UInt32) -> MultiWindowHostController {
        let host = MultiWindowHostController()
        XCTAssertEqual(seyal_app_test_seed_windows_only(host.appHandle, windows), 0)
        host.applyPendingEffectsAndReconcile()
        return host
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
