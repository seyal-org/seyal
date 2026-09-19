import AppKit
import Darwin
import XCTest

@testable import Seyal

final class SeyalHostComponentTests: XCTestCase {
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
        let emptySpan = seyal_app_block_span(handle, 0)
        XCTAssertEqual(emptySpan.start_line, 0)
        XCTAssertEqual(emptySpan.end_line, 0)
        XCTAssertEqual(seyal_app_destroy(handle), 0)
        let theme = seyal_app_theme(0)
        XCTAssertNotEqual(theme.canvas, theme.text)
        XCTAssertEqual(MemoryLayout<SeyalAppComposer>.size, 40)
        XCTAssertEqual(MemoryLayout<SeyalComposerStatus>.size, 16)
        XCTAssertEqual(MemoryLayout<SeyalAppChrome>.size, 24)
        XCTAssertEqual(MemoryLayout<SeyalAppShell>.size, 64)
        XCTAssertEqual(MemoryLayout<SeyalAppRow>.size, 56)
        let live = seyal_app_create()
        let chrome = seyal_app_chrome(live)
        XCTAssertEqual(chrome.reserved & UInt32(SEYAL_APP_CHROME_LEFT_VISIBLE), 0)
        XCTAssertEqual(chrome.reserved & UInt32(SEYAL_APP_CHROME_INSPECTOR_VISIBLE), 0)
        XCTAssertEqual(chrome.reserved & UInt32(SEYAL_APP_CHROME_TAB_STRIP_VISIBLE), 0)
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
        XCTAssertEqual(chrome.reserved, 0)
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

    func testBundledRuntimeLauncherUsesFixedHelperPath() {
        XCTAssertEqual(BundledRuntimeLauncher.helperRelativePath, "Contents/Helpers/seyal-runtime")
        XCTAssertEqual(BundledRuntimeLauncher.helperIdentifier, "dev.seyal.Seyal.runtime")
    }

    func testIsolatedRuntimeDirectoryDoesNotWeakenProductionDiscovery() {
        XCTAssertEqual(
            IsolatedRuntimeDirectory.helperArguments(from: ["Seyal"], testHostLoaded: false),
            []
        )
        XCTAssertEqual(
            IsolatedRuntimeDirectory.helperArguments(
                from: ["Seyal", "--runtime-dir", "/tmp/seyal-iso"],
                testHostLoaded: false
            ),
            ["--runtime-dir", "/tmp/seyal-iso"]
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
        let checkConnected = {
            if !connectedOnce, view.terminalBridgeIsConnected,
                view.terminalCurrentFrame() != nil,
                seyal_app_snapshot(pane.appHandle).eligibility
                    == UInt16(SEYAL_APP_ELIGIBILITY_FLOW.rawValue)
            {
                connectedOnce = true
                connected.fulfill()
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

    /// #673 `renderer_prepare_submission`: the production `--renderer-benchmark`
    /// path must write a five-cohort TOML file for the named Metal-submit
    /// boundary. This is not scanout / key-to-photon.
    @MainActor
    func testM002RendererPrepareSubmissionContractWritesCohort() throws {
        let sourceRoot = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .appendingPathComponent("Sources", isDirectory: true)
        let validation = try String(
            contentsOf: sourceRoot.appendingPathComponent("RendererValidation.swift"),
            encoding: .utf8
        )
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
        XCTAssertFalse(
            sawNewTab,
            "M001 default shell policy disallows tab creation; the command is omitted, not disabled"
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
        XCTAssertEqual(chrome.reserved & UInt32(SEYAL_APP_CHROME_INSPECTOR_VISIBLE), 0, "rejected select does not reveal")
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
