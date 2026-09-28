import XCTest

/// SPEC-024 K6 / #1138: headed XCUI evidence for the implemented keybinding catalog.
@MainActor
final class SeyalKeybindingUITests: XCTestCase {
    override func setUp() {
        continueAfterFailure = false
    }

    private func hostedApp() -> XCUIApplication {
        let app = XCUIApplication()
        app.launchIsolatedHost()
        return app
    }

    /// §14.12 / acceptance: projected menu titles are realized on the host menus.
    func testProjectedMenuTitlesArePresent() throws {
        let app = hostedApp()
        XCTAssertTrue(app.wait(for: .runningForeground, timeout: 10))
        waitForUsablePty(in: app)

        let commandPalette = app.menuItems["Command Palette"]
        XCTAssertTrue(
            commandPalette.waitForExistence(timeout: 5),
            "View → Command Palette must come from the Rust shortcut projection"
        )
        let newTab = app.menuItems["New Tab"]
        XCTAssertTrue(
            newTab.waitForExistence(timeout: 5),
            "File → New Tab must come from the Rust shortcut projection"
        )
    }

    /// §14.4 / acceptance: ⌘K ApplicationCommand opens the palette (zero PTY path).
    func testCommandKOpensPaletteWithoutLeavingFlow() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        assertFlowBlocksOrFail(in: app)

        let palette = app.descendants(matching: .any)["seyal-command-palette"]
        app.typeKey("k", modifierFlags: .command)
        XCTAssertTrue(palette.waitForExistence(timeout: 5), "⌘K must open the palette")
        assertFlowBlocksOrFail(in: app)
        app.typeKey(.escape, modifierFlags: [])
        let closed = expectation(
            for: NSPredicate(format: "exists == false"),
            evaluatedWith: palette,
            handler: nil
        )
        XCTAssertEqual(XCTWaiter.wait(for: [closed], timeout: 5), .completed)
    }

    /// §14.5: under TUI (alt-screen), Control-C and ArrowUp reach the terminal path.
    func testTuiControlCAndArrowReachPtyCapture() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        assertFlowBlocksOrFail(in: app)

        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("seyal-k6-pty-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
        let capture = directory.appendingPathComponent("captured.bin")
        let ready = directory.appendingPathComponent("ready")
        defer {
            do {
                try FileManager.default.removeItem(at: directory)
            } catch {
                XCTFail("Could not remove K6 PTY capture: \(error)")
            }
        }
        let script = """
        import os, select, sys, termios, time, tty
        fd = sys.stdin.fileno()
        saved = termios.tcgetattr(fd)
        data = bytearray()
        try:
            tty.setraw(fd)
            os.write(1, b"\\x1b[?1049h\\x1b[3;5HK6")
            open(\(String(reflecting: ready.path)), "wb").close()
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline:
                if not select.select([fd], [], [], max(0, deadline - time.monotonic()))[0]:
                    break
                chunk = os.read(fd, 64)
                if not chunk:
                    break
                data.extend(chunk)
                if b"\\x03" in data and len(data) >= 4:
                    break
        finally:
            try:
                with open(\(String(reflecting: capture.path)), "wb") as output:
                    output.write(data)
            finally:
                termios.tcsetattr(fd, termios.TCSANOW, saved)
                os.write(1, b"\\x1b[?1049l")
        """
        let scriptURL = directory.appendingPathComponent("receive.py")
        try script.write(to: scriptURL, atomically: true, encoding: .utf8)

        submitComposerCommand(app, "/usr/bin/python3 '\(scriptURL.path)'")
        let composer = app.descendants(matching: .any)["seyal-composer"].firstMatch
        let enteredTui = expectation(
            for: NSPredicate(format: "isHittable == false"),
            evaluatedWith: composer,
            handler: nil
        )
        XCTAssertEqual(XCTWaiter.wait(for: [enteredTui], timeout: 12), .completed)
        let readyWait = expectation(
            for: NSPredicate { _, _ in
                FileManager.default.fileExists(atPath: ready.path)
            },
            evaluatedWith: nil,
            handler: nil
        )
        XCTAssertEqual(XCTWaiter.wait(for: [readyWait], timeout: 8), .completed)

        let terminal = app.descendants(matching: .any)["terminal-input"].firstMatch
        XCTAssertTrue(terminal.waitForExistence(timeout: 5))
        terminal.click()
        defer {
            if app.state == .runningForeground, !composer.isHittable {
                app.typeKey("d", modifierFlags: .control)
            }
        }

        app.typeKey(.upArrow, modifierFlags: [])
        waitBriefly(0.2)
        app.typeKey("c", modifierFlags: .control)
        waitBriefly(0.4)
        app.typeKey("d", modifierFlags: .control)

        let returned = expectation(
            for: NSPredicate(format: "isHittable == true"),
            evaluatedWith: composer,
            handler: nil
        )
        XCTAssertEqual(XCTWaiter.wait(for: [returned], timeout: 12), .completed)

        let captured = try Data(contentsOf: capture)
        XCTAssertTrue(
            captured.contains(0x03),
            "TUI Control-C must reach PTY; bytes=\(captured.map { String(format: "%02x", $0) }.joined(separator: " "))"
        )
        // CSI CUU or SS3 up — either encoding proves the arrow hit the terminal path.
        let hasArrow =
            captured.contains(Data([0x1b, 0x5b, 0x41]))
            || captured.contains(Data([0x1b, 0x4f, 0x41]))
        XCTAssertTrue(
            hasArrow,
            "TUI ArrowUp must reach PTY; bytes=\(captured.map { String(format: "%02x", $0) }.joined(separator: " "))"
        )
        assertFlowBlocksOrFail(in: app)
    }

    /// Reserved Quit stays on the AppKit path (cmd+q terminates; not rebound).
    func testReservedQuitStillTerminates() throws {
        let app = hostedApp()
        XCTAssertTrue(app.wait(for: .runningForeground, timeout: 10))
        waitForUsablePty(in: app)
        app.typeKey("q", modifierFlags: .command)
        XCTAssertTrue(app.wait(for: .notRunning, timeout: 8))
    }

    private func submitComposerCommand(_ app: XCUIApplication, _ command: String) {
        let composer = app.descendants(matching: .any)["seyal-composer"]
        XCTAssertTrue(composer.waitForExistence(timeout: 12))
        let ready = expectation(
            for: NSPredicate(format: "value == 'available'"),
            evaluatedWith: composer,
            handler: nil
        )
        XCTAssertEqual(XCTWaiter.wait(for: [ready], timeout: 12), .completed)
        composer.firstMatch.click()
        let editor = app.descendants(matching: .any)["seyal-composer-editor"]
        if editor.waitForExistence(timeout: 2), editor.firstMatch.isHittable {
            editor.firstMatch.click()
            editor.firstMatch.typeText(command)
            editor.firstMatch.typeKey("\r", modifierFlags: [])
        } else {
            composer.firstMatch.typeText(command)
            app.typeKey("\r", modifierFlags: [])
        }
    }

    private func waitForUsablePty(in app: XCUIApplication, timeout: TimeInterval = 20) {
        XCTAssertTrue(app.wait(for: .runningForeground, timeout: 10))
        let terminal = app.descendants(matching: .any)["terminal-input"]
        XCTAssertTrue(terminal.waitForExistence(timeout: 10), "terminal surface missing")
        let usable = NSPredicate(format: "value CONTAINS 'connection=usable'")
        let arrived = expectation(for: usable, evaluatedWith: terminal, handler: nil)
        XCTAssertEqual(
            XCTWaiter.wait(for: [arrived], timeout: timeout),
            .completed,
            "PTY never became usable; value=\(terminal.value ?? "nil")"
        )
    }

    private func assertFlowBlocksOrFail(in app: XCUIApplication, timeout: TimeInterval = 8) {
        let composer = app.descendants(matching: .any)["seyal-composer"]
        let blocks = app.descendants(matching: .any)["seyal-blocks"]
        let transcript = app.descendants(matching: .any)["seyal-blocks-scroll"]
        XCTAssertTrue(
            composer.waitForExistence(timeout: timeout),
            "keybinding XCUI must stay on Flow/Blocks"
        )
        XCTAssertTrue(composer.firstMatch.isHittable, "composer must stay hittable on Flow")
        XCTAssertTrue(blocks.waitForExistence(timeout: timeout))
        XCTAssertTrue(transcript.waitForExistence(timeout: timeout))
    }

    private func waitBriefly(_ seconds: TimeInterval) {
        RunLoop.current.run(until: Date().addingTimeInterval(seconds))
    }
}
