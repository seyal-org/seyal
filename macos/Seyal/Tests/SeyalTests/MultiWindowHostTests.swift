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
        // Probe-client registration with three windows is covered by Rust
        // `install_quit_fixture_attaches_probe_clients`.
        XCTAssertEqual(seyal_app_test_seed_windows_only(handle, 3), 0)
        XCTAssertEqual(seyal_app_shell(handle).window_count, 3)
    }

    func testQuitWithThreeWindowsAndAttachmentsRepliesOnce() {
        let host = MultiWindowHostController()
        defer { host.performQuitCleanup() }
        XCTAssertEqual(seyal_app_test_seed_quit_case(host.appHandle, 3), 0)
        XCTAssertEqual(seyal_app_test_live_attachment_count(host.appHandle), 3)
        host.applyPendingEffectsAndReconcile()
        XCTAssertEqual(host.orderedKeys.count, 3)
        let replies = QuitReplyCounter()
        let reply = host.quitCoordinatorForTests.beginTerminateLater(
            forwardRequestQuit: { host.forwardRequestQuit() },
            performNativeCleanup: { host.performQuitCleanup() },
            ackUntilCleanupComplete: { host.ackUntilQuitCleanupComplete() },
            reply: { replies.increment() }
        )
        XCTAssertEqual(reply, .terminateLater)
        XCTAssertEqual(replies.count, 1)
        XCTAssertEqual(host.quitCoordinatorForTests.replyCount, 1)
        XCTAssertEqual(seyal_app_test_live_attachment_count(host.appHandle), 0)
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

    func testMultiWindowHostPreservesProductFrameRefreshHook() {
        let host = MultiWindowHostController()
        defer { host.performQuitCleanup() }
        let before = host.liveHost.transcriptFrameRevision

        // No Rust snapshot change: the multi-window reconciliation returns
        // early, so only ProductChromeHostView's original frame hook advances
        // the projected PRIMARY_CLIP revision.
        host.liveHost.pane.onProductChanged?()

        XCTAssertGreaterThan(host.liveHost.transcriptFrameRevision, before)
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
        let host = seededQuitHost(windows: 2)
        defer { host.performQuitCleanup() }
        let target = host.orderedKeys[1]
        guard let targetWindow = host.realizedWindowForTests(target) else {
            return XCTFail("expected second realized window")
        }
        host.windowDidBecomeKey(
            Notification(name: NSWindow.didBecomeKeyNotification, object: targetWindow)
        )
        let shell = seyal_app_shell(host.appHandle)
        XCTAssertEqual(shell.window_count, 2)
        XCTAssertEqual(shell.active_window_lo, target.lo)
        XCTAssertEqual(shell.active_window_hi, target.hi)
        XCTAssertTrue(host.liveHost.window === targetWindow)
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
        applyAndWaitForActivation(host, reconcile: true)
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
        applyAndWaitForActivation(host)
        XCTAssertEqual(host.lastActivationAttempts, Int(SEYAL_APP_WINDOW_ACTIVATION_MAX_ATTEMPTS))
        XCTAssertEqual(host.lastActivationFailureReportForTests?.result, -4)
        XCTAssertEqual(host.lastActivationFailureReportForTests?.error, 53)
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
        applyAndWaitForActivation(host)

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

    func testWindowActivationWaitsForDelayedNamedWindowKeyAcknowledgement() {
        let host = seededQuitHost(windows: 2)
        defer { host.performQuitCleanup() }
        let target = seyal_app_window(host.appHandle, 1)
        let expectedId = "seyal-window-\(target.window_lo)-\(target.window_hi)"
        XCTAssertEqual(navigateToActiveTab(of: target, handle: host.appHandle), 0)

        let delayedKeyAcknowledgement = expectation(description: "named window becomes key")
        var targetBecameKey = false
        var requests = 0
        host.activationRequestForTests = { window in
            XCTAssertEqual(window.identifier?.rawValue, expectedId)
            requests += 1
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.02) {
                targetBecameKey = true
                delayedKeyAcknowledgement.fulfill()
            }
        }
        host.keyWindowStatusForTests = { _ in targetBecameKey }

        applyAndWaitForActivation(host)
        wait(for: [delayedKeyAcknowledgement], timeout: 1.0)

        XCTAssertEqual(requests, 1)
        XCTAssertEqual(host.lastActivationAttempts, 1)
        XCTAssertNil(host.lastActivationFailureReportForTests)
        XCTAssertEqual(host.liveHost.window?.identifier?.rawValue, expectedId)
    }

    private func applyAndWaitForActivation(_ host: MultiWindowHostController, reconcile: Bool = false) {
        let activationSettled = expectation(description: "window activation settles")
        host.activationCompletionForTests = { _ in activationSettled.fulfill() }
        if reconcile {
            host.applyPendingEffectsAndReconcile()
        } else {
            host.applyPendingEffects()
        }
        wait(for: [activationSettled], timeout: 1.0)
        host.activationCompletionForTests = nil
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
        guard let key = host.orderedKeys.first, let window = host.realizedWindowForTests(key) else {
            return XCTFail("expected bootstrap window")
        }
        XCTAssertFalse(window.isReleasedWhenClosed)
        XCTAssertFalse(host.windowShouldClose(window))
        XCTAssertTrue(NSApp.windows.contains { $0 === window })
        XCTAssertNotEqual(window.windowNumber, -1)
        XCTAssertTrue(host.orderedKeys.isEmpty)
        XCTAssertEqual(seyal_app_shell(host.appHandle).window_count, 0)
        assertWindowRealizationDestroyed(window)
    }

    func testProjectedCloseCommandDrainsRustWindowDestroyEffect() {
        let host = MultiWindowHostController()
        defer { host.performQuitCleanup() }
        host.bootstrapAfterLaunch()
        guard let key = host.orderedKeys.first,
              let window = host.realizedWindowForTests(key)
        else {
            return XCTFail("expected bootstrap window")
        }

        let item = NSMenuItem()
        item.representedObject = KeybindingShortcutRealization.appCloseFocused
        host.liveHost.invokeProjectedWorkspaceCommand(item)

        XCTAssertEqual(seyal_app_shell(host.appHandle).window_count, 0)
        XCTAssertNil(host.realizedWindowForTests(key))
        assertWindowRealizationDestroyed(window)
    }

    func testNativeKeyActivationUpdatesRustTargetBeforeHierarchicalClose() {
        let host = MultiWindowHostController()
        defer { host.performQuitCleanup() }
        host.bootstrapAfterLaunch()
        guard let first = host.orderedKeys.first,
              let firstWindow = host.realizedWindowForTests(first)
        else {
            return XCTFail("expected bootstrap window")
        }
        let firstPane = seyal_app_shell(host.appHandle)

        host.createWindow(nil)
        guard let second = host.orderedKeys.last,
              let secondWindow = host.realizedWindowForTests(second)
        else {
            return XCTFail("expected second window")
        }
        XCTAssertNotEqual(first, second)
        XCTAssertEqual(seyal_app_shell(host.appHandle).active_window_lo, second.lo)

        firstWindow.makeKeyAndOrderFront(nil)
        let firstActivated = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                MainActor.assumeIsolated {
                    let shell = seyal_app_shell(host.appHandle)
                    return shell.active_window_lo == first.lo
                        && shell.active_window_hi == first.hi
                        && shell.focused_pane_lo == firstPane.focused_pane_lo
                        && shell.focused_pane_hi == firstPane.focused_pane_hi
                }
            },
            object: nil
        )
        XCTAssertEqual(XCTWaiter.wait(for: [firstActivated], timeout: 2), .completed)

        let closeItem = NSMenuItem()
        closeItem.representedObject = KeybindingShortcutRealization.appCloseFocused
        host.liveHost.invokeProjectedWorkspaceCommand(closeItem)

        XCTAssertNil(host.realizedWindowForTests(first), "⌘W closes the Rust-focused first Window")
        XCTAssertNotNil(host.realizedWindowForTests(second), "the formerly active second Window survives")
        let afterClose = seyal_app_shell(host.appHandle)
        XCTAssertEqual(afterClose.window_count, 1)
        XCTAssertEqual(afterClose.active_window_lo, second.lo)
        XCTAssertEqual(afterClose.active_window_hi, second.hi)
        XCTAssertNotEqual(secondWindow.windowNumber, -1)
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

    private func assertWindowRealizationDestroyed(
        _ window: NSWindow,
        file: StaticString = #filePath,
        line: UInt = #line
    ) {
        let closed = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                MainActor.assumeIsolated {
                    !window.isVisible && !NSApp.windows.contains { $0 === window }
                }
            },
            object: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [closed], timeout: 2),
            .completed,
            "Rust destroy must order the NSWindow out and drop it from NSApp.windows",
            file: file,
            line: line
        )
    }

    private func seededQuitHost(windows: UInt32) -> MultiWindowHostController {
        let host = MultiWindowHostController()
        XCTAssertEqual(seyal_app_test_seed_windows_only(host.appHandle, windows), 0)
        host.applyPendingEffectsAndReconcile()
        return host
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
