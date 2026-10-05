import AppKit
import Darwin
import Metal
import XCTest

@testable import Seyal

final class SeyalHostComponentTests: XCTestCase {
    override func setUp() {
        super.setUp()
        // Pin process cold config to a missing path so tests are hermetic and
        // do not inherit the developer's ~/.config/seyal/config.toml.
        let missing = FileManager.default.temporaryDirectory
            .appendingPathComponent("seyal-test-missing-\(UUID().uuidString).toml")
        XCTAssertEqual(reloadUiConfig(path: missing.path), 0)
        NativeThemeRealization.resetColdDiagnosticsSurfacedForTests()
    }

    func testApplicationRootABIMatchesPublishedHeader() {
        XCTAssertEqual(MemoryLayout<SeyalAppAction>.size, Int(MemoryLayout<SeyalAppAction>.stride))
        XCTAssertGreaterThanOrEqual(MemoryLayout<SeyalAppAction>.size, 80)
        XCTAssertGreaterThanOrEqual(MemoryLayout<SeyalAppSnapshot>.size, 64)
        let handle = seyal_app_create()
        XCTAssertNotEqual(handle, 0)
        let snapshot = seyal_app_snapshot(handle)
        XCTAssertEqual(snapshot.version, UInt16(SEYAL_APP_ABI_VERSION))
        XCTAssertEqual(snapshot.eligibility, UInt16(SEYAL_APP_ELIGIBILITY_UNBOUND.rawValue))
        XCTAssertEqual(MemoryLayout<SeyalAppBlockSpan>.size, 16)
        XCTAssertEqual(MemoryLayout<SeyalAppBlockProjection>.size, 24)
        XCTAssertEqual(UInt16(SEYAL_APP_BLOCK_PROJECTION_FAIL_CLOSED), 0)
        XCTAssertEqual(UInt16(SEYAL_APP_BLOCK_PROJECTION_HISTORY), 1)
        XCTAssertEqual(UInt16(SEYAL_APP_BLOCK_PROJECTION_PRIMARY_CLIP), 2)
        let emptySpan = seyal_app_block_span(handle, 0)
        XCTAssertEqual(emptySpan.start_line, 0)
        XCTAssertEqual(emptySpan.end_line, 0)
        let emptyProjection = seyal_app_block_projection(handle, 0)
        XCTAssertEqual(emptyProjection.kind, UInt16(SEYAL_APP_BLOCK_PROJECTION_FAIL_CLOSED))
        XCTAssertEqual(seyal_app_destroy(handle), 0)
        let theme = seyal_app_theme(0)
        XCTAssertNotEqual(theme.canvas, theme.text)
        XCTAssertEqual(MemoryLayout<SeyalAppComposer>.size, 40)
        XCTAssertEqual(MemoryLayout<SeyalComposerStatus>.size, 16)
        XCTAssertEqual(MemoryLayout<SeyalAppChrome>.size, 24)
        XCTAssertEqual(MemoryLayout<SeyalAppShell>.size, 112)
        XCTAssertEqual(MemoryLayout<SeyalAppRow>.size, 112)
        let live = seyal_app_create()
        // Core Terminal chrome is visible by default (#922).
        let chrome = seyal_app_chrome(live)
        XCTAssertEqual(chrome.reserved & UInt32(SEYAL_APP_CHROME_LEFT_VISIBLE), UInt32(SEYAL_APP_CHROME_LEFT_VISIBLE))
        XCTAssertEqual(
            chrome.reserved & UInt32(SEYAL_APP_CHROME_INSPECTOR_VISIBLE),
            UInt32(SEYAL_APP_CHROME_INSPECTOR_VISIBLE)
        )
        XCTAssertEqual(
            chrome.reserved & UInt32(SEYAL_APP_CHROME_TAB_STRIP_VISIBLE),
            UInt32(SEYAL_APP_CHROME_TAB_STRIP_VISIBLE)
        )
        let shell = seyal_app_shell(live)
        XCTAssertEqual(shell.workspace_count, 1)
        let workspace = seyal_app_shell_row(live, UInt16(SEYAL_APP_ROW_WORKSPACE), 0)
        XCTAssertGreaterThan(workspace.title_len, 0)
        XCTAssertEqual(seyal_app_destroy(live), 0)
        let dark = seyal_app_theme(0)
        let light = seyal_app_theme(1)
        XCTAssertNotEqual(dark.canvas, light.canvas)
        XCTAssertNotEqual(dark.canvas, dark.text)
        XCTAssertNotEqual(dark.accent, 0)
    }

    func testHostHasNoSeyalShellProductTypes() {
        let sourceRoot = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .appendingPathComponent("Sources", isDirectory: true)
        let enumerator = FileManager.default.enumerator(at: sourceRoot, includingPropertiesForKeys: nil)!
        var hits: [String] = []
        for case let file as URL in enumerator where file.pathExtension == "swift" {
            let text = (try? String(contentsOf: file, encoding: .utf8)) ?? ""
            let forbidden = [
                "SeyalShellState",
                "SeyalShellView",
                "SeyalShellPreviewFactory",
                "SeyalShellProductionFactory",
                "SeyalShellModel",
                "PanePresentationSession",
            ]
            if forbidden.contains(where: { text.contains($0) }) {
                hits.append(file.lastPathComponent)
            }
        }
        XCTAssertTrue(hits.isEmpty, "portable product types leaked into \(hits)")
    }

    func testNativeTestsConsumeRustApplicationRootFixturesOnly() {
        let handle = seyal_app_create()
        defer { XCTAssertEqual(seyal_app_destroy(handle), 0) }
        let shell = seyal_app_shell(handle)
        XCTAssertEqual(shell.workspace_count, 1)
        XCTAssertEqual(shell.tab_count, 1)
        XCTAssertEqual(shell.pane_count, 1)
        let chrome = seyal_app_chrome(handle)
        XCTAssertEqual(chrome.left_panel, 0)
        XCTAssertEqual(chrome.reserved, UInt32(SEYAL_APP_CHROME_LEFT_VISIBLE | SEYAL_APP_CHROME_INSPECTOR_VISIBLE | SEYAL_APP_CHROME_TAB_STRIP_VISIBLE))
        let composer = seyal_app_composer(handle)
        XCTAssertEqual(composer.mode, UInt16(SEYAL_APP_COMPOSER_HIDDEN.rawValue))
        let placeholder = seyal_app_copy(handle, UInt16(SEYAL_APP_COPY_COMPOSER_PLACEHOLDER))
        let execute = seyal_app_copy(handle, UInt16(SEYAL_APP_COPY_COMPOSER_EXECUTE))
        let prompt = seyal_app_copy(handle, UInt16(SEYAL_APP_COPY_BLOCK_PROMPT))
        XCTAssertEqual(utf8(placeholder), "Type a command...")
        XCTAssertEqual(utf8(execute), "⏎")
        XCTAssertEqual(utf8(prompt), "$")
        let inspector = seyal_app_chrome_row(handle, UInt16(SEYAL_APP_ROW_INSPECTOR), 0)
        XCTAssertGreaterThan(inspector.title_len, 0)
    }

    func testRefreshAlternateScreenAfterBindDerivesTui() {
        let handle = seyal_app_create()
        defer { XCTAssertEqual(seyal_app_destroy(handle), 0) }
        var snap = seyal_app_snapshot(handle)
        var bind = SeyalAppAction()
        bind.version = UInt16(SEYAL_APP_ABI_VERSION)
        bind.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        bind.kind = UInt16(SEYAL_APP_ACTION_BIND.rawValue)
        bind.flags = UInt16(SEYAL_APP_FLAG_TARGET_CONTROLLER)
        bind.fence_pane_lo = snap.pane_lo
        bind.fence_pane_hi = snap.pane_hi
        bind.fence_epoch = snap.epoch
        bind.target_execution_lo = 1
        bind.target_attachment_lo = 2
        bind.target_pty_generation = 1
        XCTAssertEqual(seyal_app_apply(handle, &bind), 0)
        snap = seyal_app_snapshot(handle)
        XCTAssertEqual(snap.eligibility, UInt16(SEYAL_APP_ELIGIBILITY_FLOW.rawValue))
        var refresh = SeyalAppAction()
        refresh.version = bind.version
        refresh.size = bind.size
        refresh.kind = UInt16(SEYAL_APP_ACTION_REFRESH.rawValue)
        refresh.applySnapshotFence(snap)
        refresh.flags |= UInt16(SEYAL_APP_FLAG_ALTERNATE_SCREEN)
        XCTAssertEqual(seyal_app_apply(handle, &refresh), 0)
        let tui = seyal_app_snapshot(handle)
        XCTAssertEqual(tui.eligibility, UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue))
        XCTAssertEqual(tui.flags & UInt16(SEYAL_APP_SNAP_COMPOSER), 0)
        XCTAssertEqual(
            seyal_app_composer(handle).mode,
            UInt16(SEYAL_APP_COMPOSER_HIDDEN.rawValue)
        )
    }

    @MainActor
    func testProductChromeReconcileIsReentrant() {
        let view = ProductChromeHostView(frame: NSRect(x: 0, y: 0, width: 800, height: 560))
        view.reconcileChrome()
        view.reconcileChrome()
    }

    @MainActor
    func testShellCompositionControlsFollowRustPolicyForTabsAndSplits() throws {
        let view = ProductChromeHostView(frame: NSRect(x: 0, y: 0, width: 800, height: 560))
        view.reconcileChrome()
        // C2b enables production tab creation; pane splitting stays off. With a
        // sole Tab/Pane, close controls remain omitted.
        let shell = seyal_app_shell(view.pane.appHandle)
        XCTAssertNotEqual(shell.flags & UInt16(SEYAL_APP_SHELL_ALLOWS_TAB_CREATION), 0)
        for bit in [
            SEYAL_APP_SHELL_ALLOWS_PANE_SPLITTING,
            SEYAL_APP_SHELL_ALLOWS_TAB_CLOSE,
            SEYAL_APP_SHELL_ALLOWS_PANE_CLOSE,
        ] {
            XCTAssertEqual(shell.flags & UInt16(bit), 0)
        }
        let newTab = try XCTUnwrap(accessibilityChild(view, identifier: "seyal-new-tab"))
        XCTAssertFalse(newTab.isHidden, "seyal-new-tab is shown when Rust allows CreateTab")
        for identifier in [
            "seyal-close-tab", "seyal-split-right", "seyal-split-down",
            "seyal-close-pane",
        ] {
            let control = try XCTUnwrap(accessibilityChild(view, identifier: identifier), identifier)
            XCTAssertTrue(control.isHidden, "\(identifier) is omitted when Rust disallows the action")
        }
    }

    @MainActor
    func testCreateTabIsEnabledInProductionCompositionWhileSplitsStayFailClosed() throws {
        let view = ProductChromeHostView(frame: NSRect(x: 0, y: 0, width: 800, height: 560))
        view.reconcileChrome()
        let handle = view.pane.appHandle
        let shellBefore = seyal_app_shell(handle)
        XCTAssertNotEqual(shellBefore.flags & UInt16(SEYAL_APP_SHELL_ALLOWS_TAB_CREATION), 0)
        XCTAssertEqual(shellBefore.flags & UInt16(SEYAL_APP_SHELL_ALLOWS_PANE_SPLITTING), 0)
        var create = SeyalAppAction()
        create.version = UInt16(SEYAL_APP_ABI_VERSION)
        create.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        create.kind = UInt16(SEYAL_APP_ACTION_CREATE_TAB.rawValue)
        XCTAssertEqual(seyal_app_apply(handle, &create), 0)
        view.reconcileChrome()
        let shell = seyal_app_shell(handle)
        XCTAssertEqual(shell.tab_count, 2)
        XCTAssertNotEqual(shell.flags & UInt16(SEYAL_APP_SHELL_ALLOWS_TAB_CREATION), 0)
        XCTAssertEqual(shell.flags & UInt16(SEYAL_APP_SHELL_ALLOWS_PANE_SPLITTING), 0)
    }

    @MainActor
    func testProductionPaneTreeProjectsOneLiveFocusedRegion() throws {
        XCTAssertEqual(MemoryLayout<SeyalAppPaneRegion>.size, 40)
        let view = ProductChromeHostView(frame: NSRect(x: 0, y: 0, width: 1200, height: 760))
        view.reconcileChrome()
        view.layoutSubtreeIfNeeded()
        let shell = seyal_app_shell(view.pane.appHandle)
        XCTAssertEqual(shell.pane_count, 1)
        let region = seyal_app_pane_region(view.pane.appHandle, 0)
        XCTAssertEqual(region.pane_lo, shell.focused_pane_lo)
        XCTAssertEqual(region.pane_hi, shell.focused_pane_hi)
        XCTAssertEqual(region.flags, UInt16(SEYAL_APP_PANE_REGION_FOCUSED | SEYAL_APP_PANE_REGION_LIVE))
        XCTAssertEqual(seyal_app_pane_region(view.pane.appHandle, 1).size, 0, "out of range fails closed")

        let regionView = try XCTUnwrap(accessibilityChild(view, identifier: "seyal-pane-region-0"))
        XCTAssertEqual(regionView.accessibilityValue() as? String, "focused")
        XCTAssertNil(accessibilityChild(view, identifier: "seyal-pane-region-1"))
        // The single live leaf hosts the terminal surface across the whole
        // center region, exactly as before multipane projection.
        let live = try XCTUnwrap(view.pane.superview)
        XCTAssertFalse(live.isHidden)
        XCTAssertEqual(live.frame, regionView.frame)
        XCTAssertGreaterThan(live.frame.width, 0)
    }

    @MainActor
    func testSinglePaneProjectsNoSplitDividerAndDividerDragFailsClosed() throws {
        XCTAssertEqual(MemoryLayout<SeyalAppPaneDivider>.size, 56)
        let view = ProductChromeHostView(frame: NSRect(x: 0, y: 0, width: 1200, height: 760))
        view.reconcileChrome()
        view.layoutSubtreeIfNeeded()
        let handle = view.pane.appHandle
        XCTAssertEqual(seyal_app_pane_divider(handle, 0).size, 0)
        XCTAssertNil(accessibilityChild(view, identifier: "seyal-pane-divider-0"))
        let row = seyal_app_shell_row(handle, UInt16(SEYAL_APP_ROW_PANE), 0)
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_MOVE_SPLIT_DIVIDER.rawValue)
        action.target_execution_lo = row.id_lo
        action.target_execution_hi = row.id_hi
        action.reserved = Float(0.3).bitPattern
        XCTAssertNotEqual(seyal_app_apply(handle, &action), 0)
        XCTAssertEqual(seyal_app_last_error(handle), 34, "NoSplitDivider")
        XCTAssertEqual(seyal_app_pane_region(handle, 0).width, 1.0, "rejected drag leaves the region full")
    }

    @MainActor
    func testNestedProductChangeDuringReconcileStillHidesComposerForTui() throws {
        let view = ProductChromeHostView(frame: NSRect(x: 0, y: 0, width: 800, height: 560))
        let handle = view.pane.appHandle
        var snap = seyal_app_snapshot(handle)
        var bind = SeyalAppAction()
        bind.version = UInt16(SEYAL_APP_ABI_VERSION)
        bind.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        bind.kind = UInt16(SEYAL_APP_ACTION_BIND.rawValue)
        bind.flags = UInt16(SEYAL_APP_FLAG_TARGET_CONTROLLER)
        bind.fence_pane_lo = snap.pane_lo
        bind.fence_pane_hi = snap.pane_hi
        bind.fence_epoch = snap.epoch
        bind.target_execution_lo = 1
        bind.target_attachment_lo = 2
        bind.target_pty_generation = 1
        XCTAssertEqual(seyal_app_apply(handle, &bind), 0)
        view.reconcileChrome()
        let composer = try XCTUnwrap(accessibilityChild(view, identifier: "seyal-composer"))
        XCTAssertFalse(composer.isHidden, "Flow bind must show composer")

        let chained = view.pane.onProductChanged
        view.pane.onProductChanged = {
            var current = seyal_app_snapshot(handle)
            var refresh = SeyalAppAction()
            refresh.version = bind.version
            refresh.size = bind.size
            refresh.kind = UInt16(SEYAL_APP_ACTION_REFRESH.rawValue)
            refresh.applySnapshotFence(current)
            refresh.flags |= UInt16(SEYAL_APP_FLAG_ALTERNATE_SCREEN)
            XCTAssertEqual(seyal_app_apply(handle, &refresh), 0)
            chained?()
            view.reconcileChrome()
        }
        view.pane.onProductChanged?()
        XCTAssertEqual(
            seyal_app_snapshot(handle).eligibility,
            UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue)
        )
        XCTAssertTrue(composer.isHidden, "nested TUI refresh must hide composer")
    }

    @MainActor
    func testShellExitRecoveryKeepsFlowAndHidesComposer() throws {
        let view = ProductChromeHostView(frame: NSRect(x: 0, y: 0, width: 800, height: 560))
        let handle = view.pane.appHandle
        let initial = seyal_app_snapshot(handle)
        var bind = SeyalAppAction()
        bind.version = UInt16(SEYAL_APP_ABI_VERSION)
        bind.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        bind.kind = UInt16(SEYAL_APP_ACTION_BIND.rawValue)
        bind.flags = UInt16(SEYAL_APP_FLAG_TARGET_CONTROLLER)
        bind.fence_pane_lo = initial.pane_lo
        bind.fence_pane_hi = initial.pane_hi
        bind.fence_epoch = initial.epoch
        bind.target_execution_lo = 1
        bind.target_attachment_lo = 2
        bind.target_pty_generation = 1
        XCTAssertEqual(seyal_app_apply(handle, &bind), 0)
        XCTAssertEqual(relayComposerStatus(handle, eligibility: 1, revision: 1), 0)
        view.reconcileChrome()
        let composer = try XCTUnwrap(accessibilityChild(view, identifier: "seyal-composer"))
        XCTAssertFalse(composer.isHidden, "a live Flow execution shows its composer")

        var begin = SeyalAppAction()
        begin.version = bind.version
        begin.size = bind.size
        begin.kind = UInt16(SEYAL_APP_ACTION_BEGIN_RECOVERY.rawValue)
        begin.target_pty_generation = 0
        XCTAssertEqual(seyal_app_apply(handle, &begin), 0)
        let generation = seyal_app_snapshot(handle).recovery_generation

        var ended = SeyalAppAction()
        ended.version = bind.version
        ended.size = bind.size
        ended.kind = UInt16(SEYAL_APP_ACTION_COMPLETE_RECOVERY.rawValue)
        ended.target_execution_lo = generation
        ended.target_pty_generation = 1
        ended.reserved = UInt32(SEYAL_APP_RECOVERY_EXECUTION_ENDED_OUTCOME.rawValue)
        XCTAssertEqual(seyal_app_apply(handle, &ended), 0)

        let snapshot = seyal_app_snapshot(handle)
        XCTAssertEqual(snapshot.eligibility, UInt16(SEYAL_APP_ELIGIBILITY_FLOW.rawValue))
        XCTAssertEqual(snapshot.recovery_stage, UInt16(SEYAL_APP_RECOVERY_EXECUTION_ENDED.rawValue))
        XCTAssertEqual(view.recoveryText(snapshot), "shell exited")
        view.reconcileChrome()
        XCTAssertTrue(composer.isHidden, "an ended shell cannot accept Flow commands")
        XCTAssertEqual(seyal_app_composer(handle).mode, UInt16(SEYAL_APP_COMPOSER_HIDDEN.rawValue))
    }

    func testBundledRuntimeLauncherUsesFixedHelperPath() {
        XCTAssertEqual(BundledRuntimeLauncher.helperRelativePath, "Contents/Helpers/seyal-runtime")
        XCTAssertEqual(BundledRuntimeLauncher.helperIdentifier, "dev.seyal.Seyal.runtime")
    }

    func testIsolatedRuntimeDirectoryDoesNotWeakenProductionDiscovery() {
        XCTAssertEqual(
            IsolatedRuntimeDirectory.helperArguments(from: ["Seyal"], testHostLoaded: false),
            []
        )
        // XCTest host with no `--runtime-dir` still synthesizes a directory and
        // must supply a default Flow shell so true-P1 empty-argv helpers create
        // one execution for headed component smoke (IME live callbacks).
        let testHostOnly = IsolatedRuntimeDirectory.helperArguments(
            from: ["Seyal"],
            testHostLoaded: true
        )
        XCTAssertEqual(testHostOnly.count, 3)
        XCTAssertEqual(testHostOnly[0], "--runtime-dir")
        XCTAssertTrue(testHostOnly[1].hasPrefix("/"))
        XCTAssertEqual(testHostOnly[2], "/bin/zsh")
        XCTAssertEqual(
            IsolatedRuntimeDirectory.helperArguments(
                from: ["Seyal", "--runtime-dir", "/tmp/seyal-iso"],
                testHostLoaded: false
            ),
            ["--runtime-dir", "/tmp/seyal-iso"]
        )
        XCTAssertEqual(
            IsolatedRuntimeDirectory.helperArguments(
                from: ["Seyal", "--runtime-dir", "/tmp/seyal-iso", "/bin/zsh"],
                testHostLoaded: false
            ),
            ["--runtime-dir", "/tmp/seyal-iso"]
        )
        XCTAssertEqual(
            IsolatedRuntimeDirectory.helperArguments(
                from: ["Seyal", "--runtime-dir", "/tmp/seyal-iso", "/bin/zsh"],
                testHostLoaded: true
            ),
            ["--runtime-dir", "/tmp/seyal-iso", "/bin/zsh"]
        )
        XCTAssertEqual(
            IsolatedRuntimeDirectory.helperArguments(
                from: ["Seyal", "--runtime-dir", "/tmp/seyal-iso", "/bin/zsh"],
                testHostLoaded: false,
                forwardHelperCommand: true
            ),
            ["--runtime-dir", "/tmp/seyal-iso", "/bin/zsh"]
        )
        XCTAssertEqual(
            BundledRuntimeLauncher.helperArgv(
                executable: "/tmp/seyal-runtime",
                processArguments: ["Seyal"],
                testHostLoaded: false
            ),
            ["/tmp/seyal-runtime"]
        )
        XCTAssertEqual(
            BundledRuntimeLauncher.helperArgv(
                executable: "/tmp/seyal-runtime",
                processArguments: ["Seyal", "--runtime-dir", "/tmp/seyal-iso"],
                testHostLoaded: false
            ),
            ["/tmp/seyal-runtime", "--runtime-dir", "/tmp/seyal-iso"]
        )
        XCTAssertEqual(
            BundledRuntimeLauncher.helperArgv(
                executable: "/tmp/seyal-runtime",
                processArguments: ["Seyal", "--runtime-dir", "/tmp/seyal-iso", "/bin/zsh"],
                testHostLoaded: false
            ),
            ["/tmp/seyal-runtime", "--runtime-dir", "/tmp/seyal-iso"]
        )
        XCTAssertEqual(
            BundledRuntimeLauncher.helperArgv(
                executable: "/tmp/seyal-runtime",
                processArguments: ["Seyal", "--runtime-dir", "/tmp/seyal-iso", "/bin/zsh"],
                testHostLoaded: true
            ),
            ["/tmp/seyal-runtime", "--runtime-dir", "/tmp/seyal-iso", "/bin/zsh"]
        )
        XCTAssertEqual(
            BundledRuntimeLauncher.helperArgv(
                executable: "/tmp/seyal-runtime",
                processArguments: ["Seyal", "--runtime-dir", "/tmp/seyal-iso", "/bin/zsh"],
                testHostLoaded: false,
                forwardHelperCommand: true
            ),
            ["/tmp/seyal-runtime", "--runtime-dir", "/tmp/seyal-iso", "/bin/zsh"]
        )
        // Release / production launch path: the env-var gate must not enable
        // helper-command forwarding, and trailing argv after `--runtime-dir`
        // PATH stays ignored even when the variable is set to "1".
        let releaseForward = BundledRuntimeLauncher.uiTestRequestsHelperCommand(
            environment: ["SEYAL_UI_TEST_FORWARD_RUNTIME_COMMAND": "1"],
            allowUiTestOverride: false
        )
        XCTAssertFalse(releaseForward)
        XCTAssertEqual(
            BundledRuntimeLauncher.helperArgv(
                executable: "/tmp/seyal-runtime",
                processArguments: [
                    "Seyal", "--runtime-dir", "/tmp/seyal-iso", "/bin/zsh", "--evil",
                ],
                testHostLoaded: false,
                forwardHelperCommand: releaseForward
            ),
            ["/tmp/seyal-runtime", "--runtime-dir", "/tmp/seyal-iso"]
        )
        XCTAssertEqual(
            IsolatedRuntimeDirectory.helperArguments(
                from: [
                    "Seyal", "--runtime-dir", "/tmp/seyal-iso", "/bin/zsh", "--evil",
                ],
                testHostLoaded: false,
                forwardHelperCommand: releaseForward
            ),
            ["--runtime-dir", "/tmp/seyal-iso"]
        )
        #if DEBUG
            XCTAssertEqual(
                BundledRuntimeLauncher.uiTestForwardRuntimeCommandEnvironmentKey,
                "SEYAL_UI_TEST_FORWARD_RUNTIME_COMMAND"
            )
            XCTAssertFalse(BundledRuntimeLauncher.uiTestRequestsHelperCommand(environment: [:]))
            XCTAssertFalse(
                BundledRuntimeLauncher.uiTestRequestsHelperCommand(
                    environment: ["XCTestConfigurationFilePath": "/tmp/config"]
                )
            )
            XCTAssertFalse(
                BundledRuntimeLauncher.uiTestRequestsHelperCommand(
                    environment: ["SEYAL_UI_TEST_FORWARD_RUNTIME_COMMAND": "0"]
                )
            )
            XCTAssertTrue(
                BundledRuntimeLauncher.uiTestRequestsHelperCommand(
                    environment: ["SEYAL_UI_TEST_FORWARD_RUNTIME_COMMAND": "1"]
                )
            )
            XCTAssertEqual(
                BundledRuntimeLauncher.helperArgv(
                    executable: "/tmp/seyal-runtime",
                    processArguments: [
                        "Seyal", "--runtime-dir", "/tmp/seyal-iso", "/bin/zsh",
                    ],
                    testHostLoaded: false,
                    forwardHelperCommand: BundledRuntimeLauncher.uiTestRequestsHelperCommand(
                        environment: ["SEYAL_UI_TEST_FORWARD_RUNTIME_COMMAND": "1"]
                    )
                ),
                ["/tmp/seyal-runtime", "--runtime-dir", "/tmp/seyal-iso", "/bin/zsh"]
            )
        #endif
    }

    @MainActor
    func testNativeKeyClassifierAndActionIDs() {
        XCTAssertTrue(InteractiveMetalSurfaceView.pass7InputSelfTest())
    }

    @MainActor
    private func withNativeIMEView(
        _ body: (InteractiveMetalSurfaceView, NSWindow) throws -> Void
    ) rethrows {
        let handle = seyal_app_create()
        defer { XCTAssertEqual(seyal_app_destroy(handle), 0) }
        let view = InteractiveMetalSurfaceView(
            frame: NSRect(x: 0, y: 0, width: 640, height: 400), appHandle: handle)
        view.suppressesAutomaticBridgeRecovery = true
        let window = NSWindow(
            contentRect: NSRect(x: 120, y: 160, width: 640, height: 400),
            styleMask: [.titled], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.contentView = view
        defer {
            view.removeFromSuperview()
            window.close()
        }
        try body(view, window)
    }

    @MainActor
    func testNativeIMEMarkedTextInsertClearsPreedit() {
        withNativeIMEView { view, _ in
            view.setMarkedText(
                "ni", selectedRange: NSRange(location: 2, length: 0),
                replacementRange: NSRange(location: NSNotFound, length: 0))
            XCTAssertTrue(view.hasMarkedText())
            XCTAssertEqual(view.markedRange(), NSRange(location: 0, length: 2))
            view.insertText(
                NSAttributedString(string: "你e\u{301}"),
                replacementRange: NSRange(location: NSNotFound, length: 0))
            XCTAssertFalse(view.hasMarkedText())
            XCTAssertEqual(view.markedRange().location, NSNotFound)
            // This unbound host checks native callbacks, not Runtime delivery.
            XCTAssertFalse(view.terminalBridgeIsConnected)
        }
    }

    @MainActor
    func testNativeIMEUnmarkClearsPreeditAndIsIdempotent() {
        withNativeIMEView { view, _ in
            view.setMarkedText(
                "かな", selectedRange: NSRange(location: 2, length: 0),
                replacementRange: NSRange(location: NSNotFound, length: 0))
            view.unmarkText()
            XCTAssertFalse(view.hasMarkedText())
            view.unmarkText()
            XCTAssertFalse(view.hasMarkedText())
            XCTAssertEqual(view.selectedRange(), NSRange(location: 0, length: 0))
        }
    }

    @MainActor
    func testNativeIMECancelCommandDiscardsPreedit() {
        withNativeIMEView { view, _ in
            view.setMarkedText(
                "preedit", selectedRange: NSRange(location: 7, length: 0),
                replacementRange: NSRange(location: NSNotFound, length: 0))
            view.doCommand(by: #selector(NSResponder.cancelOperation(_:)))
            XCTAssertFalse(view.hasMarkedText())
            XCTAssertEqual(view.markedRange().location, NSNotFound)
            view.unmarkText()
            XCTAssertFalse(view.hasMarkedText())
        }
    }

    @MainActor
    func testNativeIMEReplacementStaysInMarkedDocument() {
        withNativeIMEView { view, _ in
            view.setMarkedText(
                "abc", selectedRange: NSRange(location: 3, length: 0),
                replacementRange: NSRange(location: NSNotFound, length: 0))
            view.setMarkedText(
                NSAttributedString(string: "XYZ"),
                selectedRange: NSRange(location: 3, length: 0),
                replacementRange: NSRange(location: 1, length: 1))
            var actual = NSRange(location: NSNotFound, length: 0)
            XCTAssertEqual(
                view.attributedSubstring(
                    forProposedRange: NSRange(location: 0, length: 5),
                    actualRange: &actual)?.string, "aXYZc")
            XCTAssertEqual(actual, NSRange(location: 0, length: 5))
            view.insertText("aXYZc", replacementRange: NSRange(location: 0, length: 5))
            XCTAssertFalse(view.hasMarkedText())
        }
    }

    @MainActor
    func testNativeIMECandidateRectRejectsMissingProjection() {
        withNativeIMEView { view, window in
            view.setMarkedText(
                "ni", selectedRange: NSRange(location: 2, length: 0),
                replacementRange: NSRange(location: NSNotFound, length: 0))
            XCTAssertNil(view.terminalCurrentFrame())
            var actual = NSRange(location: 0, length: 0)
            XCTAssertEqual(
                view.firstRect(
                    forCharacterRange: NSRange(location: 0, length: 2),
                    actualRange: &actual), .zero)
            XCTAssertEqual(actual.location, NSNotFound)
            window.setFrameOrigin(NSPoint(x: 250, y: 300))
            XCTAssertEqual(
                view.firstRect(
                    forCharacterRange: NSRange(location: 99, length: 1),
                    actualRange: &actual), .zero)
            XCTAssertEqual(actual.location, NSNotFound)
        }
    }

    @MainActor
    func testNativeIMERemovingAndReattachingViewDiscardsPreedit() {
        withNativeIMEView { view, window in
            view.setMarkedText(
                "preedit", selectedRange: NSRange(location: 7, length: 0),
                replacementRange: NSRange(location: NSNotFound, length: 0))
            view.removeFromSuperview()
            XCTAssertNil(view.window)
            XCTAssertFalse(view.hasMarkedText())
            window.contentView = view
            XCTAssertNotNil(view.window)
            XCTAssertFalse(view.hasMarkedText())
            XCTAssertEqual(view.selectedRange(), NSRange(location: 0, length: 0))
        }
    }

    @MainActor
    func testNativeIMEDisconnectedBridgeDiscardsPreedit() {
        withNativeIMEView { view, _ in
            view.setMarkedText(
                "preedit", selectedRange: NSRange(location: 7, length: 0),
                replacementRange: NSRange(location: NSNotFound, length: 0))
            XCTAssertFalse(view.terminalBridgeIsConnected)
            view.terminalBridgeStatusDidChange()
            XCTAssertFalse(view.hasMarkedText())
        }
    }

    @MainActor
    func testNativeIMELiveCallbacksDeliverOnlyCommittedUTF8AndTrackCursor() async throws {
        let captureDirectory = FileManager.default.temporaryDirectory
            .appendingPathComponent("seyal-ime-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(
            at: captureDirectory, withIntermediateDirectories: false)
        defer {
            do {
                try FileManager.default.removeItem(at: captureDirectory)
            } catch {
                XCTFail("Could not remove IME capture directory: \(error)")
            }
        }
        let capture = captureDirectory.appendingPathComponent("committed.bin")
        let window = try XCTUnwrap(NSApp.windows.first {
            $0.contentView is ProductChromeHostView
        })
        let host = try XCTUnwrap(window.contentView as? ProductChromeHostView)
        let pane = host.pane
        let view = pane.inputSurface
        let originalFrame = window.frame
        let originalProductChanged = pane.onProductChanged
        var observeProductChange: (() -> Void)?
        pane.onProductChanged = {
            originalProductChanged?()
            observeProductChange?()
        }
        defer {
            pane.onProductChanged = originalProductChanged
            window.setFrame(originalFrame, display: true)
        }
        let connected = expectation(description: "production Runtime and projection connected")
        var connectedOnce = false
        var didRetryExhaustedRecovery = false
        let checkConnected = {
            let eligibility = seyal_app_snapshot(pane.appHandle).eligibility
            // A zsh account is Flow. A bash account is full-pane Raw (SPEC-008)
            // and is still a connected projection.
            let projected = eligibility == UInt16(SEYAL_APP_ELIGIBILITY_FLOW.rawValue)
                || eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue)
            if !connectedOnce, view.terminalBridgeIsConnected,
                view.terminalCurrentFrame() != nil,
                projected
            {
                connectedOnce = true
                connected.fulfill()
                return
            }
            // XCTest can reach this live-app case after the launch-time one-second
            // recovery episode has already exhausted. Exercise the same explicit
            // retry boundary available to the host once; do not poll a terminal
            // recovery state for the full 30-second test timeout.
            if !connectedOnce, !didRetryExhaustedRecovery,
                seyal_app_snapshot(pane.appHandle).recovery_stage
                    == UInt16(SEYAL_APP_RECOVERY_EXHAUSTED.rawValue)
            {
                didRetryExhaustedRecovery = true
                _ = view.retryRuntimeConnection()
            }
        }
        observeProductChange = checkConnected
        // Connection/projection can complete without another product-change
        // pulse after activate. Poll so a single missed callback is not a FAIL.
        let connectPoll = Timer.scheduledTimer(withTimeInterval: 0.25, repeats: true) { _ in
            checkConnected()
        }
        defer { connectPoll.invalidate() }
        window.makeKeyAndOrderFront(nil)
        host.activateAfterWindowPresentation()
        checkConnected()
        await fulfillment(of: [connected], timeout: 30)
        guard connectedOnce else { return }
        connectPoll.invalidate()

        // A bounded real PTY child records bytes; no input bridge is mocked.
        let script = """
        import os, select, sys, termios, time, tty
        fd = sys.stdin.fileno()
        saved = termios.tcgetattr(fd)
        data = bytearray()
        try:
            tty.setraw(fd)
            os.write(1, b"\\x1b[?1049h\\x1b[3;5HIME")
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline:
                if not select.select([fd], [], [], max(0, deadline - time.monotonic()))[0]:
                    break
                byte = os.read(fd, 1)
                if not byte or byte == b"\\x04":
                    break
                data.extend(byte)
        finally:
            try:
                with open(\(String(reflecting: capture.path)), "wb") as output:
                    output.write(data)
            finally:
                termios.tcsetattr(fd, termios.TCSANOW, saved)
                os.write(1, b"\\x1b[?1049l")
        """
        let command = "/usr/bin/python3 -c '"
            + script.replacingOccurrences(of: "'", with: "'\\''") + "'\n"
        let tui = expectation(description: "real child enters alternate screen")
        var tuiOnce = false
        observeProductChange = {
            if !tuiOnce, view.terminalCurrentFrame()?.alternate_screen == 1,
                seyal_app_snapshot(pane.appHandle).eligibility
                    == UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue)
            {
                tuiOnce = true
                tui.fulfill()
            }
        }
        XCTAssertEqual(view.terminalSubmitCommittedText(command), 0)
        defer {
            if view.terminalCurrentFrame()?.alternate_screen == 1 {
                XCTAssertEqual(view.terminalSubmitCommittedText("\u{04}"), 0)
            }
        }
        await fulfillment(of: [tui], timeout: 12)
        guard tuiOnce else { return }

        view.setMarkedText(
            "ni", selectedRange: NSRange(location: 2, length: 0),
            replacementRange: NSRange(location: NSNotFound, length: 0))
        let frame = try XCTUnwrap(view.terminalCurrentFrame())
        let cell = view.terminalPresentationCellSize()
        var actual = NSRange(location: NSNotFound, length: 0)
        let rect = view.firstRect(
            forCharacterRange: NSRange(location: 0, length: 2), actualRange: &actual)
        XCTAssertEqual(actual, NSRange(location: 0, length: 2))
        let expected = window.convertToScreen(view.convert(NSRect(
            x: view.bounds.minX + CGFloat(frame.cursor_column) * cell.width,
            y: view.bounds.maxY - CGFloat(frame.cursor_row + 1) * cell.height,
            width: cell.width, height: cell.height), to: nil))
        XCTAssertEqual(rect.origin.x, expected.origin.x, accuracy: 1)
        XCTAssertEqual(rect.origin.y, expected.origin.y, accuracy: 1)
        XCTAssertEqual(rect.width, cell.width, accuracy: 1)
        XCTAssertEqual(rect.height, cell.height, accuracy: 1)
        let previousWindowOrigin = window.frame.origin
        window.setFrameOrigin(previousWindowOrigin.applying(
            CGAffineTransform(translationX: 37, y: 29)))
        let moved = view.firstRect(
            forCharacterRange: NSRange(location: 0, length: 2), actualRange: &actual)
        XCTAssertNotEqual(window.frame.origin, previousWindowOrigin)
        XCTAssertEqual(
            moved.origin.x - rect.origin.x,
            window.frame.origin.x - previousWindowOrigin.x, accuracy: 1)
        XCTAssertEqual(
            moved.origin.y - rect.origin.y,
            window.frame.origin.y - previousWindowOrigin.y, accuracy: 1)
        view.insertText("你e\u{301}", replacementRange: NSRange(location: NSNotFound, length: 0))
        view.setMarkedText(
            "must-not-leak", selectedRange: NSRange(location: 13, length: 0),
            replacementRange: NSRange(location: NSNotFound, length: 0))
        view.doCommand(by: #selector(NSResponder.cancelOperation(_:)))
        view.unmarkText()
        view.setMarkedText(
            "abc", selectedRange: NSRange(location: 3, length: 0),
            replacementRange: NSRange(location: NSNotFound, length: 0))
        view.setMarkedText(
            "替換", selectedRange: NSRange(location: 2, length: 0),
            replacementRange: NSRange(location: 0, length: 3))
        view.insertText("替換", replacementRange: NSRange(location: 0, length: 2))
        view.setMarkedText(
            "unmark", selectedRange: NSRange(location: 6, length: 0),
            replacementRange: NSRange(location: NSNotFound, length: 0))
        view.unmarkText()
        view.unmarkText()
        let returned = expectation(description: "capture completes and child restores primary screen")
        var returnedOnce = false
        observeProductChange = {
            if !returnedOnce, view.terminalCurrentFrame()?.alternate_screen == 0,
                FileManager.default.fileExists(atPath: capture.path)
            {
                returnedOnce = true
                returned.fulfill()
            }
        }
        view.insertText("\u{04}", replacementRange: NSRange(location: NSNotFound, length: 0))
        await fulfillment(of: [returned], timeout: 20)
        XCTAssertEqual(try Data(contentsOf: capture), Data("你e\u{301}替換unmark".utf8))
    }

    @MainActor
    func testHostPasteAdmissionRejectsEmptyAndOversizedUTF8() {
        XCTAssertTrue(RustDisplayBridge.pasteAdmissionSelfTest())
    }

    /// E7 / #1020: ComposerRequestCorrelation was a dead Swift product-shaped
    /// remnant. Composer acceptance stays correlated by the Runtime request ID
    /// on the Rust side; the thin host must not reintroduce this type.
    @MainActor
    func testRustDisplayBridgeDoesNotOwnComposerRequestCorrelation() {
        let sourceRoot = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .appendingPathComponent("Sources", isDirectory: true)
        let bridge = sourceRoot.appendingPathComponent("RustDisplayBridge.swift")
        let text = (try? String(contentsOf: bridge, encoding: .utf8)) ?? ""
        XCTAssertFalse(text.isEmpty, "RustDisplayBridge.swift must be readable from the test bundle")
        XCTAssertFalse(text.contains("struct ComposerRequestCorrelation"))
        XCTAssertFalse(text.contains("ComposerRequestCorrelation"))
    }

    @MainActor
    func testXtermButtonMapDropsButtonsBeyondRight() {
        XCTAssertTrue(InteractiveMetalSurfaceView.pass7InputSelfTest())
    }

    /// Hygiene #962: orphaned `--renderer-self-test` scaffolding is gone; the
    /// production Metal surface remains constructible without that path.
    @MainActor
    func testMetalSurfaceRetainsProductionPathWithoutOrphanedSelfTestScaffolding() {
        let sourceRoot = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .appendingPathComponent("Sources", isDirectory: true)
        let metalSurface = sourceRoot.appendingPathComponent("MetalSurfaceView.swift")
        let main = sourceRoot.appendingPathComponent("Main.swift")
        let metalText = (try? String(contentsOf: metalSurface, encoding: .utf8)) ?? ""
        let mainText = (try? String(contentsOf: main, encoding: .utf8)) ?? ""
        XCTAssertFalse(metalText.contains("Pass6RegressionValidation"))
        XCTAssertFalse(metalText.contains("static func smokeTest()"))
        XCTAssertFalse(mainText.contains("--renderer-self-test"))
        XCTAssertTrue(mainText.contains("--renderer-benchmark"))
        let surface = MetalSurfaceView(frame: NSRect(x: 0, y: 0, width: 320, height: 200))
        XCTAssertEqual(surface.frame.size, CGSize(width: 320, height: 200))
        let handle = seyal_app_create()
        defer { XCTAssertEqual(seyal_app_destroy(handle), 0) }
        let interactive = InteractiveMetalSurfaceView(
            frame: NSRect(x: 0, y: 0, width: 320, height: 200),
            appHandle: handle
        )
        XCTAssertEqual(interactive.accessibilityIdentifier(), "terminal-input")
        XCTAssertTrue(InteractiveMetalSurfaceView.pass7InputSelfTest())
    }

    /// Steady-state Candidate-D frames must not call `seyal_app_snapshot`
    /// once recovery presentation is no longer pending (#1065).
    ///
    /// The production library does not export a snapshot-call counter. This
    /// test checks the same structure as `scripts/check-hot-path.py`: the
    /// pending guard returns before any snapshot FFI. A pending=false call
    /// still returns without entering that work.
    @MainActor
    func testAdvanceRecoveryPresentationMakesNoSnapshotCallsWhenNotPending() {
        let sourceRoot = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .appendingPathComponent("Sources", isDirectory: true)
        let recovery = sourceRoot.appendingPathComponent("MetalSurfaceView+Recovery.swift")
        let text = (try? String(contentsOf: recovery, encoding: .utf8)) ?? ""
        XCTAssertFalse(text.isEmpty, "MetalSurfaceView+Recovery.swift must be readable")
        guard let functionStart = text.range(of: "func advanceRecoveryPresentationIfReady()") else {
            return XCTFail("missing advanceRecoveryPresentationIfReady()")
        }
        let body = text[functionStart.lowerBound...]
        guard let pendingGuard = body.range(of: "guard recoveryPresentationPending") else {
            return XCTFail("advanceRecoveryPresentationIfReady must gate on recoveryPresentationPending")
        }
        if let snapshot = body.range(of: "seyal_app_snapshot") {
            XCTAssertGreaterThan(
                snapshot.lowerBound,
                pendingGuard.lowerBound,
                "seyal_app_snapshot must follow the recoveryPresentationPending guard"
            )
        }

        let handle = seyal_app_create()
        defer { XCTAssertEqual(seyal_app_destroy(handle), 0) }
        let view = InteractiveMetalSurfaceView(
            frame: NSRect(x: 0, y: 0, width: 320, height: 200),
            appHandle: handle
        )
        view.suppressesAutomaticBridgeRecovery = true
        view.recoveryPresentationPending = false
        XCTAssertTrue(view.advanceRecoveryPresentationIfReady())
        XCTAssertTrue(view.advanceRecoveryPresentationIfReady())
        XCTAssertFalse(view.recoveryPresentationPending)
    }

    /// #673 `renderer_prepare_submission`: the production `--renderer-benchmark`
    /// path must write a five-cohort TOML file for the named Metal-submit
    /// boundary. This is not scanout / key-to-photon.
    @MainActor
    func testTerminalDefaultCellColorsFollowThemeWithoutRebuildingPreparedFrames() {
        XCTAssertTrue(RendererValidation.retainedDefaultColorsFollowThemeOffscreenSelfTest())
    }

    @MainActor
    func testM002RendererPrepareSubmissionContractWritesCohort() throws {
        let sourceRoot = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .appendingPathComponent("Sources", isDirectory: true)
        let validationFiles = try FileManager.default.contentsOfDirectory(
            at: sourceRoot,
            includingPropertiesForKeys: nil
        ).filter { $0.lastPathComponent.hasPrefix("RendererValidation") && $0.pathExtension == "swift" }
        let validation = try validationFiles.map { try String(contentsOf: $0, encoding: .utf8) }
            .joined(separator: "\n")
        XCTAssertTrue(validation.contains("SEYAL_M002_CONTRACT_GATE"))
        XCTAssertTrue(validation.contains("renderer_prepare_submission"))
        XCTAssertTrue(validation.contains("runM002ContractCohort"))
        XCTAssertTrue(validation.contains("renderOffscreenAndMeasureSubmission"))

        let out = FileManager.default.temporaryDirectory
            .appendingPathComponent("m002-renderer-\(UUID().uuidString).toml")
        setenv("SEYAL_M002_CONTRACT_GATE", "renderer_prepare_submission", 1)
        setenv("SEYAL_M002_COHORT", "1", 1)
        setenv("SEYAL_M002_WARMUPS", "1", 1)
        setenv("SEYAL_M002_SAMPLES", "2", 1)
        setenv("SEYAL_M002_COHORT_OUT", out.path, 1)
        defer {
            unsetenv("SEYAL_M002_CONTRACT_GATE")
            unsetenv("SEYAL_M002_COHORT")
            unsetenv("SEYAL_M002_WARMUPS")
            unsetenv("SEYAL_M002_SAMPLES")
            unsetenv("SEYAL_M002_COHORT_OUT")
            try? FileManager.default.removeItem(at: out)
        }
        XCTAssertTrue(RendererValidation.runBenchmark())
        let body = try String(contentsOf: out, encoding: .utf8)
        XCTAssertTrue(body.contains("cohort = 1"))
        XCTAssertTrue(body.contains("samples = ["))
        XCTAssertEqual(body.split(separator: ",").count, 2)
    }

    /// #1020: Swift cohesion split keeps public self-test / teardown entrypoints
    /// on the production types while moving harness/helpers into sibling files.
    @MainActor
    func testCohesionSplitKeepsPublicSelfTestEntrypoints() {
        XCTAssertTrue(InteractiveMetalSurfaceView.pass7InputSelfTest())
        XCTAssertTrue(RustDisplayBridge.pasteAdmissionSelfTest())
        XCTAssertTrue(RustDisplayBridge.teardownReconnectStateSelfTest())
        XCTAssertTrue(MetalTerminalRenderer.gpuCompletionFailureRecoverySelfTest())
        let sourceRoot = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .appendingPathComponent("Sources", isDirectory: true)
        for required in [
            "InteractiveMetalSurfaceView+InputSelfTests.swift",
            "MetalTerminalRenderer+Encode.swift",
            "RustDisplayBridge+Input.swift",
            "ProductChromeBlockViews.swift",
            "MetalSurfaceRecoveryState.swift",
            "RendererValidation+Benchmark.swift",
        ] {
            let path = sourceRoot.appendingPathComponent(required)
            XCTAssertTrue(
                FileManager.default.fileExists(atPath: path.path),
                "missing cohesion sibling \(required)"
            )
        }
    }

    func testTranscriptFrameRejectsZeroBlockIdentity() {
        let invalid = NativeTranscriptFrame(
            revision: 1,
            regions: [NativeTranscriptRegion(id: 0, origin: .zero, clip: .zero)]
        )
        XCTAssertFalse(invalid.isValid)
        let valid = NativeTranscriptFrame(
            revision: 1,
            regions: [
                NativeTranscriptRegion(
                    id: 7,
                    origin: NSPoint(x: 0, y: 12),
                    clip: NSRect(x: 0, y: 12, width: 80, height: 24)
                )
            ]
        )
        XCTAssertTrue(valid.isValid)
        XCTAssertEqual(valid.regionIDs, [7])
    }

    // MARK: - Composer history (#933)

    /// Bind one Pane and drive an accepted composer submit through the FFI so
    /// Rust records history. Tests never fabricate rows.
    private func boundHandleWithHistory(_ commands: [String]) -> UInt64 {
        let handle = seyal_app_create()
        let snap = seyal_app_snapshot(handle)
        var bind = SeyalAppAction()
        bind.version = UInt16(SEYAL_APP_ABI_VERSION)
        bind.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        bind.kind = UInt16(SEYAL_APP_ACTION_BIND.rawValue)
        bind.flags = UInt16(SEYAL_APP_FLAG_TARGET_CONTROLLER)
        bind.fence_pane_lo = snap.pane_lo
        bind.fence_pane_hi = snap.pane_hi
        bind.fence_epoch = snap.epoch
        bind.target_execution_lo = 1
        bind.target_attachment_lo = 2
        bind.target_pty_generation = 1
        XCTAssertEqual(seyal_app_apply(handle, &bind), 0)
        XCTAssertEqual(relayComposerStatus(handle, eligibility: 1, revision: 1), 0)
        for command in commands {
            let bound = seyal_app_snapshot(handle)
            let composer = seyal_app_composer(handle)
            var draft = SeyalAppAction()
            draft.version = bind.version
            draft.size = bind.size
            draft.kind = UInt16(SEYAL_APP_ACTION_SET_COMPOSER_DRAFT.rawValue)
            draft.applySnapshotFence(bound)
            draft.target_pty_generation = composer.epoch
            let utf8 = Array(command.utf8)
            utf8.withUnsafeBufferPointer { buffer in
                draft.payload = buffer.baseAddress
                draft.payload_len = UInt32(buffer.count)
                XCTAssertEqual(seyal_app_apply(handle, &draft), 0)
            }
            var submit = SeyalAppAction()
            submit.version = bind.version
            submit.size = bind.size
            submit.kind = UInt16(SEYAL_APP_ACTION_SUBMIT_COMPOSER.rawValue)
            submit.applySnapshotFence(bound)
            submit.target_pty_generation = composer.epoch
            XCTAssertEqual(seyal_app_apply(handle, &submit), 0)
            var result = SeyalAppAction()
            result.version = bind.version
            result.size = bind.size
            result.kind = UInt16(SEYAL_APP_ACTION_APPLY_COMPOSER_RESULT.rawValue)
            result.applySnapshotFence(bound)
            result.target_execution_lo = seyal_app_composer(handle).request_id
            result.reserved = 1
            XCTAssertEqual(seyal_app_apply(handle, &result), 0)
        }
        return handle
    }

    /// Relay a Runtime composer status exactly as `ProductChromeHostView`
    /// does: eligibility code in `reserved`, Runtime revision in
    /// `target_execution_lo`. The value is a Runtime fixture, never a host
    /// decision.
    private func relayComposerStatus(_ handle: UInt64, eligibility: UInt32, revision: UInt64) -> Int32 {
        let snap = seyal_app_snapshot(handle)
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_APPLY_COMPOSER_STATUS.rawValue)
        action.applySnapshotFence(snap)
        action.reserved = eligibility
        action.target_execution_lo = revision
        return seyal_app_apply(handle, &action)
    }

    // MARK: - Composer eligibility (#978)

    /// A bound Pane reads busy until Runtime publishes Available, flips back
    /// to busy on a newer Busy revision, and ignores a stale Available relay.
    func testComposerReadsBusyUntilRuntimePublishesEligibility() {
        let handle = seyal_app_create()
        defer { XCTAssertEqual(seyal_app_destroy(handle), 0) }
        let snap = seyal_app_snapshot(handle)
        var bind = SeyalAppAction()
        bind.version = UInt16(SEYAL_APP_ABI_VERSION)
        bind.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        bind.kind = UInt16(SEYAL_APP_ACTION_BIND.rawValue)
        bind.flags = UInt16(SEYAL_APP_FLAG_TARGET_CONTROLLER)
        bind.fence_pane_lo = snap.pane_lo
        bind.fence_pane_hi = snap.pane_hi
        bind.fence_epoch = snap.epoch
        bind.target_execution_lo = 1
        bind.target_attachment_lo = 2
        bind.target_pty_generation = 1
        XCTAssertEqual(seyal_app_apply(handle, &bind), 0)

        var composer = seyal_app_composer(handle)
        XCTAssertEqual(composer.mode, UInt16(SEYAL_APP_COMPOSER_BUSY.rawValue))
        XCTAssertEqual(composer.flags & UInt16(SEYAL_APP_COMPOSER_CAN_SUBMIT), 0)
        XCTAssertEqual(
            utf8(seyal_app_copy(handle, UInt16(SEYAL_APP_COPY_COMPOSER_PLACEHOLDER))),
            "Waiting for prompt..."
        )

        XCTAssertEqual(
            relayComposerStatus(
                handle, eligibility: UInt32(SEYAL_APP_COMPOSER_ELIGIBILITY_AVAILABLE.rawValue), revision: 1),
            0
        )
        composer = seyal_app_composer(handle)
        XCTAssertEqual(composer.mode, UInt16(SEYAL_APP_COMPOSER_AVAILABLE.rawValue))
        XCTAssertEqual(
            utf8(seyal_app_copy(handle, UInt16(SEYAL_APP_COPY_COMPOSER_PLACEHOLDER))),
            "Type a command..."
        )

        XCTAssertEqual(
            relayComposerStatus(
                handle, eligibility: UInt32(SEYAL_APP_COMPOSER_ELIGIBILITY_BUSY.rawValue), revision: 3),
            0
        )
        XCTAssertEqual(seyal_app_composer(handle).mode, UInt16(SEYAL_APP_COMPOSER_BUSY.rawValue))
        // A delayed relay of an older Available cannot re-enable the composer.
        XCTAssertEqual(
            relayComposerStatus(
                handle, eligibility: UInt32(SEYAL_APP_COMPOSER_ELIGIBILITY_AVAILABLE.rawValue), revision: 2),
            0
        )
        XCTAssertEqual(seyal_app_composer(handle).mode, UInt16(SEYAL_APP_COMPOSER_BUSY.rawValue))
        // Transport lost clears the fact; a fresh attachment restarts at 1.
        XCTAssertEqual(
            relayComposerStatus(
                handle, eligibility: UInt32(SEYAL_APP_COMPOSER_ELIGIBILITY_NONE.rawValue), revision: 0),
            0
        )
        XCTAssertEqual(seyal_app_composer(handle).mode, UInt16(SEYAL_APP_COMPOSER_BUSY.rawValue))
        XCTAssertEqual(
            relayComposerStatus(
                handle, eligibility: UInt32(SEYAL_APP_COMPOSER_ELIGIBILITY_AVAILABLE.rawValue), revision: 1),
            0
        )
        XCTAssertEqual(seyal_app_composer(handle).mode, UInt16(SEYAL_APP_COMPOSER_AVAILABLE.rawValue))
    }

    /// After an accepted submit, Runtime's `Busy` may already be relayed when
    /// the result clears the Rust draft; the native editor must mirror the
    /// empty draft (placeholder visible) rather than keep the submitted text.
    @MainActor
    func testEditorMirrorsEmptyRustDraftWhileRuntimeBusy() throws {
        let view = ProductChromeHostView(frame: NSRect(x: 0, y: 0, width: 800, height: 560))
        let handle = view.pane.appHandle
        let snap = seyal_app_snapshot(handle)
        var bind = SeyalAppAction()
        bind.version = UInt16(SEYAL_APP_ABI_VERSION)
        bind.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        bind.kind = UInt16(SEYAL_APP_ACTION_BIND.rawValue)
        bind.flags = UInt16(SEYAL_APP_FLAG_TARGET_CONTROLLER)
        bind.fence_pane_lo = snap.pane_lo
        bind.fence_pane_hi = snap.pane_hi
        bind.fence_epoch = snap.epoch
        bind.target_execution_lo = 1
        bind.target_attachment_lo = 2
        bind.target_pty_generation = 1
        XCTAssertEqual(seyal_app_apply(handle, &bind), 0)
        XCTAssertEqual(relayComposerStatus(handle, eligibility: 1, revision: 1), 0)
        view.reconcileChrome()
        let composer = try XCTUnwrap(accessibilityChild(view, identifier: "seyal-composer"))
        let editor = try XCTUnwrap(
            accessibilityChild(view, identifier: "seyal-composer-editor") as? NSTextView)
        XCTAssertEqual(composer.accessibilityValue() as? String, "available")

        // Native typing is the source of the draft: the editor already holds
        // the text and the host commits it to Rust (SetDraft) as it changes.
        let bound = seyal_app_snapshot(handle)
        editor.string = "sleep 2"
        var draft = SeyalAppAction()
        draft.version = bind.version
        draft.size = bind.size
        draft.kind = UInt16(SEYAL_APP_ACTION_SET_COMPOSER_DRAFT.rawValue)
        draft.applySnapshotFence(bound)
        draft.target_pty_generation = seyal_app_composer(handle).epoch
        let draftBytes = Array("sleep 2".utf8)
        draftBytes.withUnsafeBufferPointer { buffer in
            draft.payload = buffer.baseAddress
            draft.payload_len = UInt32(buffer.count)
            XCTAssertEqual(seyal_app_apply(handle, &draft), 0)
        }
        view.reconcileChrome()
        XCTAssertEqual(editor.string, "sleep 2")

        var submit = SeyalAppAction()
        submit.version = bind.version
        submit.size = bind.size
        submit.kind = UInt16(SEYAL_APP_ACTION_SUBMIT_COMPOSER.rawValue)
        submit.applySnapshotFence(bound)
        submit.target_pty_generation = seyal_app_composer(handle).epoch
        XCTAssertEqual(seyal_app_apply(handle, &submit), 0)
        // Runtime's Busy flip arrives before the correlated result.
        XCTAssertEqual(relayComposerStatus(handle, eligibility: 2, revision: 2), 0)
        view.reconcileChrome()
        XCTAssertEqual(composer.accessibilityValue() as? String, "busy")
        XCTAssertEqual(editor.string, "sleep 2", "draft is kept while the request is in flight")

        var result = SeyalAppAction()
        result.version = bind.version
        result.size = bind.size
        result.kind = UInt16(SEYAL_APP_ACTION_APPLY_COMPOSER_RESULT.rawValue)
        result.applySnapshotFence(bound)
        result.target_execution_lo = seyal_app_composer(handle).request_id
        result.reserved = 1
        XCTAssertEqual(seyal_app_apply(handle, &result), 0)
        view.reconcileChrome()
        XCTAssertEqual(composer.accessibilityValue() as? String, "busy")
        XCTAssertEqual(editor.string, "", "accepted submit clears the editor even while Runtime is busy")
        XCTAssertEqual(
            utf8(seyal_app_copy(handle, UInt16(SEYAL_APP_COPY_COMPOSER_PLACEHOLDER))),
            "Waiting for prompt..."
        )
        XCTAssertEqual(relayComposerStatus(handle, eligibility: 1, revision: 3), 0)
        view.reconcileChrome()
        XCTAssertEqual(composer.accessibilityValue() as? String, "available")
        XCTAssertEqual(editor.string, "")
    }

    private func applyHistory(_ handle: UInt64, kind: UInt16, payload: String? = nil, reserved: UInt32 = 0) -> Int32 {
        let snap = seyal_app_snapshot(handle)
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = kind
        action.applySnapshotFence(snap)
        action.reserved = reserved
        action.target_pty_generation = seyal_app_composer(handle).epoch
        let utf8 = Array((payload ?? "").utf8)
        return utf8.withUnsafeBufferPointer { buffer in
            action.payload = payload == nil ? nil : buffer.baseAddress
            action.payload_len = payload == nil ? 0 : UInt32(buffer.count)
            return seyal_app_apply(handle, &action)
        }
    }

    func testComposerHistoryABIMatchesPublishedHeader() {
        XCTAssertEqual(MemoryLayout<SeyalAppComposerHistory>.size, 32)
        XCTAssertEqual(MemoryLayout<SeyalAppComposerHistory>.stride, 32)
        XCTAssertEqual(MemoryLayout<SeyalAppComposerHistory>.offset(of: \.query_utf8), 16)
        let handle = seyal_app_create()
        defer { XCTAssertEqual(seyal_app_destroy(handle), 0) }
        let closed = seyal_app_composer_history(handle)
        XCTAssertEqual(closed.version, UInt16(SEYAL_APP_ABI_VERSION))
        XCTAssertEqual(Int(closed.size), MemoryLayout<SeyalAppComposerHistory>.size)
        XCTAssertEqual(closed.flags, 0)
        XCTAssertEqual(closed.entry_count, 0)
        XCTAssertEqual(seyal_app_history_row(handle, 0).title_len, 0)
        let label = seyal_app_copy(handle, UInt16(SEYAL_APP_COPY_COMPOSER_HISTORY))
        XCTAssertGreaterThan(label.title_len, 0)
        let placeholder = seyal_app_copy(handle, UInt16(SEYAL_APP_COPY_COMPOSER_HISTORY_PLACEHOLDER))
        XCTAssertGreaterThan(placeholder.title_len, 0)
        // Unbound composer is not Available: open fails closed in Rust.
        XCTAssertEqual(applyHistory(handle, kind: UInt16(SEYAL_APP_ACTION_OPEN_COMPOSER_HISTORY.rawValue)), -4)
        XCTAssertEqual(seyal_app_composer_history(handle).flags & UInt16(SEYAL_APP_HISTORY_OPEN), 0)
    }

    func testComposerHistoryRowsAreRustRecordedAcceptedSubmits() {
        let handle = boundHandleWithHistory(["cargo build", "git status"])
        defer { XCTAssertEqual(seyal_app_destroy(handle), 0) }
        let recorded = seyal_app_composer_history(handle)
        XCTAssertEqual(recorded.flags, UInt16(SEYAL_APP_HISTORY_HAS_ENTRIES))
        XCTAssertEqual(recorded.entry_count, 2)
        XCTAssertEqual(recorded.row_count, 0, "closed overlay projects no rows")
        XCTAssertEqual(applyHistory(handle, kind: UInt16(SEYAL_APP_ACTION_OPEN_COMPOSER_HISTORY.rawValue)), 0)
        let open = seyal_app_composer_history(handle)
        XCTAssertEqual(open.flags, UInt16(SEYAL_APP_HISTORY_OPEN | SEYAL_APP_HISTORY_HAS_ENTRIES))
        XCTAssertEqual(open.row_count, 2)
        XCTAssertEqual(utf8(seyal_app_history_row(handle, 0)), "git status")
        XCTAssertEqual(seyal_app_history_row(handle, 0).flags & UInt16(SEYAL_APP_ROW_SELECTED), UInt16(SEYAL_APP_ROW_SELECTED))
        XCTAssertEqual(applyHistory(handle, kind: UInt16(SEYAL_APP_ACTION_SET_COMPOSER_HISTORY_FILTER.rawValue), payload: "carg"), 0)
        XCTAssertEqual(seyal_app_composer_history(handle).row_count, 1)
        XCTAssertEqual(utf8(seyal_app_history_row(handle, 0)), "cargo build")
        XCTAssertEqual(applyHistory(handle, kind: UInt16(SEYAL_APP_ACTION_SELECT_COMPOSER_HISTORY.rawValue)), 0)
        let composer = seyal_app_composer(handle)
        XCTAssertEqual(composer.request_id, 0, "select inserts into the draft; it never submits")
        let draft = String(decoding: UnsafeBufferPointer(start: composer.draft_utf8, count: Int(composer.draft_utf8_len)), as: UTF8.self)
        XCTAssertEqual(draft, "cargo build")
        XCTAssertEqual(seyal_app_composer_history(handle).flags & UInt16(SEYAL_APP_HISTORY_OPEN), 0)
    }

    @MainActor
    func testComposerHistoryOverlayProjectsRustStateOnly() {
        let handle = boundHandleWithHistory(["echo one", "echo two"])
        defer { XCTAssertEqual(seyal_app_destroy(handle), 0) }
        let overlay = ComposerHistoryOverlayView(appHandle: handle)
        overlay.reconcile()
        XCTAssertTrue(overlay.isHidden, "overlay is hidden until Rust opens it")
        XCTAssertEqual(applyHistory(handle, kind: UInt16(SEYAL_APP_ACTION_OPEN_COMPOSER_HISTORY.rawValue)), 0)
        overlay.reconcile()
        XCTAssertFalse(overlay.isHidden)
        XCTAssertEqual(overlay.accessibilityValue() as? String, "2")
        XCTAssertEqual(applyHistory(handle, kind: UInt16(SEYAL_APP_ACTION_MOVE_COMPOSER_HISTORY_SELECTION.rawValue), reserved: 1), 0)
        var dismissed = 0
        overlay.onDismissed = { dismissed += 1 }
        overlay.reconcile()
        overlay.reconcile()
        XCTAssertEqual(dismissed, 0)
        XCTAssertEqual(applyHistory(handle, kind: UInt16(SEYAL_APP_ACTION_CLOSE_COMPOSER_HISTORY.rawValue)), 0)
        overlay.reconcile()
        XCTAssertTrue(overlay.isHidden)
        XCTAssertEqual(dismissed, 1, "closing notifies the host exactly once")
        overlay.reconcile()
        XCTAssertEqual(dismissed, 1)
    }

    @MainActor
    func testProductChromeHostsHiddenHistoryOverlayAndReconciles() {
        let view = ProductChromeHostView(frame: NSRect(x: 0, y: 0, width: 800, height: 560))
        view.reconcileChrome()
        XCTAssertTrue(view.historyOverlay.isHidden)
        XCTAssertNotNil(view.historyOverlay.superview)
        view.reconcileChrome()
        XCTAssertTrue(view.historyOverlay.isHidden)
    }

    // MARK: - Command palette (#932)

    func testCommandPaletteABIMatchesPublishedHeaderAndFailsClosedForBogusRow() {
        XCTAssertEqual(MemoryLayout<SeyalAppPalette>.size, 32)
        XCTAssertEqual(MemoryLayout<SeyalAppPalette>.stride, 32)
        XCTAssertEqual(MemoryLayout<SeyalAppPalette>.offset(of: \.query_utf8), 16)
        let handle = seyal_app_create()
        defer { XCTAssertEqual(seyal_app_destroy(handle), 0) }
        let closed = seyal_app_palette(handle)
        XCTAssertEqual(closed.version, UInt16(SEYAL_APP_ABI_VERSION))
        XCTAssertEqual(Int(closed.size), MemoryLayout<SeyalAppPalette>.size)
        XCTAssertEqual(closed.flags, 0)
        XCTAssertEqual(closed.row_count, 0)
        XCTAssertTrue(closed.query_utf8 == nil)
        XCTAssertEqual(seyal_app_palette_row(handle, 0).title_len, 0, "no row at any index while closed")
    }

    func testCommandPaletteOpenListsCommandsWithoutBindingAndOmitsDisallowedOnes() {
        let handle = seyal_app_create()
        defer { XCTAssertEqual(seyal_app_destroy(handle), 0) }
        let snap = seyal_app_snapshot(handle)
        var open = SeyalAppAction()
        open.version = UInt16(SEYAL_APP_ABI_VERSION)
        open.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        open.kind = UInt16(SEYAL_APP_ACTION_OPEN_PALETTE.rawValue)
        open.applySnapshotFence(snap)
        // Unbound handle: require_fence only needs the Pane to exist, so the
        // palette opens before any Runtime attach.
        XCTAssertEqual(seyal_app_apply(handle, &open), 0)
        let palette = seyal_app_palette(handle)
        XCTAssertNotEqual(palette.flags & UInt16(SEYAL_APP_PALETTE_OPEN), 0)
        XCTAssertGreaterThan(palette.row_count, 0)
        var sawNewTab = false
        for index in 0..<Int(palette.row_count) {
            let row = seyal_app_palette_row(handle, UInt32(index))
            if utf8(row) == "New Tab" { sawNewTab = true }
        }
        XCTAssertTrue(
            sawNewTab,
            "production composition lists New Tab after C2b enablement"
        )
    }

    // MARK: - Block details inspector (#935)

    func testBlockSelectionFailsClosedWithoutRuntimeBlocks() {
        XCTAssertEqual(UInt16(SEYAL_APP_BLOCK_STATE_MASK), 7)
        XCTAssertEqual(UInt16(SEYAL_APP_BLOCK_SELECTED), 8)
        XCTAssertEqual(UInt16(SEYAL_APP_BLOCK_STATE_FAILED) & UInt16(SEYAL_APP_BLOCK_SELECTED), 0)
        XCTAssertEqual(UInt16(SEYAL_APP_INSPECTOR_BLOCK.rawValue), 4)
        let handle = seyal_app_create()
        defer { XCTAssertEqual(seyal_app_destroy(handle), 0) }
        let inspectorVisibleBeforeSelect =
            seyal_app_chrome(handle).reserved & UInt32(SEYAL_APP_CHROME_INSPECTOR_VISIBLE)
        var snap = seyal_app_snapshot(handle)
        var bind = SeyalAppAction()
        bind.version = UInt16(SEYAL_APP_ABI_VERSION)
        bind.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        bind.kind = UInt16(SEYAL_APP_ACTION_BIND.rawValue)
        bind.flags = UInt16(SEYAL_APP_FLAG_TARGET_CONTROLLER)
        bind.fence_pane_lo = snap.pane_lo
        bind.fence_pane_hi = snap.pane_hi
        bind.fence_epoch = snap.epoch
        bind.target_execution_lo = 1
        bind.target_attachment_lo = 2
        bind.target_pty_generation = 1
        XCTAssertEqual(seyal_app_apply(handle, &bind), 0)
        snap = seyal_app_snapshot(handle)
        XCTAssertEqual(seyal_app_composer(handle).block_count, 0, "no Runtime timeline in a unit test")
        var select = SeyalAppAction()
        select.version = bind.version
        select.size = bind.size
        select.kind = UInt16(SEYAL_APP_ACTION_SELECT_BLOCK.rawValue)
        select.applySnapshotFence(snap)
        select.target_execution_lo = 0x5151_5151_5151_5151
        select.target_execution_hi = 0x5151_5151_5151_5151
        XCTAssertEqual(seyal_app_apply(handle, &select), -4, "unknown Block fails closed")
        XCTAssertEqual(seyal_app_last_error(handle), 30)
        let chrome = seyal_app_chrome(handle)
        XCTAssertEqual(chrome.inspector_mode, UInt16(SEYAL_APP_INSPECTOR_CONTEXT.rawValue))
        XCTAssertEqual(
            chrome.reserved & UInt32(SEYAL_APP_CHROME_INSPECTOR_VISIBLE),
            inspectorVisibleBeforeSelect,
            "rejected select does not change inspector visibility"
        )
        for index in 0..<Int(chrome.inspector_row_count) {
            let row = seyal_app_chrome_row(handle, UInt16(SEYAL_APP_ROW_INSPECTOR), UInt32(index))
            XCTAssertFalse(utf8(row).hasPrefix("Block ·"), "no Block rows without a Block list")
        }
        var clear = SeyalAppAction()
        clear.version = bind.version
        clear.size = bind.size
        clear.kind = UInt16(SEYAL_APP_ACTION_CLEAR_BLOCK_SELECTION.rawValue)
        clear.applySnapshotFence(seyal_app_snapshot(handle))
        XCTAssertEqual(seyal_app_apply(handle, &clear), 0, "clearing nothing is idempotent")
    }

    @MainActor
    func testProductChromeReconcilesWithNoBlockSelection() {
        let view = ProductChromeHostView(frame: NSRect(x: 0, y: 0, width: 800, height: 560))
        view.reconcileChrome()
        let chrome = seyal_app_chrome(view.pane.appHandle)
        XCTAssertEqual(chrome.inspector_mode, UInt16(SEYAL_APP_INSPECTOR_CONTEXT.rawValue))
        view.reconcileChrome()
    }

    func testColdConfigTomlDrivesAppearanceFontsAndPadding() throws {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("seyal-993-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer {
            let missing = dir.appendingPathComponent("restore-missing.toml")
            reloadUiConfig(path: missing.path)
            try? FileManager.default.removeItem(at: dir)
        }

        let configured = dir.appendingPathComponent("config.toml")
        try """
        [ui]
        appearance = "light"
        window-padding = 12
        [ui.font]
        size = 16
        [terminal]
        padding = 14
        [terminal.font]
        size = 18
        """.write(to: configured, atomically: true, encoding: .utf8)

        XCTAssertEqual(reloadUiConfig(path: configured.path), 0)

        let visual = seyal_app_visual(0) // platform dark; preference light → resolved light
        XCTAssertEqual(visual.appearance, 1)
        XCTAssertEqual(visual.preference, 1)
        XCTAssertEqual(visual.ui_font_size, 16, accuracy: 0.01)
        XCTAssertEqual(visual.terminal_font_size, 18, accuracy: 0.01)
        XCTAssertEqual(visual.window_padding, 12, accuracy: 0.01)
        XCTAssertEqual(visual.terminal_padding, 14, accuracy: 0.01)
        XCTAssertEqual(visual.utility_material, 2, "default cold config allows frosted utility")
        XCTAssertEqual(visual.flags & 1, 0)

        let theme = NativeThemeRealization.theme(from: visual)
        XCTAssertEqual(theme.appearance.name, NSAppearance.Name.aqua)
        XCTAssertEqual(theme.terminalDefaultForeground, visual.text.byteSwapped)
        XCTAssertEqual(theme.terminalDefaultBackground, visual.canvas.byteSwapped)
        XCTAssertEqual(theme.uiFontSize, 16, accuracy: 0.01)
        XCTAssertEqual(theme.terminalFontSize, 18, accuracy: 0.01)
        XCTAssertEqual(theme.windowPadding, 12, accuracy: 0.01)
        XCTAssertEqual(theme.terminalPadding, 14, accuracy: 0.01)
        XCTAssertTrue(theme.usesFrostedUtilityMaterial)

        let invalid = dir.appendingPathComponent("bad.toml")
        try "this is not = toml [".write(to: invalid, atomically: true, encoding: .utf8)
        XCTAssertEqual(reloadUiConfig(path: invalid.path), 0)
        let fallback = seyal_app_visual(0)
        XCTAssertEqual(fallback.flags & 2, 2, "full-default fallback flag")
        XCTAssertEqual(fallback.ui_font_size, 12, accuracy: 0.01)
        XCTAssertGreaterThan(fallback.warning_count, 0)
    }

    /// #1119: Swift only renders Rust-owned launch-policy copy (ADR-015).
    func testLaunchPolicyProductCopyRendersRustOwnedStrings() {
        XCTAssertEqual(
            LaunchPolicyProductCopy.failureMessage(resultCode: 17, detailCode: 2),
            "Shell unavailable"
        )
        XCTAssertEqual(
            LaunchPolicyProductCopy.failureMessage(resultCode: 17, detailCode: 3),
            "Working directory unavailable"
        )
        XCTAssertFalse(
            LaunchPolicyProductCopy.failureMessage(resultCode: 17, detailCode: 1).contains("/")
        )
        let warnings = LaunchPolicyProductCopy.warningMessages(detailCode: 0b11)
        XCTAssertEqual(warnings.count, 2)
        XCTAssertTrue(warnings[0].contains("default shell"))
        XCTAssertTrue(warnings[1].contains("home directory"))
    }

    @MainActor
    func testColdConfigMaterialPreferenceRealizesOnVisualEffectView() throws {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("seyal-993-material-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer {
            let missing = dir.appendingPathComponent("restore-missing.toml")
            reloadUiConfig(path: missing.path)
            try? FileManager.default.removeItem(at: dir)
        }

        let frosted = dir.appendingPathComponent("frosted.toml")
        try """
        [ui]
        appearance = "dark"
        reduced-material = false
        utility-opacity = 0.85
        """.write(to: frosted, atomically: true, encoding: .utf8)
        XCTAssertEqual(reloadUiConfig(path: frosted.path), 0)

        let host = NSView(frame: NSRect(x: 0, y: 0, width: 200, height: 120))
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 200, height: 120),
            styleMask: [.titled, .closable],
            backing: .buffered,
            defer: true
        )
        window.contentView = host
        let material = NSVisualEffectView(frame: host.bounds)
        host.addSubview(material)
        let dark = NSAppearance(named: .darkAqua)!
        let frostedTheme = NativeThemeRealization.apply(to: host, material: material, appearance: dark)
        XCTAssertTrue(frostedTheme.usesFrostedUtilityMaterial)
        XCTAssertFalse(material.isHidden, "frosted utility material must be visible")
        XCTAssertEqual(material.material, .underWindowBackground)
        XCTAssertEqual(material.blendingMode, .withinWindow)
        XCTAssertTrue(window.isOpaque, "frosted utility must not clear window opacity")
        XCTAssertEqual(frostedTheme.utilityOpacity, 0.85, accuracy: 0.01)

        let reduced = dir.appendingPathComponent("reduced.toml")
        try """
        [ui]
        appearance = "dark"
        reduced-material = true
        """.write(to: reduced, atomically: true, encoding: .utf8)
        XCTAssertEqual(reloadUiConfig(path: reduced.path), 0)
        let reducedTheme = NativeThemeRealization.apply(to: host, material: material, appearance: dark)
        XCTAssertFalse(reducedTheme.usesFrostedUtilityMaterial)
        XCTAssertTrue(material.isHidden, "reduced-material must hide the frost effect")
        XCTAssertTrue(window.isOpaque)
        XCTAssertTrue((seyal_app_visual(0).flags & 1) != 0)
    }

    // MARK: - Flow live-tail primary clip (#865)

    /// Running Flow Blocks must clip only the Rust-mapped primary-frame row
    /// slice into the Block region (skip preceding viewport rows) without
    /// enabling Pane-wide live-grid drawing or inventing `start+511` history.
    @MainActor
    func testFlowLiveTailPrimaryClipStaysInsideBlockRegion() throws {
        guard let device = MTLCreateSystemDefaultDevice() else {
            throw XCTSkip("Metal unavailable")
        }
        let renderer = try MetalTerminalRenderer(device: device)
        renderer.setPresentationPlan(.flow())

        // Three prepared rows: preceding Block output on row 0, running
        // Block on rows 1..2. Clip must exclude row 0.
        func cell(_ ch: Character) -> SeyalPreparedCell {
            var c = SeyalPreparedCell()
            c.scalar = UInt32(ch.unicodeScalars.first!.value)
            c.foreground = 0xffe9_e1d8
            c.background = 0xff10_0d0b
            return c
        }
        var cells = [
            cell("A"), cell("B"),
            cell("H"), cell("i"),
            cell("!"), cell("!")
        ]
        func update(generation: UInt64, fullRebuild: Bool, damage: DamageMask) throws -> RendererUpdateResult {
            try cells.withUnsafeBufferPointer { buffer in
                try renderer.update(
                    frame: NativePreparedFrame(
                        cells: buffer,
                        generation: generation,
                        rows: 3,
                        columns: 2,
                        fullRebuild: fullRebuild,
                        damage: damage
                    ),
                    backingScale: 1
                )
            }
        }
        var damage = DamageMask()
        damage.markAll(rows: 3)
        XCTAssertEqual(try update(generation: 865, fullRebuild: true, damage: damage), .updated)

        let cellSize = renderer.cellPixelSize(backingScale: 1)
        let region = NativeTranscriptRegion(
            id: 865,
            origin: .zero,
            clip: NSRect(
                x: 0,
                y: 0,
                width: CGFloat(cellSize.width * 2),
                height: CGFloat(cellSize.height * 2)
            )
        )
        renderer.setTranscriptRegions([region])
        renderer.setLiveTailBlocks([
            865: LiveTailClip(startLine: 30, firstRow: 1, rowCount: 2)
        ])

        XCTAssertEqual(renderer.liveTailRegionCount, 1)
        // Dense layout: 2 rows × 2 cols (including any zero-size placeholders).
        XCTAssertEqual(renderer.liveTailInstanceCount(for: 865), 4)
        XCTAssertEqual(renderer.liveTailPaintedInstanceCount(for: 865), 4)
        // Block-local row 0 comes from prepared viewport row 1 ("H"), not
        // preceding viewport row 0 ("A").
        XCTAssertEqual(
            renderer.liveTailFirstPaintedOriginY(for: 865) ?? -1,
            0,
            "first painted live-tail row must be Block-local Y 0"
        )
        let allocationsAfterClip = renderer.stats.instanceBufferAllocations
        let rewrittenAfterClip = renderer.stats.liveTailCellsRewritten
        let inspection = renderer.inspectPresentation()
        XCTAssertEqual(inspection.mode, .flow)
        XCTAssertFalse(inspection.drawsLiveGrid)
        XCTAssertFalse(inspection.drawsFullGridBackground)
        XCTAssertFalse(inspection.drawsCursorOutsideBlockRegions)
        XCTAssertEqual(inspection.blockRegionIDs, [865])

        let paint = renderer.inspectFlowPaint()
        XCTAssertTrue(paint.isClean, "Flow must not submit Pane-wide live grid")
        XCTAssertEqual(paint.historyInstanceCount, 4)
        XCTAssertEqual(paint.instancesOutsideClips, 0)

        // Damage-free update must reuse the live-tail clip without allocating
        // another live-tail buffer or rewriting cells.
        XCTAssertEqual(try update(generation: 866, fullRebuild: false, damage: DamageMask()), .updated)
        XCTAssertEqual(renderer.liveTailInstanceCount(for: 865), 4)
        XCTAssertEqual(
            renderer.stats.instanceBufferAllocations,
            allocationsAfterClip,
            "damage-free live-tail refresh must not allocate"
        )
        XCTAssertEqual(
            renderer.stats.liveTailCellsRewritten,
            rewrittenAfterClip,
            "damage-free live-tail refresh must not rewrite cells"
        )

        // Partial damage on prepared row 2 (clip-local rowOffset 1) rewrites
        // only that row's two cells, not the full 4-cell clip.
        var partial = DamageMask()
        partial.mark(row: 2)
        cells[4] = cell("X")
        cells[5] = cell("Y")
        XCTAssertEqual(try update(generation: 867, fullRebuild: false, damage: partial), .updated)
        XCTAssertEqual(
            renderer.stats.liveTailCellsRewritten,
            rewrittenAfterClip &+ 2,
            "partial damage must rewrite only the damaged clip row"
        )
        XCTAssertEqual(renderer.liveTailPaintedInstanceCount(for: 865), 4)
        XCTAssertEqual(renderer.liveTailFirstPaintedOriginY(for: 865) ?? -1, 0, accuracy: 0.01)

        // Clearing live-tail must not re-enable Pane-wide live grid.
        renderer.setLiveTailBlocks([:])
        XCTAssertEqual(renderer.liveTailRegionCount, 0)
        XCTAssertFalse(renderer.inspectPresentation().drawsLiveGrid)
    }

    /// #865 / SPEC-008 §3.1: Flow transcript scroll handling republishes Block
    /// clips only. It must not change prepared-frame PTY size, cursor, or the
    /// last geometry proposal. Headed overflow scroll is covered by XCUI.
    @MainActor
    func testFlowTranscriptScrollDoesNotMutatePtySizeOrCursor() async throws {
        let window = try XCTUnwrap(NSApp.windows.first {
            $0.contentView is ProductChromeHostView
        })
        let host = try XCTUnwrap(window.contentView as? ProductChromeHostView)
        let pane = host.pane
        let view = pane.inputSurface
        let originalProductChanged = pane.onProductChanged
        var observeProductChange: (() -> Void)?
        pane.onProductChanged = {
            originalProductChanged?()
            observeProductChange?()
        }
        defer { pane.onProductChanged = originalProductChanged }

        let connected = expectation(description: "Flow projection connected for scroll invariant")
        var connectedOnce = false
        var didRetryExhaustedRecovery = false
        let checkConnected = {
            let eligibility = seyal_app_snapshot(pane.appHandle).eligibility
            if !connectedOnce, view.terminalBridgeIsConnected,
                view.terminalCurrentFrame() != nil,
                eligibility == UInt16(SEYAL_APP_ELIGIBILITY_FLOW.rawValue)
            {
                connectedOnce = true
                connected.fulfill()
                return
            }
            if !connectedOnce, !didRetryExhaustedRecovery,
                seyal_app_snapshot(pane.appHandle).recovery_stage
                    == UInt16(SEYAL_APP_RECOVERY_EXHAUSTED.rawValue)
            {
                didRetryExhaustedRecovery = true
                _ = view.retryRuntimeConnection()
            }
        }
        observeProductChange = checkConnected
        let connectPoll = Timer.scheduledTimer(withTimeInterval: 0.25, repeats: true) { _ in
            checkConnected()
        }
        defer { connectPoll.invalidate() }
        window.makeKeyAndOrderFront(nil)
        host.activateAfterWindowPresentation()
        checkConnected()
        await fulfillment(of: [connected], timeout: 30)
        guard connectedOnce else { return }
        connectPoll.invalidate()

        // Wait until prepared geometry/cursor stop moving so a later PTY
        // frame cannot be mistaken for scroll mutation.
        var before = try XCTUnwrap(view.terminalCurrentFrame())
        var stableCount = 0
        for _ in 0..<60 {
            try await Task.sleep(nanoseconds: 50_000_000)
            let sample = try XCTUnwrap(view.terminalCurrentFrame())
            if sample.rows == before.rows,
                sample.columns == before.columns,
                sample.cursor_row == before.cursor_row,
                sample.cursor_column == before.cursor_column,
                sample.cursor_visible == before.cursor_visible
            {
                stableCount += 1
                if stableCount >= 3 { break }
            } else {
                stableCount = 0
            }
            before = sample
        }
        XCTAssertGreaterThanOrEqual(stableCount, 3, "prepared frame must settle before scroll probe")
        view.refreshRecoveryAccessibilityValue()
        XCTAssertGreaterThan(before.rows, 0)
        XCTAssertGreaterThan(before.columns, 0)
        let eligibility = seyal_app_snapshot(pane.appHandle).eligibility
        guard eligibility == UInt16(SEYAL_APP_ELIGIBILITY_FLOW.rawValue) else {
            throw XCTSkip(
                "Flow scroll invariant requires Flow eligibility immediately before transcriptDidScroll (ENVIRONMENT_UNSUPPORTED when hosted CI has already left Flow)."
            )
        }
        let beforeGeometry = view.lastProposedGeometry
        let beforeBounds = view.bounds.integral
        let beforeRevision = host.transcriptFrameRevision

        // Exercise the same handler path as NSScrollView bounds changes:
        // clip republish + AX refresh, never proposeGeometry/resize.
        host.isProgrammaticTranscriptScroll = false
        host.transcriptDidScroll()
        host.transcriptDidScroll()
        // Immediate re-read: do not wait for unrelated PTY frames.
        let after = try XCTUnwrap(view.terminalCurrentFrame())

        XCTAssertGreaterThan(
            host.transcriptFrameRevision,
            beforeRevision,
            "scroll handler must republish Block clip frames"
        )
        XCTAssertEqual(view.bounds.integral, beforeBounds, "scroll handler must not resize Metal surface")
        XCTAssertEqual(
            view.lastProposedGeometry,
            beforeGeometry,
            "Flow transcript scroll must not propose a new PTY geometry"
        )
        XCTAssertEqual(after.rows, before.rows, "PTY rows must be unchanged after Flow scroll")
        XCTAssertEqual(after.columns, before.columns, "PTY columns must be unchanged after Flow scroll")
        XCTAssertEqual(after.cursor_row, before.cursor_row, "cursor row must be unchanged after Flow scroll")
        XCTAssertEqual(
            after.cursor_column,
            before.cursor_column,
            "cursor column must be unchanged after Flow scroll"
        )
        XCTAssertEqual(
            after.cursor_visible,
            before.cursor_visible,
            "cursor visibility must be unchanged after Flow scroll"
        )

        let ax = view.accessibilityValue() as? String ?? ""
        XCTAssertTrue(ax.contains("rows=\(before.rows)"), "AX probe must mirror unchanged rows")
        XCTAssertTrue(ax.contains("columns=\(before.columns)"), "AX probe must mirror unchanged columns")
        XCTAssertTrue(
            ax.contains("cursor=\(before.cursor_row),\(before.cursor_column)"),
            "AX probe must mirror unchanged cursor"
        )
    }

    /// Hygiene: Flow scroll handler must not call the resize/geometry path.
    @MainActor
    func testFlowTranscriptScrollSourceDoesNotProposeGeometry() throws {
        let sourceRoot = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .appendingPathComponent("Sources", isDirectory: true)
        let chrome = try String(
            contentsOf: sourceRoot.appendingPathComponent("ProductChromeHostView.swift"),
            encoding: .utf8
        )
        let blocks = try String(
            contentsOf: sourceRoot.appendingPathComponent("ProductChromeHostView+Blocks.swift"),
            encoding: .utf8
        )
        XCTAssertTrue(chrome.contains("func transcriptDidScroll()"))
        XCTAssertTrue(chrome.contains("publishBlockOutputFrame()"))
        XCTAssertFalse(
            chrome.contains("proposeGeometry"),
            "ProductChromeHostView scroll path must not propose PTY geometry"
        )
        XCTAssertFalse(
            chrome.contains("proposeCurrentGeometry"),
            "ProductChromeHostView scroll path must not propose PTY geometry"
        )
        XCTAssertTrue(blocks.contains("refreshRecoveryAccessibilityValue()"))
        XCTAssertFalse(
            blocks.contains("proposeGeometry"),
            "Block clip republish must not propose PTY geometry"
        )
        XCTAssertFalse(
            blocks.contains("proposeCurrentGeometry"),
            "Block clip republish must not propose PTY geometry"
        )
    }

    func testBlockProjectionABIRejectsInventedRunningHistoryEnd() {
        let handle = seyal_app_create()
        defer { XCTAssertEqual(seyal_app_destroy(handle), 0) }
        let unbound = seyal_app_block_projection(handle, 0)
        XCTAssertEqual(unbound.kind, UInt16(SEYAL_APP_BLOCK_PROJECTION_FAIL_CLOSED))
        XCTAssertEqual(unbound.end_line, 0)
        XCTAssertEqual(unbound.reserved0, 0)
        XCTAssertEqual(unbound.reserved1, 0)
    }

    @MainActor
    func testPaletteAddressPayloadOmitsEmptyRowsAndPrefixesVersionAndKind() throws {
        var empty = SeyalAppRow()
        XCTAssertNil(CommandPaletteOverlayView.addressPayload(for: empty))
        var row = SeyalAppRow()
        row.address_version = 1
        row.address_kind = 2
        row.address_len = 1
        let payload = try XCTUnwrap(CommandPaletteOverlayView.addressPayload(for: row))
        XCTAssertEqual(Array(payload.prefix(4)), [1, 0, 2, 0])
        XCTAssertEqual(payload.count, 5)
    }

    @MainActor
    func testGotoOpenSelectorIsWiredOnTheChromeHost() {
        XCTAssertTrue(ProductChromeHostView.instancesRespond(to: #selector(ProductChromeHostView.openGoto)))
    }

    @MainActor
    func testOpenGotoReusesPaletteOverlayWithDefaultPanesScope() {
        let view = ProductChromeHostView(frame: NSRect(x: 0, y: 0, width: 800, height: 560))
        view.openGoto()
        let palette = seyal_app_palette(view.pane.appHandle)
        XCTAssertNotEqual(palette.flags & UInt16(SEYAL_APP_PALETTE_OPEN), 0)
        XCTAssertNotEqual(palette.flags & UInt16(SEYAL_APP_PALETTE_GOTO), 0)
        XCTAssertEqual(
            UInt8(palette.reserved & 0xff),
            UInt8(SEYAL_APP_GOTO_PANES.rawValue)
        )
    }

    func testCommandKNormalizesToTheCommandModifierAndLowercaseK() throws {
        let event = try XCTUnwrap(NSEvent.keyEvent(
            with: .keyDown,
            location: .zero,
            modifierFlags: .command,
            timestamp: 0,
            windowNumber: 0,
            context: nil,
            characters: "k",
            charactersIgnoringModifiers: "k",
            isARepeat: false,
            keyCode: 40
        ))
        let payload = try XCTUnwrap(KeybindingStrokeNormalizer.normalize(event))
        XCTAssertEqual(payload.modifierBits, 1)
        XCTAssertEqual(payload.namedKey, 0)
        XCTAssertEqual(payload.base, UInt32(UnicodeScalar("k").value))
        XCTAssertEqual(payload.shiftApplied, 0)
    }

    func testF5NormalizesAsNamedKeyThirteen() throws {
        let f5 = String(Character(UnicodeScalar(NSF5FunctionKey)!))
        let event = try XCTUnwrap(NSEvent.keyEvent(
            with: .keyDown,
            location: .zero,
            modifierFlags: [],
            timestamp: 0,
            windowNumber: 0,
            context: nil,
            characters: f5,
            charactersIgnoringModifiers: f5,
            isARepeat: false,
            keyCode: 96
        ))
        let payload = try XCTUnwrap(KeybindingStrokeNormalizer.normalize(event))
        XCTAssertEqual(payload.namedKey, 1)
        XCTAssertEqual(payload.base, KeybindingStrokeNormalizer.namedF5)
        XCTAssertEqual(payload.shiftApplied, 0)
    }

    func testNamedKeyEquivalentMapsFKeysAndNavigation() {
        XCTAssertEqual(
            KeybindingShortcutRealization.namedKeyEquivalent(13),
            String(Character(UnicodeScalar(NSF5FunctionKey)!))
        )
        XCTAssertEqual(
            KeybindingShortcutRealization.namedKeyEquivalent(21),
            String(Character(UnicodeScalar(NSHomeFunctionKey)!))
        )
        XCTAssertEqual(
            KeybindingShortcutRealization.namedKeyEquivalent(25),
            String(Character(UnicodeScalar(NSDeleteFunctionKey)!))
        )
    }

    func testShortcutRealizationReadsTheRustCommandPaletteEquivalent() throws {
        let row = try XCTUnwrap(
            KeybindingShortcutRealization.item(commandId: KeybindingShortcutRealization.commandPaletteOpen)
        )
        XCTAssertEqual(row.command_id, KeybindingShortcutRealization.commandPaletteOpen)
        XCTAssertNotEqual(row.has_key_equivalent, 0)
    }

    /// R6.2.1: Command key equivalents route through Rust before the main menu.
    @MainActor
    func testPerformKeyEquivalentRoutesCommandBeforeMenu() throws {
        let view = ProductChromeHostView(frame: NSRect(x: 0, y: 0, width: 800, height: 560))
        let event = try XCTUnwrap(NSEvent.keyEvent(
            with: .keyDown,
            location: .zero,
            modifierFlags: .command,
            timestamp: 0,
            windowNumber: 0,
            context: nil,
            characters: "k",
            charactersIgnoringModifiers: "k",
            isARepeat: false,
            keyCode: 40
        ))
        XCTAssertTrue(
            view.performKeyEquivalent(with: event),
            "matched cmd+k must be consumed before AppKit menu dispatch"
        )
        let palette = seyal_app_palette(view.pane.appHandle)
        XCTAssertNotEqual(
            palette.flags & UInt16(SEYAL_APP_PALETTE_OPEN),
            0,
            "Rust should have opened the palette via route_keystroke"
        )
    }

    /// §6.2 step 2c: unbound Cmd+Left is native — must not be swallowed as consumed.
    @MainActor
    func testPerformKeyEquivalentLeavesUnboundCmdLeftNative() throws {
        let view = ProductChromeHostView(frame: NSRect(x: 0, y: 0, width: 800, height: 560))
        let event = try XCTUnwrap(NSEvent.keyEvent(
            with: .keyDown,
            location: .zero,
            modifierFlags: [.command, .numericPad, .function],
            timestamp: 0,
            windowNumber: 0,
            context: nil,
            characters: "\u{F702}",
            charactersIgnoringModifiers: "\u{F702}",
            isARepeat: false,
            keyCode: 123
        ))
        let routed = KeybindingStrokeNormalizer.route(
            appHandle: view.pane.appHandle,
            event: event,
            composerFocused: true,
            compositionActive: false
        )
        guard case .nativeCommand = routed else {
            XCTFail("unbound Cmd+Left must report native for composer text editing, got \(routed)")
            return
        }
        // No window/menu ownership here: native path calls super and returns false.
        XCTAssertFalse(
            view.performKeyEquivalent(with: event),
            "unbound Cmd+Left must not be consumed by the host"
        )
    }

    /// R8.4 via R6.2.1: `performKeyEquivalent` must forward marked-text from the
    /// focused metal surface (not hardcode `compositionActive: false`).
    @MainActor
    func testPerformKeyEquivalentSeesCompositionOnFocusedSurface() throws {
        let view = ProductChromeHostView(frame: NSRect(x: 0, y: 0, width: 800, height: 560))
        let window = NSWindow(
            contentRect: NSRect(x: 40, y: 80, width: 800, height: 560),
            styleMask: [.titled],
            backing: .buffered,
            defer: false
        )
        window.isReleasedWhenClosed = false
        window.contentView = view
        defer {
            view.removeFromSuperview()
            window.close()
        }

        XCTAssertTrue(window.makeFirstResponder(view.inputSurface))
        XCTAssertFalse(
            ProductChromeHostView.compositionActiveForKeyRouting(
                responder: window.firstResponder as? NSView,
                composer: view.composer,
                inputSurface: view.inputSurface
            ),
            "no marked text yet"
        )

        view.inputSurface.setMarkedText(
            "ni",
            selectedRange: NSRange(location: 2, length: 0),
            replacementRange: NSRange(location: NSNotFound, length: 0)
        )
        XCTAssertTrue(view.inputSurface.hasMarkedText())
        XCTAssertTrue(
            ProductChromeHostView.compositionActiveForKeyRouting(
                responder: window.firstResponder as? NSView,
                composer: view.composer,
                inputSurface: view.inputSurface
            ),
            "focused metal surface with marked text must report composition"
        )

        // Wiring check: performKeyEquivalent must use the live probe. A matched
        // single-stroke (⌘K) still consumes during composition (match precedes
        // the R8.4 gate); the probe must still be true at the call site.
        let event = try XCTUnwrap(NSEvent.keyEvent(
            with: .keyDown,
            location: .zero,
            modifierFlags: .command,
            timestamp: 0,
            windowNumber: window.windowNumber,
            context: nil,
            characters: "k",
            charactersIgnoringModifiers: "k",
            isARepeat: false,
            keyCode: 40
        ))
        XCTAssertTrue(
            ProductChromeHostView.compositionActiveForKeyRouting(
                responder: window.firstResponder as? NSView,
                composer: view.composer,
                inputSurface: view.inputSurface
            )
        )
        _ = view.performKeyEquivalent(with: event)
        XCTAssertTrue(
            view.inputSurface.hasMarkedText(),
            "routing must not clear marked text as a side effect"
        )
    }

    /// R8.4: palette / Go to… query field editor marked text must count as
    /// composition so Command chord prefixes cannot activate during overlay IME.
    @MainActor
    func testPerformKeyEquivalentSeesCompositionOnPaletteQueryFieldEditor() throws {
        let view = ProductChromeHostView(frame: NSRect(x: 0, y: 0, width: 800, height: 560))
        let window = NSWindow(
            contentRect: NSRect(x: 40, y: 80, width: 800, height: 560),
            styleMask: [.titled],
            backing: .buffered,
            defer: false
        )
        window.isReleasedWhenClosed = false
        window.contentView = view
        window.makeKeyAndOrderFront(nil)
        defer {
            view.removeFromSuperview()
            window.close()
        }

        view.openGoto()
        view.layoutSubtreeIfNeeded()
        let query = try XCTUnwrap(
            accessibilityChild(view, identifier: "seyal-command-palette-query") as? NSTextField,
            "goto overlay must expose the query field"
        )
        XCTAssertTrue(window.makeFirstResponder(query))
        let editor = try XCTUnwrap(
            (query.currentEditor() as? NSTextView) ?? (window.firstResponder as? NSTextView),
            "NSTextField must install a field editor while first responder"
        )

        XCTAssertFalse(
            ProductChromeHostView.compositionActiveForKeyRouting(
                responder: window.firstResponder as? NSView,
                composer: view.composer,
                inputSurface: view.inputSurface
            ),
            "no marked text on palette query yet"
        )

        editor.setMarkedText(
            "ni",
            selectedRange: NSRange(location: 2, length: 0),
            replacementRange: NSRange(location: NSNotFound, length: 0)
        )
        XCTAssertTrue(editor.hasMarkedText())
        XCTAssertTrue(
            ProductChromeHostView.compositionActiveForKeyRouting(
                responder: window.firstResponder as? NSView,
                composer: view.composer,
                inputSurface: view.inputSurface
            ),
            "focused palette/goto field editor with marked text must report composition"
        )

        // Live wiring: performKeyEquivalent must see the probe as true. Do not
        // assert marked text survives afterward — reconcile may rewrite the
        // query from Rust and AppKit field-editor IME state is not sticky across
        // that path.
        let event = try XCTUnwrap(NSEvent.keyEvent(
            with: .keyDown,
            location: .zero,
            modifierFlags: .command,
            timestamp: 0,
            windowNumber: window.windowNumber,
            context: nil,
            characters: "k",
            charactersIgnoringModifiers: "k",
            isARepeat: false,
            keyCode: 40
        ))
        XCTAssertTrue(
            ProductChromeHostView.compositionActiveForKeyRouting(
                responder: window.firstResponder as? NSView,
                composer: view.composer,
                inputSurface: view.inputSurface
            )
        )
        _ = view.performKeyEquivalent(with: event)
    }

    /// R11.2 / R6.4.1: Go to… is a projected WorkspaceCommand (title, enablement, invoke).
    @MainActor
    func testGotoMenuItemUsesProjectionInvokeAndRouteEnablement() {
        let view = ProductChromeHostView(frame: NSRect(x: 0, y: 0, width: 800, height: 560))
        let item = NSMenuItem(
            title: "",
            action: #selector(ProductChromeHostView.invokeProjectedWorkspaceCommand(_:)),
            keyEquivalent: ""
        )
        item.target = view
        KeybindingShortcutRealization.realize(item, commandId: KeybindingShortcutRealization.gotoOpen)
        XCTAssertEqual(item.representedObject as? UInt16, KeybindingShortcutRealization.gotoOpen)
        XCTAssertEqual(item.title, "Go to…")
        XCTAssertTrue(
            view.validateMenuItem(item),
            "goto.open is app-route permitted while the palette is closed"
        )

        view.openCommandPalette()
        XCTAssertFalse(
            view.validateMenuItem(item),
            "non-palette menu commands must disable while the palette owns the route"
        )

        // Fresh host: menu-invoke goto through the projected WorkspaceCommand path.
        let invokeView = ProductChromeHostView(frame: NSRect(x: 0, y: 0, width: 800, height: 560))
        let invokeItem = NSMenuItem(
            title: "",
            action: #selector(ProductChromeHostView.invokeProjectedWorkspaceCommand(_:)),
            keyEquivalent: ""
        )
        invokeItem.target = invokeView
        KeybindingShortcutRealization.realize(
            invokeItem,
            commandId: KeybindingShortcutRealization.gotoOpen
        )
        invokeView.invokeProjectedWorkspaceCommand(invokeItem)
        let palette = seyal_app_palette(invokeView.pane.appHandle)
        XCTAssertNotEqual(palette.flags & UInt16(SEYAL_APP_PALETTE_GOTO), 0)
    }

}

@discardableResult
private func reloadUiConfig(path: String) -> Int32 {
    let bytes = Array(path.utf8)
    return bytes.withUnsafeBufferPointer { buffer in
        seyal_app_test_reload_ui_configuration(buffer.baseAddress, bytes.count)
    }
}

private func utf8(_ row: SeyalAppRow) -> String {
    guard row.title_len > 0, let title = row.title else { return "" }
    return String(decoding: UnsafeBufferPointer(start: title, count: Int(row.title_len)), as: UTF8.self)
}

@MainActor
private func accessibilityChild(_ root: NSView, identifier: String) -> NSView? {
    if root.accessibilityIdentifier() == identifier {
        return root
    }
    for child in root.subviews {
        if let found = accessibilityChild(child, identifier: identifier) {
            return found
        }
    }
    return nil
}
