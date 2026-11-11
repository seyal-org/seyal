import AppKit
import ImageIO
import XCTest
import Vision

/// Issue #923 headed evidence: with production pane splitting (C3), Split
/// Right/Down projects one region per Rust leaf, clicking a region focuses that
/// Pane, closing a non-last Pane removes its region, the surviving Pane still
/// offers hierarchical close, and exactly one live terminal surface exists,
/// inside the focused region. This file also covers #1219 zoom.
@MainActor
final class SeyalPaneSplitUITests: XCTestCase {
    override func setUp() {
        continueAfterFailure = false
    }

    private func hostedApp() -> XCUIApplication {
        let app = XCUIApplication()
        app.launchIsolatedHost()
        return app
    }

    func testSplitProjectsRegionsAndClickFocusesTheOtherPane() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        XCTAssertFalse(element(app, "seyal-pane-region-1").exists, "one Pane before splitting")

        click(app, "seyal-split-right")
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12), "Split Right projects a second region")
        XCTAssertTrue(element(app, "seyal-pane-divider-0").exists, "the Split projects its divider")
        XCTAssertEqual(app.state, .runningForeground, "splitting must not crash Seyal.app")
        let first = try focusedRegionIndex(app, count: 2)
        assertSingleLiveSurface(in: app, focusedRegion: first)

        let other = first == 0 ? 1 : 0
        element(app, "seyal-pane-region-\(other)").click()
        waitForFocus(app, region: other)
        XCTAssertEqual(try focusedRegionIndex(app, count: 2), other)
        assertSingleLiveSurface(in: app, focusedRegion: other)

        click(app, "seyal-split-down")
        XCTAssertTrue(element(app, "seyal-pane-region-2").waitForExistence(timeout: 12), "Split Down projects a third region")
        XCTAssertFalse(element(app, "seyal-pane-region-3").exists)
        let focused = try focusedRegionIndex(app, count: 3)
        assertSingleLiveSurface(in: app, focusedRegion: focused)
    }

    /// #1464: ⌘D from the composer must re-route first responder so typing
    /// appears in the visible pane editor without an extra click.
    func testComposerShowsTypedDraftImmediatelyAfterCommandDSplit() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        XCTAssertTrue(element(app, "seyal-composer").waitForExistence(timeout: 8))
        let editorBefore = element(app, "seyal-composer-editor")
        XCTAssertTrue(editorBefore.waitForExistence(timeout: 8))
        editorBefore.click()

        app.typeKey("d", modifierFlags: .command)
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12))
        XCTAssertTrue(
            element(app, "seyal-pane-presentation-host").waitForExistence(timeout: 8),
            "focused Flow leaf keeps a durable presentation host after ⌘D"
        )

        let marker = "cmd-d-visible-\(Int(Date().timeIntervalSince1970) % 100_000)"
        // Do not click the composer or pane — that masked #1464.
        app.typeText("echo \(marker)")

        let editor = element(app, "seyal-composer-editor")
        XCTAssertTrue(editor.waitForExistence(timeout: 8))
        let visibleDraft = expectation(
            for: NSPredicate(format: "value CONTAINS %@", marker),
            evaluatedWith: editor,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [visibleDraft], timeout: 5),
            .completed,
            "typed text must appear in the visible composer after ⌘D without clicking; value=\(editor.value ?? "nil")"
        )
        editor.typeKey("\r", modifierFlags: [])
        XCTAssertTrue(
            anyPaneBlock(in: app, containing: marker)
                .waitForExistence(timeout: 10),
            "Return must still submit the visible draft to Blocks"
        )
        XCTAssertEqual(app.state, .runningForeground)
    }

    func testSecondPaneComposerShowsTypedDraftBeforeSubmitAfterPriorCommands() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        submitComposerCommand("ls", in: app)
        submitComposerCommand("ls -lrt", in: app)

        click(app, "seyal-split-right")
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12))
        element(app, "seyal-pane-region-1").click()
        waitForFocus(app, region: 1)
        waitForUsablePty(in: app)

        let secondPane = element(app, "seyal-pane-region-1")
        let composer = element(app, "seyal-composer")
        XCTAssertTrue(composer.waitForExistence(timeout: 8))
        let editor = composer.descendants(matching: .any)
            .matching(identifier: "seyal-composer-editor")
            .firstMatch
        XCTAssertTrue(editor.waitForExistence(timeout: 8))
        XCTAssertTrue(secondPane.frame.insetBy(dx: -1, dy: -1).contains(editor.frame))
        let marker = "pane2-visible"
        let command = "echo \(marker)"
        app.typeText(command)

        let visibleDraft = expectation(
            for: NSPredicate(format: "value CONTAINS %@", marker),
            evaluatedWith: editor,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [visibleDraft], timeout: 5),
            .completed,
            "typed text must appear in the second-pane composer before Return; value=\(editor.value ?? "nil")"
        )
        let draftScreenshot = editor.screenshot()
        let attachment = XCTAttachment(screenshot: draftScreenshot)
        attachment.name = "Second pane composer draft before submit"
        attachment.lifetime = .keepAlways
        add(attachment)
        let recognizedDraft = try recognizedText(in: draftScreenshot)
            .split(whereSeparator: \.isWhitespace)
            .joined(separator: " ")
        XCTAssertTrue(
            recognizedDraft.contains(marker),
            "the second-pane draft must be visibly painted before Return; OCR=\(recognizedDraft)"
        )
        XCTAssertEqual(try focusedRegionIndex(app, count: 2), 1)

        editor.typeKey("\r", modifierFlags: [])
        XCTAssertTrue(
            anyPaneBlock(in: app, containing: marker)
                .waitForExistence(timeout: 10),
            "the visible draft must submit to the focused pane's Block timeline"
        )

        element(app, "seyal-pane-region-0").click()
        waitForFocus(app, region: 0)
        let unfocusedEditor = element(app, "seyal-secondary-composer-editor")
        XCTAssertTrue(unfocusedEditor.waitForExistence(timeout: 8))
        XCTAssertTrue(secondPane.frame.insetBy(dx: -1, dy: -1).contains(unfocusedEditor.frame))
        let unfocusedMarker = "pane2-unfocused"
        unfocusedEditor.click()
        app.typeText("echo \(unfocusedMarker)")
        waitForFocus(app, region: 1)
        let refocusedUnfocusedEditor = element(app, "seyal-composer-editor")
        XCTAssertTrue(refocusedUnfocusedEditor.waitForExistence(timeout: 8))
        let unfocusedDraft = expectation(
            for: NSPredicate(format: "value CONTAINS %@", unfocusedMarker),
            evaluatedWith: refocusedUnfocusedEditor,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [unfocusedDraft], timeout: 5),
            .completed,
            "clicking an unfocused pane composer must transfer focus without losing typing; value=\(refocusedUnfocusedEditor.value ?? "nil")"
        )
        XCTAssertEqual(try focusedRegionIndex(app, count: 2), 1)
        refocusedUnfocusedEditor.typeKey("\r", modifierFlags: [])
        XCTAssertTrue(
            anyPaneBlock(in: app, containing: unfocusedMarker).waitForExistence(timeout: 10),
            "typing into an unfocused pane's composer must submit to that pane"
        )

        let clickedMarker = "pane2-clicked"
        let primaryEditor = element(app, "seyal-composer-editor")
        XCTAssertTrue(primaryEditor.waitForExistence(timeout: 8))
        primaryEditor.click()
        primaryEditor.typeText("echo \(clickedMarker)")
        let clickedDraft = expectation(
            for: NSPredicate(format: "value CONTAINS %@", clickedMarker),
            evaluatedWith: primaryEditor,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [clickedDraft], timeout: 5),
            .completed,
            "clicking the focused pane editor must keep subsequent typing in that editor"
        )
        primaryEditor.typeKey("\r", modifierFlags: [])
        XCTAssertTrue(
            anyPaneBlock(in: app, containing: clickedMarker).waitForExistence(timeout: 10),
            "text typed after an explicit composer click must submit in the same pane"
        )
    }

    func testSecondPanePythonReceivesInputViaDirectInput() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        click(app, "seyal-split-right")
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12))
        element(app, "seyal-pane-region-1").click()
        waitForFocus(app, region: 1)
        waitForUsablePty(in: app)
        submitComposerCommand("python3 -q", in: app, expectBlock: false)
        XCTAssertEqual(try focusedRegionIndex(app, count: 2), 1)

        let secondPane = element(app, "seyal-pane-region-1")
        XCTAssertFalse(
            element(app, "seyal-running-program-interact").waitForExistence(timeout: 2),
            "DirectInput must not require Interact for Pane 2 python"
        )
        // Leaf hosts sit above regions in the view tree; locate by frame.
        let terminal = try waitForElement(
            app,
            matchingIdentifier: "terminal-input",
            in: secondPane,
            timeout: 8
        )
        let directRoute = expectation(
            for: NSPredicate { _, _ in terminal.elementType == .textView },
            evaluatedWith: nil,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [directRoute], timeout: 8),
            .completed,
            "Flow DirectInput must expose Pane 2 terminal input without Interact→Raw"
        )

        let pythonBlock = anyPaneBlock(in: app, containing: "python3")
        XCTAssertTrue(
            pythonBlock.waitForExistence(timeout: 12),
            "Pane 2 must keep the running python Block under DirectInput"
        )
        XCTAssertTrue(
            secondPane.frame.insetBy(dx: -1, dy: -1).contains(
                CGPoint(x: pythonBlock.frame.midX, y: pythonBlock.frame.midY)
            ),
            "python Block must sit in Pane 2"
        )
        let pythonBody = pythonBlock.descendants(matching: .any).matching(
            NSPredicate(format: "identifier ENDSWITH '-body'")
        ).firstMatch
        if pythonBody.waitForExistence(timeout: 5) {
            pythonBody.click()
        } else {
            terminal.click()
        }
        let evaluatedOutput = "seyalpy-ok"
        app.typeText("print('seyalpy'+'-ok')")
        app.typeKey("\r", modifierFlags: [])
        XCTAssertTrue(secondPane.frame.insetBy(dx: -1, dy: -1).contains(terminal.frame))
        let renderedText = try waitForRenderedText(evaluatedOutput, in: terminal)
        XCTAssertTrue(
            renderedText.contains(evaluatedOutput),
            "Python must evaluate the typed expression in Pane 2; the computed result is absent from the source; OCR=\(renderedText)"
        )
    }

    /// Headed #1467 regression: after Ctrl+C, Pane 2 python must keep
    /// DirectInput (composer stays retracted), then Ctrl+D (EOF) must exit the
    /// REPL and restore the Available composer — the multipane exit path users
    /// hit. Prefer classic REPL so pyrepl post-SIGINT quirks do not hide the
    /// DirectInput ownership contract under test.
    func testSecondPanePythonControlCThenControlDExitsDirectInput() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        click(app, "seyal-split-right")
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12))
        element(app, "seyal-pane-region-1").click()
        waitForFocus(app, region: 1)
        waitForUsablePty(in: app)
        // System Python 3.9 classic REPL: deterministic empty-line EOF after SIGINT.
        // mise/pyrepl (3.13+) can swallow a lone post-SIGINT EOF in some TERM setups.
        submitComposerCommand("/usr/bin/python3 -q", in: app, expectBlock: false)

        let secondPane = element(app, "seyal-pane-region-1")
        let terminal = try waitForElement(
            app,
            matchingIdentifier: "terminal-input",
            in: secondPane,
            timeout: 8
        )
        let directBefore = expectation(
            for: NSPredicate { _, _ in terminal.elementType == .textView },
            evaluatedWith: nil,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [directBefore], timeout: 8),
            .completed,
            "Pane 2 python must enter DirectInput before Ctrl+C"
        )
        let pythonBody = anyPaneBlock(in: app, containing: "python3")
            .descendants(matching: .any)
            .matching(NSPredicate(format: "identifier ENDSWITH '-body'"))
            .firstMatch
        if pythonBody.waitForExistence(timeout: 5) {
            pythonBody.click()
        } else {
            terminal.click()
        }

        app.typeKey("c", modifierFlags: .control)
        RunLoop.current.run(until: Date().addingTimeInterval(0.4))
        XCTAssertEqual(
            terminal.elementType,
            .textView,
            "first Ctrl+C must keep Pane 2 python DirectInput (composer must not steal keys)"
        )
        let composer = app.descendants(matching: .any)["seyal-composer"].firstMatch
        if composer.exists {
            XCTAssertFalse(
                composer.isHittable,
                "python DirectInput dock must stay retracted after the first Ctrl+C"
            )
        }

        // Re-click so XCUI Ctrl+D lands on the live surface after SIGINT.
        if pythonBody.exists {
            pythonBody.click()
        } else {
            terminal.click()
        }
        app.typeKey("d", modifierFlags: .control)

        // Match single-pane HostWorkload restore: editor exists + Available mode
        // (isHittable alone can lag while the dock is reclaiming focus).
        let editor = app.descendants(matching: .any)["seyal-composer-editor"].firstMatch
        XCTAssertTrue(
            editor.waitForExistence(timeout: 12),
            "Pane 2 composer dock must return after python Ctrl+C then Ctrl+D"
        )
        let available = expectation(
            for: NSPredicate(format: "value == %@", "available"),
            evaluatedWith: composer,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [available], timeout: 8),
            .completed,
            "Pane 2 python Ctrl+D must exit DirectInput and restore Available composer"
        )
        let leftDirectInput = expectation(
            for: NSPredicate { _, _ in terminal.elementType != .textView },
            evaluatedWith: nil,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [leftDirectInput], timeout: 8),
            .completed,
            "Pane 2 terminal must leave DirectInput textArea after python Ctrl+D"
        )
    }

    /// After the first Ctrl+C, Node still owns the PTY; DirectInput must not
    /// hand keys to an Available composer before the second Ctrl+C / Ctrl+D.
    func testSecondPaneNodeControlCKeepsDirectInputThenExits() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        click(app, "seyal-split-right")
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12))
        element(app, "seyal-pane-region-1").click()
        waitForFocus(app, region: 1)
        waitForUsablePty(in: app)
        submitComposerCommand("node", in: app, expectBlock: false)

        let secondPane = element(app, "seyal-pane-region-1")
        let terminal = try waitForElement(
            app,
            matchingIdentifier: "terminal-input",
            in: secondPane,
            timeout: 8
        )
        let directBefore = expectation(
            for: NSPredicate { _, _ in terminal.elementType == .textView },
            evaluatedWith: nil,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [directBefore], timeout: 8),
            .completed,
            "Pane 2 node must enter DirectInput before Ctrl+C"
        )
        let nodeBody = anyPaneBlock(in: app, containing: "node")
            .descendants(matching: .any)
            .matching(NSPredicate(format: "identifier ENDSWITH '-body'"))
            .firstMatch
        if nodeBody.waitForExistence(timeout: 5) {
            nodeBody.click()
        } else {
            terminal.click()
        }

        app.typeKey("c", modifierFlags: .control)
        RunLoop.current.run(until: Date().addingTimeInterval(0.4))

        XCTAssertEqual(
            terminal.elementType,
            .textView,
            "first Ctrl+C must keep Pane 2 DirectInput (composer must not steal keys)"
        )
        let composer = app.descendants(matching: .any)["seyal-composer"].firstMatch
        if composer.exists {
            XCTAssertFalse(
                composer.isHittable,
                "DirectInput dock must stay retracted after the first Ctrl+C"
            )
        }

        app.typeKey("d", modifierFlags: .control)
        let returned = expectation(
            for: NSPredicate(format: "isHittable == true"),
            evaluatedWith: composer,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [returned], timeout: 12),
            .completed,
            "Pane 2 node Ctrl+D must exit DirectInput and restore the composer"
        )
    }

    /// Multipane Flow DirectInput must deliver Ctrl+C (0x03) and Ctrl+D (0x04)
    /// to the focused Pane 2 PTY without Interact→Raw (#1467 headed regression).
    func testSecondPaneDirectInputControlCAndDReachPty() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        click(app, "seyal-split-right")
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12))
        element(app, "seyal-pane-region-1").click()
        waitForFocus(app, region: 1)
        waitForUsablePty(in: app)

        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("seyal-di-ctrl-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
        let capture = directory.appendingPathComponent("captured.bin")
        let ready = directory.appendingPathComponent("ready")
        defer {
            try? FileManager.default.removeItem(at: directory)
        }
        let script = """
        import os, select, sys, termios, time, tty
        fd = sys.stdin.fileno()
        saved = termios.tcgetattr(fd)
        data = bytearray()
        path = \(String(reflecting: capture.path))
        def flush():
            with open(path, "wb") as output:
                output.write(data)
        try:
            tty.setraw(fd)
            os.write(1, b"SEYAL-DI-CTRL-READY\\n")
            open(\(String(reflecting: ready.path)), "wb").close()
            flush()
            deadline = time.monotonic() + 20
            while time.monotonic() < deadline:
                if not select.select([fd], [], [], 0.25)[0]:
                    continue
                chunk = os.read(fd, 64)
                if not chunk:
                    break
                data.extend(chunk)
                flush()
                if 0x03 in data and 0x04 in data and b"hi" in data:
                    break
        finally:
            flush()
            termios.tcsetattr(fd, termios.TCSANOW, saved)
        """
        let scriptURL = directory.appendingPathComponent("receive.py")
        try script.write(to: scriptURL, atomically: true, encoding: .utf8)
        submitComposerCommand("/usr/bin/python3 '\(scriptURL.path)'", in: app, expectBlock: false)

        let secondPane = element(app, "seyal-pane-region-1")
        let terminal = try waitForElement(
            app,
            matchingIdentifier: "terminal-input",
            in: secondPane,
            timeout: 8
        )
        let directRoute = expectation(
            for: NSPredicate { _, _ in terminal.elementType == .textView },
            evaluatedWith: nil,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [directRoute], timeout: 8),
            .completed,
            "Flow DirectInput must expose Pane 2 terminal before Ctrl+C/D"
        )
        let readyWait = expectation(
            for: NSPredicate { _, _ in
                FileManager.default.fileExists(atPath: ready.path)
            },
            evaluatedWith: nil,
            handler: nil
        )
        XCTAssertEqual(XCTWaiter.wait(for: [readyWait], timeout: 8), .completed)
        let runningBody = anyPaneBlock(in: app, containing: "python3")
            .descendants(matching: .any)
            .matching(NSPredicate(format: "identifier ENDSWITH '-body'"))
            .firstMatch
        if runningBody.waitForExistence(timeout: 5) {
            runningBody.click()
        } else {
            terminal.click()
        }
        XCTAssertEqual(terminal.elementType, .textView, "Pane 2 must stay DirectInput before typing")

        // Prove FR/DirectInput with printable bytes, then SIGINT + EOF.
        app.typeText("hi")
        RunLoop.current.run(until: Date().addingTimeInterval(0.3))
        app.typeKey("c", modifierFlags: .control)
        RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        app.typeKey("d", modifierFlags: .control)

        let sawControls = expectation(
            for: NSPredicate { _, _ in
                guard let captured = try? Data(contentsOf: capture) else { return false }
                return captured.contains(0x03) && captured.contains(0x04)
            },
            evaluatedWith: nil,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [sawControls], timeout: 12),
            .completed,
            "Pane 2 DirectInput must deliver printable + Ctrl+C/D; bytes=\(String(describing: try? Data(contentsOf: capture).map { String(format: "%02x", $0) }.joined(separator: " ")))"
        )
    }

    /// Sibling Available must not clear the REPL pane's DirectInput; refocus
    /// must type again (headed multipane regression).
    func testDirectInputSurvivesFocusSwitchAwayAndBack() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        click(app, "seyal-split-right")
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12))
        element(app, "seyal-pane-region-1").click()
        waitForFocus(app, region: 1)
        waitForUsablePty(in: app)
        submitComposerCommand("python3 -q", in: app, expectBlock: false)

        let replPane = element(app, "seyal-pane-region-1")
        let otherPane = element(app, "seyal-pane-region-0")
        let terminalBefore = try waitForElement(
            app,
            matchingIdentifier: "terminal-input",
            in: replPane,
            timeout: 8
        )
        let directBefore = expectation(
            for: NSPredicate { _, _ in terminalBefore.elementType == .textView },
            evaluatedWith: nil,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [directBefore], timeout: 8),
            .completed,
            "REPL pane DirectInput must arm before the focus switch"
        )

        otherPane.click()
        waitForFocus(app, region: 0)
        XCTAssertTrue(anyPaneBlock(in: app, containing: "python3").waitForExistence(timeout: 8))
        // Unfocused REPL pane must retract its Busy dock (no Available/Busy chrome).
        let unfocusedReplComposer = app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier BEGINSWITH %@", "seyal-composer-")
        ).allElementsBoundByIndex.first { candidate in
            let frame = candidate.frame
            guard replPane.frame.insetBy(dx: -1, dy: -1)
                .contains(CGPoint(x: frame.midX, y: frame.midY))
            else { return false }
            let value = candidate.value as? String
            return value == "available" || value == "busy"
        }
        XCTAssertNil(
            unfocusedReplComposer,
            "unfocused DirectInput REPL must not expose an Available/Busy composer dock"
        )
        // Focused empty sibling must keep a visible composer (Available or
        // pre-prompt Busy) — never inherit the sibling's DirectInput hide.
        // Focused leaf uses primary AX id `seyal-composer` (not *-editor).
        let focusedEmptyComposer = try waitForElement(
            app,
            matchingIdentifier: "^seyal-composer(-[0-9a-f]+-[0-9a-f]+)?$",
            in: otherPane,
            timeout: 8
        )
        let emptyVisible = expectation(
            for: NSPredicate(format: "value == %@ OR value == %@", "available", "busy"),
            evaluatedWith: focusedEmptyComposer,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [emptyVisible], timeout: 5),
            .completed,
            "focused empty pane must keep a visible composer while the sibling REPL runs DirectInput; value=\(focusedEmptyComposer.value ?? "nil")"
        )

        replPane.click()
        waitForFocus(app, region: 1)
        let terminalAfter = try waitForElement(
            app,
            matchingIdentifier: "terminal-input",
            in: replPane,
            timeout: 8
        )
        let directAfter = expectation(
            for: NSPredicate { _, _ in terminalAfter.elementType == .textView },
            evaluatedWith: nil,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [directAfter], timeout: 8),
            .completed,
            "refocusing the REPL pane must restore Block-scoped DirectInput after sibling Available"
        )
        let pythonBody = anyPaneBlock(in: app, containing: "python3").descendants(matching: .any)
            .matching(NSPredicate(format: "identifier ENDSWITH '-body'")).firstMatch
        if pythonBody.waitForExistence(timeout: 5) {
            pythonBody.click()
        } else {
            terminalAfter.click()
        }
        let evaluatedOutput = "seyalfocus-ok"
        app.typeText("print('seyalfocus'+'-ok')")
        app.typeKey("\r", modifierFlags: [])
        let renderedText = try waitForRenderedText(evaluatedOutput, in: terminalAfter)
        XCTAssertTrue(
            renderedText.contains(evaluatedOutput),
            "python must accept keys after focus leaves and returns; OCR=\(renderedText)"
        )
    }

    /// node → then split must match split → then node: REPL dock retracts,
    /// new pane keeps Available composer (#1467 ordering asymmetry).
    func testNodeThenSplitKeepsDirectInputAndSiblingComposer() throws {
        guard FileManager.default.isExecutableFile(atPath: "/opt/homebrew/bin/node") else {
            throw XCTSkip("Node.js is not installed on this macOS test host")
        }
        let app = hostedApp()
        waitForUsablePty(in: app)
        submitComposerCommand("node", in: app, expectBlock: false)
        let terminalBeforeSplit = app.descendants(matching: .any)["terminal-input"].firstMatch
        XCTAssertTrue(terminalBeforeSplit.waitForExistence(timeout: 8))
        let directBefore = expectation(
            for: NSPredicate { _, _ in terminalBeforeSplit.elementType == .textView },
            evaluatedWith: nil,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [directBefore], timeout: 8),
            .completed,
            "single-pane node must enter Block-scoped DirectInput before split"
        )

        click(app, "seyal-split-right")
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12))
        // Split focuses the new leaf; original node pane is unfocused.
        waitForFocus(app, region: 1)
        let nodePane = element(app, "seyal-pane-region-0")
        let newPane = element(app, "seyal-pane-region-1")
        XCTAssertTrue(anyPaneBlock(in: app, containing: "node").waitForExistence(timeout: 12))

        let nodeComposerVisible = app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier BEGINSWITH %@", "seyal-composer-")
        ).allElementsBoundByIndex.contains { candidate in
            let frame = candidate.frame
            guard nodePane.frame.insetBy(dx: -1, dy: -1)
                .contains(CGPoint(x: frame.midX, y: frame.midY))
            else { return false }
            let value = candidate.value as? String
            return value == "available" || value == "busy"
        }
        XCTAssertFalse(
            nodeComposerVisible,
            "after node→split, the unfocused node pane must keep DirectInput dock retracted"
        )

        let newComposer = try waitForElement(
            app,
            matchingIdentifier: "^seyal-composer(-[0-9a-f]+-[0-9a-f]+)?$",
            in: newPane,
            timeout: 8
        )
        let newVisible = expectation(
            for: NSPredicate(format: "value == %@ OR value == %@", "available", "busy"),
            evaluatedWith: newComposer,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [newVisible], timeout: 5),
            .completed,
            "new pane after node→split must show a visible composer; value=\(newComposer.value ?? "nil")"
        )

        nodePane.click()
        waitForFocus(app, region: 0)
        let nodeTerminal = try waitForElement(
            app,
            matchingIdentifier: "terminal-input",
            in: nodePane,
            timeout: 8
        )
        let directAfter = expectation(
            for: NSPredicate { _, _ in nodeTerminal.elementType == .textView },
            evaluatedWith: nil,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [directAfter], timeout: 8),
            .completed,
            "refocusing the node pane after split must restore DirectInput"
        )
    }

    func testSecondPaneNodeReceivesInputWithoutSessionSetupPopup() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        click(app, "seyal-split-right")
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12))
        element(app, "seyal-pane-region-1").click()
        waitForFocus(app, region: 1)
        waitForUsablePty(in: app)
        submitComposerCommand("node", in: app, expectBlock: false)
        XCTAssertEqual(try focusedRegionIndex(app, count: 2), 1)

        let secondPane = element(app, "seyal-pane-region-1")
        XCTAssertFalse(
            element(app, "seyal-running-program-interact").waitForExistence(timeout: 2),
            "DirectInput must not require Interact for Pane 2 node"
        )
        XCTAssertFalse(
            app.descendants(matching: .any)["seyal-session-bootstrap-title"].exists,
            "a local Node REPL must not be interrupted by a session setup popup"
        )
        let terminal = try waitForElement(
            app,
            matchingIdentifier: "terminal-input",
            in: secondPane,
            timeout: 8
        )
        let directRoute = expectation(
            for: NSPredicate { _, _ in terminal.elementType == .textView },
            evaluatedWith: nil,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [directRoute], timeout: 8),
            .completed,
            "Flow DirectInput must expose Pane 2 terminal input without Interact→Raw"
        )

        let nodeBlock = anyPaneBlock(in: app, containing: "node")
        XCTAssertTrue(
            nodeBlock.waitForExistence(timeout: 12),
            "Pane 2 must keep the running node Block under DirectInput"
        )
        XCTAssertTrue(
            secondPane.frame.insetBy(dx: -1, dy: -1).contains(
                CGPoint(x: nodeBlock.frame.midX, y: nodeBlock.frame.midY)
            ),
            "node Block must sit in Pane 2"
        )
        let nodeBody = nodeBlock.descendants(matching: .any).matching(
            NSPredicate(format: "identifier ENDSWITH '-body'")
        ).firstMatch
        if nodeBody.waitForExistence(timeout: 5) {
            nodeBody.click()
        } else {
            terminal.click()
        }
        let evaluatedOutput = "seyalnode-ok"
        app.typeText("console.log('seyalnode'+'-ok')")
        app.typeKey("\r", modifierFlags: [])
        XCTAssertTrue(secondPane.frame.insetBy(dx: -1, dy: -1).contains(terminal.frame))
        let renderedText = try waitForRenderedText(evaluatedOutput, in: terminal)
        XCTAssertTrue(
            renderedText.contains(evaluatedOutput),
            "Node must evaluate the typed expression in Pane 2; the computed result is absent from the source; OCR=\(renderedText)"
        )
    }

    func testPaneTabOverflowNeverOverlapsNeighborOrTrailingActions() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)

        let newTerminal = app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier CONTAINS %@", "-new-terminal")
        ).firstMatch
        let tabs = app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier MATCHES %@", "^seyal-terminal-tab-[0-9]+-[0-9]+$")
        )
        XCTAssertTrue(newTerminal.waitForExistence(timeout: 8))
        XCTAssertEqual(tabs.count, 1, "fresh pane starts with one terminal tab")
        for expectedCount in 2...3 {
            newTerminal.click()
            let arrived = expectation(for: NSPredicate(format: "count == %d", expectedCount), evaluatedWith: tabs, handler: nil)
            XCTAssertEqual(
                XCTWaiter.wait(for: [arrived], timeout: 15),
                .completed,
                "terminal count after New Terminal is \(tabs.count), expected \(expectedCount)"
            )
        }

        click(app, "seyal-split-right")
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12))
        let strips = app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier CONTAINS %@", "-terminal-tabs")
        )
        XCTAssertEqual(strips.count, 2)
        let paneStrips = strips.allElementsBoundByIndex
        let screenshot = XCTAttachment(screenshot: app.screenshot())
        screenshot.name = "Pane-local tabs and trailing split toolbar"
        screenshot.lifetime = .keepAlways
        add(screenshot)
        XCTAssertFalse(paneStrips[0].frame.intersects(paneStrips[1].frame), "neighboring Pane strips remain in their own regions")

        let focusedStrips = paneStrips.filter { ($0.value as? String) == "focused" }
        XCTAssertEqual(focusedStrips.count, 1, "exactly one focused pane exposes split chrome")
        for strip in paneStrips {
            let splitRight = strip.buttons.matching(identifier: "seyal-split-right").firstMatch
            let splitDown = strip.buttons.matching(identifier: "seyal-split-down").firstMatch
            let isFocused = (strip.value as? String) == "focused"
            if isFocused {
                XCTAssertTrue(splitRight.exists, "focused strip exposes Split Right")
                XCTAssertTrue(splitDown.exists, "focused strip exposes Split Down")
                XCTAssertGreaterThan(splitRight.frame.minX, strip.frame.midX, "split controls sit on the Pane toolbar’s right side")
                XCTAssertLessThanOrEqual(splitRight.frame.maxX, splitDown.frame.minX, "split controls keep a consistent icon order")
                XCTAssertTrue(strip.frame.contains(splitRight.frame), "Split Right stays inside its Pane strip")
                XCTAssertTrue(strip.frame.contains(splitDown.frame), "Split Down stays inside its Pane strip")
            } else {
                XCTAssertFalse(splitRight.isHittable, "unfocused strip keeps Split Right out of the always-visible toolbar")
                XCTAssertFalse(splitDown.isHittable, "unfocused strip keeps Split Down out of the always-visible toolbar")
            }
            let tabViewport = strip.descendants(matching: .any).matching(
                identifier: "\(strip.identifier.replacingOccurrences(of: "-terminal-tabs", with: "-terminal-tab-viewport"))"
            ).firstMatch
            XCTAssertTrue(tabViewport.exists)
            XCTAssertTrue(strip.frame.contains(tabViewport.frame), "tab viewport stays inside its Pane strip")
            if isFocused {
                XCTAssertLessThanOrEqual(tabViewport.frame.maxX, splitRight.frame.minX - 8, "tab viewport reserves clear space before the toolbar")
            }
            let selectedTab = strip.buttons.matching(NSPredicate(format: "value == 'selected'")).firstMatch
            if selectedTab.exists {
                XCTAssertTrue(
                    tabViewport.frame.insetBy(dx: -2, dy: -1).contains(selectedTab.frame),
                    "selected terminal tab \(selectedTab.frame) is outside viewport \(tabViewport.frame)"
                )
            }
            XCTAssertFalse(strip.buttons.matching(identifier: "seyal-close-pane").firstMatch.exists,
                           "Close Pane stays out of the always-visible toolbar")
        }
    }

    /// #1424: Split Right keeps Block transcript + composer on both Flow panes.
    func testSplitProjectsBlocksAndComposerOnFocusedAndSecondaryPanes() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        XCTAssertTrue(element(app, "seyal-composer").waitForExistence(timeout: 8))
        XCTAssertTrue(element(app, "seyal-blocks").waitForExistence(timeout: 5))
        XCTAssertTrue(element(app, "seyal-blocks-scroll").waitForExistence(timeout: 5))

        click(app, "seyal-split-right")
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12))
        XCTAssertTrue(element(app, "seyal-secondary-live-pane").waitForExistence(timeout: 12))
        XCTAssertTrue(
            element(app, "seyal-pane-presentation-host").waitForExistence(timeout: 8),
            "focused Flow leaf keeps a durable presentation host"
        )
        XCTAssertTrue(element(app, "seyal-composer").exists, "focused Pane keeps seyal-composer")
        XCTAssertTrue(element(app, "seyal-blocks").exists, "focused Pane keeps Blocks")
        XCTAssertTrue(
            element(app, "seyal-secondary-blocks").waitForExistence(timeout: 8),
            "unfocused Flow pane must present Blocks, not Metal-only chrome"
        )
        XCTAssertTrue(element(app, "seyal-secondary-blocks-scroll").exists)
        let secondaryComposers = app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier BEGINSWITH %@", "seyal-composer-")
        )
        let appeared = expectation(
            for: NSPredicate(format: "count >= 1"),
            evaluatedWith: secondaryComposers,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [appeared], timeout: 8),
            .completed,
            "inactive Pane projects a subdued pane-scoped composer"
        )

        let other = (try focusedRegionIndex(app, count: 2)) == 0 ? 1 : 0
        element(app, "seyal-pane-region-\(other)").click()
        waitForFocus(app, region: other)
        XCTAssertTrue(element(app, "seyal-composer").waitForExistence(timeout: 5))
        XCTAssertTrue(element(app, "seyal-blocks").exists)
        XCTAssertTrue(element(app, "seyal-pane-presentation-host").waitForExistence(timeout: 5))
        XCTAssertTrue(element(app, "seyal-secondary-live-pane").waitForExistence(timeout: 5))
        XCTAssertTrue(element(app, "seyal-secondary-blocks").exists)

        let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        shot.name = "1424-dual-flow-blocks-composers"
        shot.lifetime = .keepAlways
        add(shot)
        XCTAssertEqual(app.state, .runningForeground)
    }

    /// Splitting a Pane creates an independent empty Block timeline; the source
    /// keeps its completed history without copying it into the new Pane.
    func testSplitStartsEmptyPaneWithoutCopyingSourceBlocks() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        XCTAssertTrue(element(app, "seyal-composer").waitForExistence(timeout: 8))

        let marker = "split-origin-\(String(UUID().uuidString.prefix(6)))"
        submitComposerCommand(
            "printf '\(marker)\\n'",
            in: app,
            expectBlock: true,
            blockMarker: marker
        )
        let sourceBlock = blockCommand(in: app, containing: marker)
        XCTAssertTrue(sourceBlock.waitForExistence(timeout: 8))
        waitForBodyMarker(blockBody(for: sourceBlock, in: app), marker: marker)

        click(app, "seyal-split-right")
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12))
        waitForFocus(app, region: 1)
        waitForUsablePty(in: app)
        XCTAssertTrue(element(app, "seyal-blocks").exists)

        let focusedPaneCopy = blockCommand(in: app, containing: marker)
        XCTAssertFalse(focusedPaneCopy.exists, "the new Pane must start with empty Block history")
        let retainedSource = app.descendants(matching: .any).matching(
            NSPredicate(
                format: "identifier BEGINSWITH %@ AND label CONTAINS %@",
                "seyal-secondary-block-",
                marker
            )
        ).firstMatch
        XCTAssertTrue(retainedSource.waitForExistence(timeout: 8))
        let retainedSourceBody = blockBody(for: retainedSource, in: app)
        waitForBodyMarker(
            retainedSourceBody,
            marker: marker,
            message: "the source Block body must remain available after the new Pane is bound"
        )
        let allCopies = app.descendants(matching: .any).matching(
            NSPredicate(
                format: "identifier MATCHES %@ AND label CONTAINS %@",
                "^seyal-(secondary-)?block-[0-9]+$",
                marker
            )
        )
        XCTAssertEqual(allCopies.count, 1, "only the source Pane retains its prior Block")
        XCTAssertEqual(app.state, .runningForeground)
    }

    /// Repeated command completion must leave previously rendered Block bodies
    /// present while the transcript publishes new geometry for the latest card.
    func testSequentialCommandsKeepEarlierBlockBodiesDuringTranscriptRefresh() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        XCTAssertTrue(element(app, "seyal-composer").waitForExistence(timeout: 8))

        var completedMarkers: [String] = []
        for index in 1...4 {
            let marker = "SEQUENTIAL_REFRESH_\(index)_\(String(UUID().uuidString.prefix(6)))"
            submitComposerCommand(
                "printf '\(marker)\\n'",
                in: app,
                blockMarker: marker
            )

            let block = anyPaneBlock(in: app, containing: marker)
            XCTAssertTrue(block.waitForExistence(timeout: 8))
            waitForBodyMarker(
                blockBody(for: block, in: app, expectedMarker: marker),
                marker: marker
            )
            completedMarkers.append(marker)

            for earlierMarker in completedMarkers.dropLast() {
                let earlier = anyPaneBlock(in: app, containing: earlierMarker)
                XCTAssertTrue(earlier.waitForExistence(timeout: 5))
                waitForBodyMarker(
                    blockBody(for: earlier, in: app, expectedMarker: earlierMarker),
                    marker: earlierMarker,
                    timeout: 5,
                    message: "adding a new Block must retain earlier transcript content"
                )
            }
        }

        let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        shot.name = "sequential-block-transcript-refresh"
        shot.lifetime = .keepAlways
        add(shot)
        XCTAssertEqual(app.state, .runningForeground)
    }

    /// A live Flow command must remain a Block when its pane becomes a
    /// secondary pane during a split. Repeated focus changes must keep the
    /// command and streamed output in that pane's transcript.
    func testRunningFlowBlockStaysInSecondaryPaneDuringFocusSwitches() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        XCTAssertTrue(element(app, "seyal-composer").waitForExistence(timeout: 8))

        // Keep the completed body under the bounded AX summary. The running
        // live tail is verified separately through its painted Metal region.
        let marker = "stream1426"
        submitComposerCommand(
            "for i in $(seq 1 12); do printf '\(marker) %02d\\n' \"$i\"; sleep 1; done; printf '\(marker) COMPLETE\\n'",
            in: app,
            expectBlock: true,
            blockMarker: marker
        )
        let runningBlock = anyPaneBlock(in: app, containing: marker)
        XCTAssertTrue(runningBlock.waitForExistence(timeout: 5))

        click(app, "seyal-split-right")
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12))
        let secondaryBlock = app.descendants(matching: .any).matching(
            NSPredicate(
                format: "identifier BEGINSWITH %@ AND label CONTAINS %@",
                "seyal-secondary-block-",
                marker
            )
        ).firstMatch
        XCTAssertTrue(
            secondaryBlock.waitForExistence(timeout: 8),
            "the running source pane must keep its Block when split focus moves to the sibling"
        )

        for _ in 0..<3 {
            element(app, "seyal-pane-region-0").click()
            waitForFocus(app, region: 0)
            element(app, "seyal-pane-region-1").click()
            waitForFocus(app, region: 1)
            XCTAssertTrue(
                secondaryBlock.exists,
                "the inactive source pane must retain its Block after focus switches"
            )
        }
        let secondaryBody = app.descendants(matching: .any)[
            "\(secondaryBlock.identifier)-body"
        ].firstMatch
        XCTAssertTrue(
            secondaryBody.waitForExistence(timeout: 5),
            "the secondary Block must retain its pane-local output body"
        )
        waitForBodyMarker(
            secondaryBody,
            marker: "\(marker) COMPLETE",
            timeout: 18,
            message: "the live command must finish and retain its output after focus changes"
        )
        for line in 1...12 {
            let expected = String(format: "%@ %02d", marker, line)
            XCTAssertTrue(
                secondaryBody.label.contains(expected),
                "the secondary Block retains streamed line \(line) after focus changes"
            )
        }
        XCTAssertEqual(app.state, .runningForeground)
    }

    /// #1426: fresh supported panes and both sides of a split present Blocks;
    /// the source pane retains its numbered live stream through focus changes.
    func testBlocksSplitKeepsUnfocusedStream() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        XCTAssertTrue(element(app, "seyal-composer").waitForExistence(timeout: 8))

        click(app, "seyal-split-right")
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12))
        waitForFocus(app, region: 1)
        waitForUsablePty(in: app)
        XCTAssertTrue(
            element(app, "seyal-composer").waitForExistence(timeout: 8),
            "a trusted split creates an independent Blocks pane"
        )
        XCTAssertTrue(element(app, "seyal-blocks").exists)

        let marker = "switchflow1426-\(String(UUID().uuidString.prefix(6)))"
        submitComposerCommand(
            "for i in $(seq 1 16); do printf '\(marker)-%02d\\n' \"$i\"; sleep 0.5; done",
            in: app,
            expectBlock: true,
            blockMarker: marker
        )
        let running = anyPaneBlock(in: app, containing: marker)
        XCTAssertTrue(running.waitForExistence(timeout: 6), "Flow command becomes a Block before exit")

        element(app, "seyal-pane-region-0").click()
        waitForFocus(app, region: 0)
        let inactiveBlock = app.descendants(matching: .any).matching(
            NSPredicate(
                format: "identifier BEGINSWITH %@ AND label CONTAINS %@",
                "seyal-secondary-block-",
                marker
            )
        ).firstMatch
        XCTAssertTrue(
            inactiveBlock.waitForExistence(timeout: 8),
            "an existing Pane's Flow selection must not become Raw when focus moves"
        )
        let body = blockBody(for: inactiveBlock, in: app)
        waitForBodyMarker(body, marker: "\(marker)-01")
        waitForBodyMarker(body, marker: "\(marker)-16")

        let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        shot.name = "1426-raw-split-blocks-remains-while-unfocused"
        shot.lifetime = .keepAlways
        add(shot)
        XCTAssertEqual(app.state, .runningForeground)
    }

    /// #1426: a sibling split from Flow stays Flow when focus moves away from
    /// it. Run a live numbered stream in that sibling, switch to the source
    /// Pane, and verify the secondary transcript retains every output line.
    func testFlowSplitSiblingStaysBlocksWhileUnfocusedDuringStream() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        XCTAssertTrue(element(app, "seyal-composer").waitForExistence(timeout: 8))

        click(app, "seyal-split-right")
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12))
        waitForFocus(app, region: 1)
        XCTAssertTrue(
            element(app, "seyal-pane-presentation-host").waitForExistence(timeout: 8),
            "a pane split from Flow must inherit the Blocks presentation"
        )
        XCTAssertTrue(element(app, "seyal-composer").exists)
        XCTAssertTrue(element(app, "seyal-blocks-scroll").exists)

        let marker = "splitstream1426"
        submitComposerCommand(
            "for i in $(seq 1 24); do printf '\(marker) %02d\\n' \"$i\"; sleep 1; done; printf '\(marker) COMPLETE\\n'",
            in: app,
            expectBlock: true,
            blockMarker: marker
        )
        let secondaryPane = element(app, "seyal-pane-region-0")
        secondaryPane.click()
        waitForFocus(app, region: 0)

        let secondaryBlocks = element(app, "seyal-secondary-blocks")
        XCTAssertTrue(
            secondaryBlocks.waitForExistence(timeout: 8),
            "the inactive Flow sibling must remain a Block transcript, not Raw"
        )
        let secondaryBlock = app.descendants(matching: .any).matching(
            NSPredicate(
                format: "identifier BEGINSWITH %@ AND label CONTAINS %@",
                "seyal-secondary-block-",
                marker
            )
        ).firstMatch
        XCTAssertTrue(secondaryBlock.waitForExistence(timeout: 8))
        let secondaryBody = app.descendants(matching: .any)[
            "\(secondaryBlock.identifier)-body"
        ].firstMatch
        XCTAssertTrue(secondaryBody.waitForExistence(timeout: 8))
        let runningComposer = app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier BEGINSWITH %@ AND value == %@", "seyal-composer-", "busy")
        ).firstMatch
        XCTAssertTrue(
            runningComposer.waitForExistence(timeout: 5),
            "the inactive pane remains in Blocks while its command is still running"
        )
        let inactiveStreamScreenshot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        inactiveStreamScreenshot.name = "1426-inactive-flow-block-during-numbered-stream"
        inactiveStreamScreenshot.lifetime = .keepAlways
        add(inactiveStreamScreenshot)
        let finalLine = "\(marker) COMPLETE"
        let streamFinished = expectation(
            for: NSPredicate(format: "label CONTAINS %@ OR value CONTAINS %@", finalLine, finalLine),
            evaluatedWith: secondaryBody,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [streamFinished], timeout: 35),
            .completed,
            "the unfocused Flow Block receives the end of the stream"
        )
        for line in 1...24 {
            let expected = String(format: "%@ %02d", marker, line)
            let containsLine = expectation(
                for: NSPredicate(format: "label CONTAINS %@ OR value CONTAINS %@", expected, expected),
                evaluatedWith: secondaryBody,
                handler: nil
            )
            XCTAssertEqual(
                XCTWaiter.wait(for: [containsLine], timeout: 8),
                .completed,
                "the unfocused Flow Block retains streamed line \(line)"
            )
        }
        let completedInactiveScreenshot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        completedInactiveScreenshot.name = "1426-complete-flow-block-while-inactive"
        completedInactiveScreenshot.lifetime = .keepAlways
        add(completedInactiveScreenshot)

        element(app, "seyal-pane-region-1").click()
        waitForFocus(app, region: 1)
        let completedBlock = anyPaneBlock(in: app, containing: marker)
        XCTAssertTrue(completedBlock.waitForExistence(timeout: 8))
        let completedStreamScreenshot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        completedStreamScreenshot.name = "1426-flow-block-after-stream-and-focus-return"
        completedStreamScreenshot.lifetime = .keepAlways
        add(completedStreamScreenshot)
        XCTAssertEqual(app.state, .runningForeground)
    }

    /// #1426: splitting the Pane that owns a live command must not lose its
    /// retained Block body when that Pane moves from the focused host to a
    /// secondary host. This exercises the source Pane during split, not only
    /// a sibling that was already idle when the split occurred.
    func testSplitFromLivePaneRetainsSourceBlockBody() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        XCTAssertTrue(element(app, "seyal-composer").waitForExistence(timeout: 8))

        click(app, "seyal-split-right")
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12))
        waitForFocus(app, region: 1)
        waitForUsablePty(in: app)
        let sourceExecution = try XCTUnwrap(
            executionIdentity(element(app, "seyal-pane-presentation-host").value as? String),
            "focused split source must have an execution identity"
        )

        let marker = "split-live-source-1426"
        submitComposerCommand(
            "for i in $(seq 1 32); do printf '\(marker)-%02d\\n' \"$i\"; sleep 0.25; done; printf '\(marker)-COMPLETE\\n'",
            in: app,
            expectBlock: true,
            blockMarker: marker
        )
        let liveBlock = anyPaneBlock(in: app, containing: marker)
        XCTAssertTrue(liveBlock.waitForExistence(timeout: 8))

        // Pane 2 is the split source. Splitting it while its command is live
        // turns it into an inactive leaf while the new Pane takes focus.
        click(app, "seyal-split-down")
        XCTAssertTrue(element(app, "seyal-pane-region-2").waitForExistence(timeout: 12))
        let sourcePane = element(app, "seyal-pane-region-1")
        sourcePane.click()
        waitForFocus(app, region: 1)

        let sourceBlock = anyPaneBlock(in: app, containing: marker)
        XCTAssertTrue(sourceBlock.waitForExistence(timeout: 8))
        let sourceBody = blockBody(for: sourceBlock, in: app)
        XCTAssertTrue(sourceBody.waitForExistence(timeout: 8))
        let completion = "\(marker)-COMPLETE"
        waitForBodyMarker(
            sourceBody,
            marker: completion,
            timeout: 15,
            message: "the split source Block must receive its final line after becoming a secondary Pane"
        )
        waitForBodyMarker(sourceBody, marker: "\(marker)-01")
        waitForBodyMarker(sourceBody, marker: "\(marker)-32")
        XCTAssertGreaterThan(
            sourceBody.frame.height,
            0,
            "the source Block output region must remain laid out and paintable after split"
        )

        let sourceScroll = element(app, "seyal-blocks-scroll")
        XCTAssertTrue(sourceScroll.exists)
        XCTAssertTrue(
            sourceBody.frame.intersects(sourceScroll.frame),
            "the retained source Block body must stay inside its visible transcript after the split resize"
        )
        XCTAssertTrue(
            sourceBlock.frame.intersects(sourceScroll.frame),
            "the source Block card itself must remain visible after the split resize; "
                + "block=\(sourceBlock.frame) transcript=\(sourceScroll.frame)"
        )
        let sourceHost = app.descendants(matching: .any).matching(
            NSPredicate(
                format: "(identifier == %@ OR identifier == %@) AND value CONTAINS %@",
                "seyal-pane-presentation-host",
                "seyal-secondary-live-pane",
                "execution=\(sourceExecution)"
            )
        ).firstMatch
        waitForPaintedHistoryRegion(sourceHost, timeout: 8)
        let screenshot = app.windows.firstMatch.screenshot()
        let shot = XCTAttachment(screenshot: screenshot)
        shot.name = "1426-live-split-source-retains-block-body"
        shot.lifetime = .keepAlways
        add(shot)
        let visibleLines = try recognizedText(in: screenshot)
        XCTAssertTrue(
            visibleLines.contains(completion),
            "the retained source Block must render visible output pixels after split; OCR=\(visibleLines)"
        )
        XCTAssertEqual(app.state, .runningForeground)
    }

    /// A Pane's terminal tabs own separate Runtime timelines. Their local
    /// Block ids may match, so switching executions must not reuse a sibling
    /// tab's command card or history body.
    func testTerminalTabBlocksStayBoundToTheirOwnExecution() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        XCTAssertTrue(element(app, "seyal-composer").waitForExistence(timeout: 8))

        let tokenA = "pane-a-\(String(UUID().uuidString.prefix(6)))"
        submitComposerCommand(
            "printf '\(tokenA)\\n'",
            in: app,
            expectBlock: true,
            blockMarker: tokenA
        )
        let blockA = anyPaneBlock(in: app, containing: tokenA)
        XCTAssertTrue(blockA.waitForExistence(timeout: 8))
        waitForBodyMarker(blockBody(for: blockA, in: app), marker: tokenA)

        click(app, "seyal-split-right")
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12))
        let paneBRegion = try focusedRegionIndex(app, count: 2)
        waitForUsablePty(in: app)

        let tokenB1 = "pane-b1-\(String(UUID().uuidString.prefix(6)))"
        submitComposerCommand(
            "printf '\(tokenB1)\\n'",
            in: app,
            expectBlock: true,
            blockMarker: tokenB1
        )
        let blockB1 = anyPaneBlock(in: app, containing: tokenB1)
        XCTAssertTrue(blockB1.waitForExistence(timeout: 8))
        waitForBodyMarker(blockBody(for: blockB1, in: app), marker: tokenB1)

        let paneRegion = element(app, "seyal-pane-region-\(paneBRegion)")
        let addButtons = app.buttons.matching(
            NSPredicate(format: "identifier CONTAINS %@", "-new-terminal")
        ).allElementsBoundByIndex
        guard let addTerminal = addButtons.first(where: { paneRegion.frame.contains($0.frame) }) else {
            XCTFail("Pane B must expose its pane-local New Terminal control")
            return
        }
        addTerminal.click()
        let tabTwo = app.buttons.matching(
            NSPredicate(format: "label == %@", "Pane \(paneBRegion + 1) · 2")
        ).firstMatch
        XCTAssertTrue(tabTwo.waitForExistence(timeout: 10), "Pane B's second terminal tab must exist")
        waitForUsablePty(in: app)

        let tokenB2 = "pane-b2-\(String(UUID().uuidString.prefix(6)))"
        submitComposerCommand(
            "printf '\(tokenB2)\\n'",
            in: app,
            expectBlock: true,
            blockMarker: tokenB2
        )
        let blockB2 = anyPaneBlock(in: app, containing: tokenB2)
        XCTAssertTrue(blockB2.waitForExistence(timeout: 8))
        waitForBodyMarker(blockBody(for: blockB2, in: app), marker: tokenB2)

        let tabOne = app.buttons.matching(
            NSPredicate(format: "label == %@", "Pane \(paneBRegion + 1) · 1")
        ).firstMatch
        XCTAssertTrue(tabOne.waitForExistence(timeout: 8))
        tabOne.click()

        let selectedB1 = element(app, "seyal-block-0")
        XCTAssertTrue(
            selectedB1.waitForExistence(timeout: 8),
            "selected terminal 1 must keep a Block card"
        )
        let titleUpdated = expectation(
            for: NSPredicate(format: "label CONTAINS %@", tokenB1),
            evaluatedWith: selectedB1,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [titleUpdated], timeout: 6),
            .completed,
            "selected terminal 1 must show its own command title; actual=\(selectedB1.label)"
        )
        XCTAssertFalse(selectedB1.label.contains(tokenB2), "terminal 1 command title must not bleed from terminal 2")
        let selectedBodyB1 = blockBody(for: selectedB1, in: app)
        waitForBodyMarker(selectedBodyB1, marker: tokenB1)
        XCTAssertFalse(
            "\(selectedBodyB1.label) \(selectedBodyB1.value ?? "")".contains(tokenB2),
            "terminal 1 history body must not contain terminal 2 output"
        )

        element(app, "seyal-pane-region-0").click()
        waitForFocus(app, region: 0)
        let inactiveA = anyPaneBlock(in: app, containing: tokenA)
        XCTAssertTrue(inactiveA.exists, "Pane A history stays visible when Pane B changes terminal tabs")
        waitForBodyMarker(blockBody(for: inactiveA, in: app), marker: tokenA)

        let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        shot.name = "1426-terminal-tab-block-isolation"
        shot.lifetime = .keepAlways
        add(shot)
        XCTAssertEqual(app.state, .runningForeground)
    }

    /// #1424 scenario 2: focus churn keeps host/Block content and scroll mid-transcript.
    func testFocusChurnRetainsScrolledTranscriptAndPaneHosts() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        // Split first so region projection is established before tall transcripts.
        click(app, "seyal-split-right")
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12))
        waitForUsablePty(in: app)
        XCTAssertTrue(element(app, "seyal-secondary-blocks-scroll").waitForExistence(timeout: 8))
        XCTAssertTrue(element(app, "seyal-pane-presentation-host").waitForExistence(timeout: 8))

        let focusedIdx = try focusedRegionIndex(app, count: 2)
        let regions = (0..<2).map { element(app, "seyal-pane-region-\($0)") }
        submitComposerCommand(
            "printf '%s\\n' $(seq 1 96) 'seyal-1424-scroll-a'",
            in: app
        )
        let other = focusedIdx == 0 ? 1 : 0
        // Seed the newly focused sibling with its own tall transcript.
        regions[other].click()
        waitForFocus(app, region: other)
        waitForUsablePty(in: app)
        submitComposerCommand(
            "printf '%s\\n' $(seq 1 96) 'seyal-1424-scroll-b'",
            in: app
        )

        let primaryScroll = element(app, "seyal-blocks-scroll")
        let secondaryScroll = element(app, "seyal-secondary-blocks-scroll")
        XCTAssertTrue(primaryScroll.exists)
        XCTAssertTrue(secondaryScroll.exists)
        let marker = "seyal-1424-scroll-b"
        let bodyBeforeScroll = blockBody(for: anyPaneBlock(in: app, containing: marker), in: app)
        XCTAssertTrue(bodyBeforeScroll.waitForExistence(timeout: 8))
        waitForBodyMarker(bodyBeforeScroll, marker: marker)
        XCTAssertGreaterThan(
            bodyBeforeScroll.frame.height,
            primaryScroll.frame.height,
            "the transcript must be taller than its viewport before testing scroll retention"
        )
        let bodyOffsetBeforeScroll = bodyBeforeScroll.frame.minY - regions[other].frame.minY
        // 96 lines overflow the Pane viewport, so this must move actual Block
        // content instead of scrolling a short transcript with no overflow.
        primaryScroll.scroll(byDeltaX: 0, deltaY: 420)
        let bodyAfterScroll = blockBody(for: anyPaneBlock(in: app, containing: marker), in: app)
        let bodyOffsetAfterScroll = bodyAfterScroll.frame.minY - regions[other].frame.minY
        XCTAssertGreaterThan(
            abs(bodyOffsetAfterScroll - bodyOffsetBeforeScroll),
            80,
            "scrolling an overflowing transcript must move its Block body within its Pane"
        )
        let blockA = anyPaneBlock(in: app, containing: "seyal-1424-scroll-a")
        let blockB = anyPaneBlock(in: app, containing: "seyal-1424-scroll-b")
        XCTAssertTrue(blockA.exists || blockB.exists)

        // Switching to the sibling and back must preserve the scrolled Block's
        // location within its Pane, regardless of primary/secondary host role.
        regions[focusedIdx].click()
        waitForFocus(app, region: focusedIdx)
        regions[other].click()
        waitForFocus(app, region: other)
        XCTAssertTrue(element(app, "seyal-pane-presentation-host").exists)
        XCTAssertTrue(element(app, "seyal-secondary-live-pane").exists)
        XCTAssertTrue(element(app, "seyal-composer").exists)
        XCTAssertTrue(element(app, "seyal-blocks").exists)
        XCTAssertTrue(element(app, "seyal-secondary-blocks").exists)
        let bodyAfterFocus = blockBody(for: anyPaneBlock(in: app, containing: marker), in: app)
        let bodyOffsetAfterFocus = bodyAfterFocus.frame.minY - regions[other].frame.minY
        XCTAssertEqual(
            bodyOffsetAfterFocus,
            bodyOffsetAfterScroll,
            accuracy: 48,
            "Pane B's mid-transcript scroll position must survive focus ownership churn"
        )
        let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        shot.name = "1424-focus-churn-scroll-retained"
        shot.lifetime = .keepAlways
        add(shot)
        XCTAssertEqual(app.state, .runningForeground)
    }

    /// #1424 scenario 3: unfocused pane keeps updating the same Block while the sibling is used.
    func testUnfocusedPaneStreamKeepsUpdatingSameBlock() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        XCTAssertTrue(element(app, "seyal-composer").waitForExistence(timeout: 8))
        let token = "stream-marker-\(String(UUID().uuidString.prefix(6)))"
        click(app, "seyal-split-right")
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12))
        waitForUsablePty(in: app)
        // Start the stream on the focused pane, then move focus to the sibling.
        // Long stream so late lines arrive after we move focus to the sibling.
        submitComposerCommand(
            "for i in $(seq 1 40); do printf '%s-%s\\n' '\(token)' \"$i\"; sleep 0.15; done",
            in: app,
            expectBlock: true,
            blockMarker: token
        )
        let streamBlock = anyPaneBlock(in: app, containing: token)
        XCTAssertTrue(streamBlock.waitForExistence(timeout: 12), "stream Block must appear")
        let streamBody = blockBody(for: streamBlock, in: app)
        XCTAssertTrue(streamBody.waitForExistence(timeout: 8))
        let heightBefore = streamBody.frame.height
        let lateMarker = "\(token)-35"

        let focused = try focusedRegionIndex(app, count: 2)
        // Prefer focusing the non-streaming pane so the stream stays visible but unfocused.
        let sibling = focused == 0 ? 1 : 0
        element(app, "seyal-pane-region-\(sibling)").click()
        waitForFocus(app, region: sibling)
        waitForUsablePty(in: app)
        submitComposerCommand("printf 'seyal-1424-sibling\\n'", in: app)
        XCTAssertTrue(
            anyPaneBlock(in: app, containing: "seyal-1424-sibling").waitForExistence(timeout: 10)
        )

        let unfocusedStream = anyPaneBlock(in: app, containing: token)
        XCTAssertTrue(unfocusedStream.exists, "unfocused stream Block must remain after focus move")
        let unfocusedBody = blockBody(for: unfocusedStream, in: app)
        let progressed = expectation(
            for: NSPredicate { _, _ in
                guard unfocusedBody.exists else { return false }
                if unfocusedBody.frame.height > heightBefore + 8 { return true }
                let label = (unfocusedBody.label as String?) ?? ""
                let value = (unfocusedBody.value as? String) ?? ""
                return label.contains(lateMarker) || value.contains(lateMarker)
            },
            evaluatedWith: nil,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [progressed], timeout: 12),
            .completed,
            "unfocused stream must keep updating the same Block; before=\(heightBefore) after=\(unfocusedBody.frame.height) label=\(unfocusedBody.label) value=\(unfocusedBody.value ?? "nil")"
        )
        XCTAssertTrue(
            unfocusedStream.identifier.hasPrefix("seyal-secondary-block-")
                || unfocusedStream.identifier.hasPrefix("seyal-block-"),
            "stream must stay on a Block surface, id=\(unfocusedStream.identifier)"
        )
        let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        shot.name = "1424-unfocused-stream-same-block"
        shot.lifetime = .keepAlways
        add(shot)
        XCTAssertEqual(app.state, .runningForeground)
    }

    /// #1426: completed Block history stays on the producing pane after focus moves.
    /// Handle-addressed history FFI must not blank or swap bodies via the focused display.
    func testCompletedBlockHistoryStaysOnProducingUnfocusedPane() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        XCTAssertTrue(element(app, "seyal-composer").waitForExistence(timeout: 12))
        let markerA = "seyal-1426-hist-a-\(String(UUID().uuidString.prefix(6)))"
        let markerB = "seyal-1426-hist-b-\(String(UUID().uuidString.prefix(6)))"
        click(app, "seyal-split-right")
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12))
        waitForUsablePty(in: app)
        let paneARegion = try focusedRegionIndex(app, count: 2)

        submitComposerCommand(
            "printf '\(markerA)\\n'",
            in: app,
            expectBlock: true,
            blockMarker: markerA
        )
        XCTAssertTrue(
            anyPaneBlock(in: app, containing: markerA).waitForExistence(timeout: 12),
            "pane A completed Block must appear"
        )
        let bodyA = blockBody(for: anyPaneBlock(in: app, containing: markerA), in: app)
        XCTAssertTrue(bodyA.waitForExistence(timeout: 8))
        let heightA = bodyA.frame.height

        let sibling = paneARegion == 0 ? 1 : 0
        element(app, "seyal-pane-region-\(sibling)").click()
        waitForFocus(app, region: sibling)
        waitForUsablePty(in: app)
        submitComposerCommand(
            "printf '\(markerB)\\n'",
            in: app,
            expectBlock: true,
            blockMarker: markerB
        )
        XCTAssertTrue(
            anyPaneBlock(in: app, containing: markerB).waitForExistence(timeout: 12),
            "pane B completed Block must appear"
        )

        let bodyB = blockBody(for: anyPaneBlock(in: app, containing: markerB), in: app)
        XCTAssertTrue(bodyB.waitForExistence(timeout: 8), "pane B completed body must appear")
        // Focus ownership retags the retained card and body from primary to
        // secondary AX identifiers. Reacquire Pane A by its output marker so
        // this assertion remains pane-bound after the focus switch.
        let inactiveBlockA = anyPaneBlock(in: app, containing: markerA)
        XCTAssertTrue(inactiveBlockA.waitForExistence(timeout: 8))
        let inactiveBodyA = blockBody(for: inactiveBlockA, in: app)
        waitForBodyMarker(inactiveBodyA, marker: markerA)
        waitForBodyMarker(bodyB, marker: markerB)

        let unfocusedA = app.descendants(matching: .any).matching(
            NSPredicate(
                format: "identifier BEGINSWITH %@ AND label CONTAINS %@",
                "seyal-secondary-block-",
                markerA
            )
        ).firstMatch
        XCTAssertTrue(
            unfocusedA.waitForExistence(timeout: 8),
            "pane A's inactive transcript must remain a visible Flow Block host"
        )
        let unfocusedBodyA = blockBody(for: unfocusedA, in: app)
        XCTAssertTrue(unfocusedBodyA.exists, "pane A Block body must stay bound")
        XCTAssertGreaterThan(
            unfocusedBodyA.frame.height,
            0,
            "pane A history body must not blank via focused-handle routing"
        )
        XCTAssertEqual(
            unfocusedBodyA.frame.height,
            heightA,
            accuracy: 12,
            "pane A completed body height must stay stable after sibling history bind"
        )
        XCTAssertTrue(
            anyPaneBlock(in: app, containing: markerB).exists,
            "pane B Block must not steal or erase pane A history"
        )

        // Exercise the actual focus boundary repeatedly. The focused leaf is
        // also hosted by SecondaryLivePaneHost in multipane Flow; both
        // completed bodies must stay in their producing pane as each leaf
        // alternates between focused and unfocused ownership.
        for cycle in 0..<10 {
            let target = cycle % 2 == 0 ? paneARegion : sibling
            element(app, "seyal-pane-region-\(target)").click()
            waitForFocus(app, region: target)
            let paneAPrefix = target == paneARegion ? "seyal-block-" : "seyal-secondary-block-"
            let paneBPrefix = target == sibling ? "seyal-block-" : "seyal-secondary-block-"
            let blockA = app.descendants(matching: .any).matching(
                NSPredicate(format: "identifier BEGINSWITH %@ AND label CONTAINS %@", paneAPrefix, markerA)
            ).firstMatch
            let blockB = app.descendants(matching: .any).matching(
                NSPredicate(format: "identifier BEGINSWITH %@ AND label CONTAINS %@", paneBPrefix, markerB)
            ).firstMatch
            XCTAssertTrue(
                blockA.waitForExistence(timeout: 8),
                "cycle \(cycle): pane A command stays on its own Flow transcript"
            )
            XCTAssertTrue(
                blockB.waitForExistence(timeout: 8),
                "cycle \(cycle): pane B command stays on its own Flow transcript"
            )
            let currentBodyA = blockBody(for: blockA, in: app)
            let currentBodyB = blockBody(for: blockB, in: app)
            waitForBodyMarker(currentBodyA, marker: markerA)
            waitForBodyMarker(currentBodyB, marker: markerB)
            let textA = "\(currentBodyA.label) \(currentBodyA.value ?? "")"
            let textB = "\(currentBodyB.label) \(currentBodyB.value ?? "")"
            XCTAssertFalse(textA.contains(markerB), "cycle \(cycle): pane A never shows pane B output")
            XCTAssertFalse(textB.contains(markerA), "cycle \(cycle): pane B never shows pane A output")
        }

        let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        shot.name = "1426-completed-history-on-producing-pane"
        shot.lifetime = .keepAlways
        add(shot)
        XCTAssertEqual(app.state, .runningForeground)
    }

    /// #1424 scenario 4: divider drag keeps dual Flow Blocks+composers.
    func testDividerDragKeepsDualFlowBlocksAndComposers() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        click(app, "seyal-split-right")
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12))
        waitForUsablePty(in: app)
        let focused = try focusedRegionIndex(app, count: 2)
        submitComposerCommand("printf 'seyal-1424-div-a\\n'", in: app)
        let other = focused == 0 ? 1 : 0
        element(app, "seyal-pane-region-\(other)").click()
        waitForFocus(app, region: other)
        waitForUsablePty(in: app)
        submitComposerCommand("printf 'seyal-1424-div-b\\n'", in: app)

        let divider = element(app, "seyal-pane-divider-0")
        XCTAssertTrue(divider.waitForExistence(timeout: 8))
        let leading = element(app, "seyal-pane-region-0")
        let trailing = element(app, "seyal-pane-region-1")
        let area = leading.frame.union(trailing.frame)
        let start = divider.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
        let end = start.withOffset(CGVector(dx: area.minX + area.width * 0.35 - divider.frame.midX, dy: 0))
        start.press(forDuration: 0.2, thenDragTo: end, withVelocity: 300, thenHoldForDuration: 0.3)

        XCTAssertTrue(element(app, "seyal-composer").waitForExistence(timeout: 5))
        XCTAssertTrue(element(app, "seyal-blocks").exists)
        XCTAssertTrue(element(app, "seyal-secondary-blocks").exists)
        XCTAssertTrue(element(app, "seyal-secondary-blocks-scroll").exists)
        XCTAssertFalse(
            app.descendants(matching: .any).matching(
                NSPredicate(format: "identifier BEGINSWITH %@", "seyal-composer-")
            ).allElementsBoundByIndex.isEmpty
        )
        XCTAssertTrue(anyPaneBlock(in: app, containing: "seyal-1424-div-a").exists)
        XCTAssertTrue(anyPaneBlock(in: app, containing: "seyal-1424-div-b").exists)
        let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        shot.name = "1424-divider-dual-flow-retained"
        shot.lifetime = .keepAlways
        add(shot)
        XCTAssertEqual(app.state, .runningForeground)
    }

    /// #1424 scenario 5: disposable vim TUI enter/exit restores Blocks+composer.
    func testVimTuiEnterExitRestoresFlowBlocksAndComposer() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        XCTAssertTrue(element(app, "seyal-composer").waitForExistence(timeout: 8))
        // TUI entry can fence Flow Blocks immediately; do not require a Block card.
        submitComposerCommand("vim -Nu NONE -n", in: app, expectBlock: false)

        let composer = element(app, "seyal-composer")
        let terminal = app.descendants(matching: .any)["terminal-input"].firstMatch
        // TUI should take the live surface; composer becomes non-hittable / Hidden.
        let tuiArrived = expectation(
            for: NSPredicate(format: "isHittable == false"),
            evaluatedWith: composer,
            handler: nil
        )
        let tuiResult = XCTWaiter.wait(for: [tuiArrived], timeout: 12)
        if tuiResult != .completed {
            // Some shells surface vim under Raw with a still-visible composer;
            // require a usable live terminal either way.
            XCTAssertTrue(terminal.waitForExistence(timeout: 5), "vim must attach a live surface")
        }
        let tuiShot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        tuiShot.name = "1424-vim-tui-active"
        tuiShot.lifetime = .keepAlways
        add(tuiShot)

        if terminal.exists, terminal.isHittable {
            terminal.click()
        }
        // Quit vim: Escape then :q!
        app.typeKey(.escape, modifierFlags: [])
        RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        app.typeText(":q!")
        app.typeKey("\r", modifierFlags: [])

        let restored = expectation(
            for: NSPredicate(format: "isHittable == true"),
            evaluatedWith: composer,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [restored], timeout: 15),
            .completed,
            "leaving vim must restore hittable Flow composer"
        )
        XCTAssertTrue(element(app, "seyal-blocks").waitForExistence(timeout: 8))
        XCTAssertTrue(element(app, "seyal-blocks-scroll").exists)
        let editor = element(app, "seyal-composer-editor")
        XCTAssertTrue(editor.waitForExistence(timeout: 5))
        editor.click()
        let token = "after-vim-marker-1424"
        editor.typeText(token)
        XCTAssertTrue(
            NSPredicate(format: "value CONTAINS %@", token).evaluate(with: editor),
            "post-vim keys must enter the composer"
        )
        let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        shot.name = "1424-vim-tui-restored-flow"
        shot.lifetime = .keepAlways
        add(shot)
        XCTAssertEqual(app.state, .runningForeground)
    }

    /// #1424 scenario 6: resize/reflow keeps dual Flow chrome without blank-canvas collapse.
    func testResizeReflowKeepsDualFlowHostsWithoutBlankCanvas() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        click(app, "seyal-split-right")
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12))
        waitForUsablePty(in: app)
        XCTAssertTrue(element(app, "seyal-secondary-blocks").waitForExistence(timeout: 8))
        submitComposerCommand("printf '%s\\n' $(seq 1 12) 'seyal-1424-resize'", in: app)

        let window = app.windows.firstMatch
        XCTAssertTrue(window.waitForExistence(timeout: 5))
        let original = window.frame
        let origin = window.coordinate(withNormalizedOffset: CGVector(dx: 1, dy: 1))
        origin.click()
        origin.press(
            forDuration: 0.05,
            thenDragTo: origin.withOffset(CGVector(dx: -160, dy: -90))
        )
        RunLoop.current.run(until: Date().addingTimeInterval(0.4))
        origin.press(
            forDuration: 0.05,
            thenDragTo: origin.withOffset(CGVector(dx: 120, dy: 60))
        )
        RunLoop.current.run(until: Date().addingTimeInterval(0.4))

        let focused = try focusedRegionIndex(app, count: 2)
        element(app, "seyal-pane-region-\(focused)").click()
        waitForUsablePty(in: app)
        submitComposerCommand("printf 'seyal-1424-reflow\\n'", in: app)

        XCTAssertTrue(element(app, "seyal-composer").exists)
        XCTAssertTrue(element(app, "seyal-blocks").exists)
        XCTAssertTrue(element(app, "seyal-secondary-blocks").exists)
        XCTAssertTrue(element(app, "seyal-secondary-blocks-scroll").exists)
        XCTAssertTrue(element(app, "seyal-pane-presentation-host").exists)
        XCTAssertTrue(element(app, "seyal-secondary-live-pane").exists)
        XCTAssertTrue(anyPaneBlock(in: app, containing: "seyal-1424-resize").exists)
        XCTAssertTrue(anyPaneBlock(in: app, containing: "seyal-1424-reflow").exists)
        let secondaryScroll = element(app, "seyal-secondary-blocks-scroll")
        XCTAssertLessThan(
            secondaryScroll.frame.height,
            max(original.height, window.frame.height),
            "secondary scroll clip stays inside the window"
        )
        let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        shot.name = "1424-resize-reflow-dual-flow"
        shot.lifetime = .keepAlways
        add(shot)
        XCTAssertEqual(app.state, .runningForeground)
    }

    func testPaneLocalTerminalTabsSelectIndependentlyAndNewTerminalDoesNotSplit() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        XCTAssertFalse(
            element(app, "seyal-tab-strip").exists,
            "outer Seyal Tab chrome must not appear as a second terminal tab row"
        )
        let paneStrip = app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier CONTAINS %@", "-terminal-tabs")
        ).firstMatch
        let splitRight = element(app, "seyal-split-right")
        let splitDown = element(app, "seyal-split-down")
        XCTAssertTrue(
            splitRight.exists,
            "Split actions live with the focused Pane's local terminal tabs"
        )
        XCTAssertTrue(splitDown.exists)
        XCTAssertGreaterThan(splitRight.frame.minX, paneStrip.frame.minX)
        XCTAssertLessThan(splitRight.frame.maxX, paneStrip.frame.maxX)
        XCTAssertEqual(splitRight.frame.midY, splitDown.frame.midY, accuracy: 1)
        let terminalSurface = app.descendants(matching: .any)["terminal-input"].firstMatch
        let originalExecution = try XCTUnwrap(executionIdentity(terminalSurface.value as? String))
        let terminalTabs = app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier MATCHES %@", "^seyal-terminal-tab-[0-9]+-[0-9]+$")
        )
        XCTAssertEqual(terminalTabs.count, 1, "fresh Workspace has one terminal tab in its first Pane")

        let newTerminal = app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier CONTAINS %@", "-new-terminal")
        ).firstMatch
        XCTAssertTrue(newTerminal.waitForExistence(timeout: 8), "focused Pane must expose New Terminal")
        let newTerminalID = newTerminal.identifier
        XCTAssertFalse(newTerminalID.isEmpty, "New Terminal must advertise a stable AX id")
        click(app, newTerminalID)
        let twoTabs = expectation(for: NSPredicate(format: "count == 2"), evaluatedWith: terminalTabs, handler: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [twoTabs], timeout: 20), .completed, "New Terminal adds a pane-local tab")
        XCTAssertFalse(element(app, "seyal-pane-region-1").exists, "New Terminal does not split the PaneTree")
        waitForUsablePty(in: app, timeout: 20)

        let original = terminalTabs.element(boundBy: 0)
        let created = terminalTabs.element(boundBy: 1)
        let originalTabID = original.identifier
        let createdTabID = created.identifier
        XCTAssertNotEqual(originalTabID, createdTabID)
        original.click()
        XCTAssertEqual(original.value as? String, "selected", "selection returns to the original live terminal")
        XCTAssertEqual(created.value as? String, "", "the other terminal remains unselected")
        XCTAssertEqual(
            try XCTUnwrap(executionIdentity(terminalSurface.value as? String)),
            originalExecution,
            "the focused terminal surface returns to the original execution"
        )
        created.click()
        XCTAssertEqual(created.value as? String, "selected")
        let createdExecution = try XCTUnwrap(executionIdentity(terminalSurface.value as? String))
        XCTAssertNotEqual(createdExecution, originalExecution, "the selected terminal surface follows the selected tab")
        original.click()
        XCTAssertEqual(
            try XCTUnwrap(executionIdentity(terminalSurface.value as? String)),
            originalExecution,
            "switching back restores the original execution without replacing it"
        )

        // Split while the AX tree is still light (before Block markers). Seeding
        // Blocks first stalled multipane region queries on CI (#1424).
        click(app, "seyal-split-right")
        XCTAssertTrue(
            element(app, "seyal-pane-region-1").waitForExistence(timeout: 12),
            "Split Right projects a second Pane after pane-local tabs"
        )
        let secondPaneStrips = app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier CONTAINS %@ AND identifier CONTAINS %@", "-terminal-tabs", "seyal-pane-")
        )
        let stripsReady = expectation(
            for: NSPredicate(format: "count == 2"),
            evaluatedWith: secondPaneStrips,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [stripsReady], timeout: 12),
            .completed,
            "each Pane owns its own terminal-tab strip"
        )
        waitForFocus(app, region: 1)
        waitForUsablePty(in: app)
        let focusedHost = element(app, "seyal-pane-presentation-host")
        XCTAssertTrue(focusedHost.waitForExistence(timeout: 8))
        let secondPaneExecution = try XCTUnwrap(executionIdentity(focusedHost.value as? String))
        XCTAssertNotEqual(secondPaneExecution, originalExecution, "Split creates an independent terminal execution")
        XCTAssertNotEqual(secondPaneExecution, createdExecution, "split Pane does not reuse either terminal tab execution")
        XCTAssertEqual(terminalTabs.count, 3, "two terminal tabs stay in Pane 1 and Pane 2 starts with one")
        submitComposerCommand("echo SEYAL_PANE_TWO_MARKER", in: app)

        element(app, "seyal-pane-region-0").click()
        waitForFocus(app, region: 0)
        waitForUsablePty(in: app)
        let pane0Host = element(app, "seyal-pane-presentation-host")
        XCTAssertTrue(pane0Host.waitForExistence(timeout: 8))
        // Pane 1 still has the original tab selected from before the split.
        XCTAssertEqual(
            try XCTUnwrap(executionIdentity(pane0Host.value as? String)),
            originalExecution,
            "returning to Pane 1 preserves its selected terminal"
        )
        app.buttons.matching(identifier: originalTabID).firstMatch.click()
        waitForSelectedTerminalTab(app, identifier: originalTabID)
        waitForPaneExecution(app, presentationID: "seyal-pane-presentation-host", execution: originalExecution)
        waitForUsablePty(in: app)
        submitComposerCommand("echo SEYAL_TAB_ONE_MARKER", in: app)
        app.buttons.matching(identifier: createdTabID).firstMatch.click()
        waitForSelectedTerminalTab(app, identifier: createdTabID)
        waitForPaneExecution(app, presentationID: "seyal-pane-presentation-host", execution: createdExecution)
        waitForUsablePty(in: app)
        submitComposerCommand("echo SEYAL_TAB_TWO_MARKER", in: app)
        app.buttons.matching(identifier: originalTabID).firstMatch.click()
        waitForSelectedTerminalTab(app, identifier: originalTabID)
        waitForPaneExecution(app, presentationID: "seyal-pane-presentation-host", execution: originalExecution)
        waitForUsablePty(in: app)
        waitForAnyPaneBlock("SEYAL_TAB_ONE_MARKER", in: app, expected: true)
        waitForAnyPaneBlock("SEYAL_TAB_TWO_MARKER", in: app, expected: false)
        app.buttons.matching(identifier: createdTabID).firstMatch.click()
        waitForSelectedTerminalTab(app, identifier: createdTabID)
        waitForPaneExecution(app, presentationID: "seyal-pane-presentation-host", execution: createdExecution)
        waitForUsablePty(in: app)
        waitForAnyPaneBlock("SEYAL_TAB_TWO_MARKER", in: app, expected: true)
        waitForAnyPaneBlock("SEYAL_TAB_ONE_MARKER", in: app, expected: false)
        // Multipane Flow keeps Pane 2 Blocks visible as secondary chrome (#1424).
        // Assert isolation against the focused Pane's primary Block ids only.
        XCTAssertFalse(
            blockCommand(in: app, containing: "SEYAL_PANE_TWO_MARKER").exists,
            "Pane 2 command history stays out of focused Pane 1 Blocks"
        )

        element(app, "seyal-pane-region-1").click()
        waitForFocus(app, region: 1)
        let pane1Host = element(app, "seyal-pane-presentation-host")
        XCTAssertTrue(pane1Host.waitForExistence(timeout: 8))
        XCTAssertEqual(
            try XCTUnwrap(executionIdentity(pane1Host.value as? String)),
            secondPaneExecution,
            "returning to Pane 2 restores its independent execution"
        )
        XCTAssertTrue(anyPaneBlock(in: app, containing: "SEYAL_PANE_TWO_MARKER").exists)

        XCTAssertEqual(app.state, .runningForeground)
    }

    /// Load acceptance for the split-created Blocks workspace: two axes,
    /// multiple terminal tabs in every Pane, concurrent numbered streams, and
    /// repeated focus changes must preserve each Pane's live Block body.
    func testFourPaneBlocksLoadWithMultipleTerminalTabsAndFocusChurn() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        XCTAssertTrue(element(app, "seyal-composer").waitForExistence(timeout: 8))

        // Build a 2×2 grid through both split directions.
        splitPane(app, "seyal-pane-region-0", direction: "right")
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12))
        splitPane(app, "seyal-pane-region-1", direction: "down")
        XCTAssertTrue(element(app, "seyal-pane-region-2").waitForExistence(timeout: 12))
        splitPane(app, "seyal-pane-region-0", direction: "down")
        XCTAssertTrue(element(app, "seyal-pane-region-3").waitForExistence(timeout: 12))
        waitForExactlyOneFocusedRegion(app, count: 4)

        let regions = (0..<4).map { element(app, "seyal-pane-region-\($0)") }
        let xCenters = Set(regions.map { Int($0.frame.midX.rounded()) })
        let yCenters = Set(regions.map { Int($0.frame.midY.rounded()) })
        XCTAssertEqual(xCenters.count, 2, "vertical split must create two columns")
        XCTAssertEqual(yCenters.count, 2, "horizontal splits must create two rows")
        XCTAssertEqual(
            Set(regions.map { "\(Int($0.frame.midX.rounded()))/\(Int($0.frame.midY.rounded()))" }).count,
            4,
            "the four regions must occupy all four 2×2 grid positions"
        )
        let strips = app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier CONTAINS %@", "-terminal-tabs")
        )
        let stripsReady = expectation(for: NSPredicate(format: "count == 4"), evaluatedWith: strips, handler: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [stripsReady], timeout: 12), .completed)

        // Give each independently provisioned Pane a second TerminalTab and
        // prove that tab's command history stays local before returning to the
        // original execution for the concurrent stream load.
        var secondTabMarkers: [Int: String] = [:]
        var secondTabIDs: [Int: String] = [:]
        for strip in strips.allElementsBoundByIndex {
            guard let paneIndex = regions.firstIndex(where: { $0.frame.contains(strip.frame) }) else {
                XCTFail("terminal tab strip is outside all four Pane regions: \(strip.frame)")
                continue
            }
            regions[paneIndex].click()
            waitForFocus(app, region: paneIndex)
            let original = strip.buttons.matching(
                NSPredicate(format: "identifier MATCHES %@", "^seyal-terminal-tab-[0-9]+-[0-9]+$")
            ).firstMatch
            XCTAssertTrue(original.waitForExistence(timeout: 5))
            let originalTabID = original.identifier
            strip.buttons.matching(
                NSPredicate(format: "identifier CONTAINS %@", "-new-terminal")
            ).firstMatch.click()
            waitForUsablePty(in: app, timeout: 20)
            let tabButtons = strip.buttons.matching(
                NSPredicate(format: "identifier MATCHES %@", "^seyal-terminal-tab-[0-9]+-[0-9]+$")
            )
            guard let secondTab = tabButtons.allElementsBoundByIndex.first(where: {
                $0.identifier != originalTabID
            }) else {
                XCTFail("new terminal tab was not provisioned for Pane \(paneIndex)")
                continue
            }
            secondTabIDs[paneIndex] = secondTab.identifier
            let marker = "MPTAB_\(paneIndex)"
            secondTabMarkers[paneIndex] = marker
            submitComposerCommand("printf '\(marker)\\n'", in: app, blockMarker: marker)
            strip.buttons.matching(identifier: originalTabID).firstMatch.click()
            waitForUsablePty(in: app, timeout: 20)
        }
        let terminalTabs = app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier MATCHES %@", "^seyal-terminal-tab-[0-9]+-[0-9]+$")
        )
        let tabsReady = expectation(for: NSPredicate(format: "count == 8"), evaluatedWith: terminalTabs, handler: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [tabsReady], timeout: 12), .completed)

        let markers = (0..<4).map { "MP\($0)" }
        for index in 0..<4 {
            regions[index].click()
            waitForFocus(app, region: index)
            waitForUsablePty(in: app)
            submitComposerCommand(
                "for i in $(seq 1 100); do printf '\(markers[index])-%03d\\n' \"$i\"; sleep 0.15; done",
                in: app,
                blockMarker: markers[index]
            )
        }

        for turn in 0..<12 {
            let index = turn % 4
            regions[index].click()
            waitForFocus(app, region: index)
            XCTAssertTrue(element(app, "seyal-composer").exists)
            XCTAssertTrue(element(app, "seyal-blocks").exists)
        }

        // Verify every numbered line in each owning Block and exactly one AX
        // Block per unique token, detecting both lost output and pane bleed.
        // The Block AX summary retains the newest 240 characters. With these
        // short unique markers, rows 77...100 fit in that tail, including the
        // line lost by the headed focus-churn run at 62c10894.
        for (index, marker) in markers.enumerated() {
            let matchingBlocks = app.descendants(matching: .any).matching(
                NSPredicate(
                    format: "identifier MATCHES %@ AND label CONTAINS %@",
                    "^(seyal-block|seyal-secondary-block)-[0-9]+$",
                    marker
                )
            )
            XCTAssertTrue(matchingBlocks.firstMatch.waitForExistence(timeout: 10), marker)
            XCTAssertEqual(matchingBlocks.count, 1, "\(marker) must appear in one Pane only")
            let body = blockBody(for: matchingBlocks.firstMatch, in: app, expectedMarker: marker)
            let finalLine = String(format: "%@-%03d", marker, 100)
            waitForBodyMarker(
                body,
                marker: finalLine,
                timeout: 15,
                message: "Block history must publish the final stream line before checking its tail"
            )
            let scroll = try XCTUnwrap(
                element(
                    app,
                    matchingIdentifier: "^(seyal-blocks-scroll|seyal-secondary-blocks-scroll)$",
                    in: regions[index]
                ),
                "Pane \(index + 1) must own a visible Blocks transcript viewport"
            )
            let composer = try XCTUnwrap(
                element(app, matchingIdentifier: "^seyal-composer(?:-[0-9a-f]{16}-[0-9a-f]{16})?$", in: regions[index]),
                "Pane \(index + 1) must own its composer"
            )
            XCTAssertTrue(
                body.frame.intersects(scroll.frame),
                "Pane \(index + 1) live Block body must intersect its own transcript viewport; "
                    + "body=\(body.frame) scroll=\(scroll.frame)"
            )
            XCTAssertEqual(
                body.frame.maxY,
                scroll.frame.maxY,
                accuracy: 96,
                "Pane \(index + 1) live tail should sit at the bottom above its composer; "
                    + "composer=\(composer.frame) body=\(body.frame) scroll=\(scroll.frame)"
            )
            let bodyText = "\(body.label) \(body.value ?? "")"
            // The AX summary intentionally exposes only the newest 240
            // characters, so full-history retention is asserted by the
            // Runtime history projection and earlier smaller-stream tests.
            // Here verify the complete visible tail after focus churn.
            for line in 77...100 {
                let expected = String(format: "%@-%03d", marker, line)
                XCTAssertTrue(
                    bodyText.contains(expected),
                    "missing streamed line \(expected), body=\(bodyText)"
                )
            }

            let secondTab = try XCTUnwrap(secondTabMarkers[index])
            let strip = try XCTUnwrap(strips.allElementsBoundByIndex.first {
                regions[index].frame.contains($0.frame)
            })
            let tabs = strip.buttons.matching(
                NSPredicate(format: "identifier MATCHES %@", "^seyal-terminal-tab-[0-9]+-[0-9]+$")
            )
            XCTAssertEqual(tabs.count, 2)
            regions[index].click()
            waitForFocus(app, region: index)
            let secondTabID = try XCTUnwrap(secondTabIDs[index])
            strip.buttons.matching(identifier: secondTabID).firstMatch.click()
            waitForSelectedTerminalTab(strip, identifier: secondTabID)
            waitForUsablePty(in: app)
            waitForAnyPaneBlock(secondTab, in: app, expected: true)
            waitForAnyPaneBlock(marker, in: app, expected: false)
            let originalTabID = try XCTUnwrap(
                tabs.allElementsBoundByIndex.first(where: { $0.identifier != secondTabID })?.identifier
            )
            strip.buttons.matching(identifier: originalTabID).firstMatch.click()
            waitForSelectedTerminalTab(strip, identifier: originalTabID)
            waitForUsablePty(in: app)
            waitForAnyPaneBlock(marker, in: app, expected: true)
            waitForAnyPaneBlock(secondTab, in: app, expected: false)
            let restoredHost = try XCTUnwrap(
                element(
                    app,
                    matchingIdentifier: "^(seyal-pane-presentation-host|seyal-secondary-live-pane)$",
                    in: regions[index]
                )
            )
            waitForPaintedHistoryRegion(restoredHost, timeout: 8)
            let paneScreenshot = app.windows.firstMatch.screenshot()
            let paneShot = XCTAttachment(screenshot: paneScreenshot)
            paneShot.name = "1426-pane-\(index + 1)-after-terminal-tab-restore"
            paneShot.lifetime = .keepAlways
            add(paneShot)
            let visibleLines = try recognizedText(in: paneScreenshot)
            XCTAssertTrue(
                visibleLines.contains("\(marker)-100"),
                "Pane \(index + 1) must visibly render its stream tail in the headed screenshot; "
                    + "OCR=\(visibleLines)"
            )
        }

        // Pane 2 and Pane 4 own separate transcript scroll positions. Scroll
        // them by different amounts, then revisit both after focus changes.
        let liveGrid = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        liveGrid.name = "1426-four-pane-live-blocks-above-composers"
        liveGrid.lifetime = .keepAlways
        add(liveGrid)

        var scrolledOffsets: [Int: CGFloat] = [:]
        let paneScrollDeltas: [(Int, CGFloat)] = [(1, 260), (3, 520)]
        for (paneIndex, deltaY) in paneScrollDeltas {
            regions[paneIndex].click()
            waitForFocus(app, region: paneIndex)
            let marker = markers[paneIndex]
            let block = app.descendants(matching: .any).matching(
                NSPredicate(
                    format: "identifier MATCHES %@ AND label CONTAINS %@",
                    "^(seyal-block|seyal-secondary-block)-[0-9]+$",
                    marker
                )
            ).firstMatch
            XCTAssertTrue(block.waitForExistence(timeout: 8), "Pane \(paneIndex + 1) Block exists")
            let body = blockBody(for: block, in: app, expectedMarker: marker)
            let before = body.frame.minY - regions[paneIndex].frame.minY
            element(app, "seyal-blocks-scroll").scroll(byDeltaX: 0, deltaY: deltaY)
            let after = body.frame.minY - regions[paneIndex].frame.minY
            XCTAssertGreaterThan(
                abs(after - before),
                80,
                "Pane \(paneIndex + 1) scroll must move its own Block body"
            )
            scrolledOffsets[paneIndex] = after
        }
        XCTAssertGreaterThan(
            abs(try XCTUnwrap(scrolledOffsets[1]) - XCTUnwrap(scrolledOffsets[3])),
            100,
            "Pane 2 and Pane 4 must retain distinct, independent scroll positions"
        )
        for paneIndex in [1, 3] {
            regions[paneIndex].click()
            waitForFocus(app, region: paneIndex)
            let marker = markers[paneIndex]
            let paneBlock = app.descendants(matching: .any).matching(
                NSPredicate(
                    format: "identifier MATCHES %@ AND label CONTAINS %@",
                    "^(seyal-block|seyal-secondary-block)-[0-9]+$",
                    marker
                )
            ).firstMatch
            XCTAssertTrue(paneBlock.waitForExistence(timeout: 8), "Pane \(paneIndex + 1) Block remains visible")
            let body = blockBody(for: paneBlock, in: app, expectedMarker: marker)
            XCTAssertEqual(
                body.frame.minY - regions[paneIndex].frame.minY,
                try XCTUnwrap(scrolledOffsets[paneIndex]),
                accuracy: 48,
                "Pane \(paneIndex + 1) must restore its own scroll offset"
            )
        }
        XCTAssertEqual(app.state, .runningForeground)
    }

    func testClosePaneLivesInContextMenuWithoutToolbarIcon() throws {
        let app = hostedApp()
        let strip = app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier CONTAINS %@", "-terminal-tabs")
        ).firstMatch
        XCTAssertTrue(strip.waitForExistence(timeout: 5))
        XCTAssertFalse(element(app, "seyal-close-pane").exists,
                        "Close Pane is not an always-visible toolbar icon")
        strip.rightClick()
        XCTAssertTrue(app.menuItems["Close Pane"].waitForExistence(timeout: 5),
                      "Close Pane remains available in the Pane contextual menu")
        XCTAssertEqual(app.state, .runningForeground)
    }

    /// #1219 headed evidence: ⌘⇧↩ (`pane.zoom_toggle`) shows the focused leaf
    /// full-Tab without destroying its sibling; a second ⌘⇧↩ restores the split.
    func testZoomToggleFillsTabAndRestoresSplit() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        click(app, "seyal-split-right")
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12))
        XCTAssertTrue(element(app, "seyal-pane-divider-0").exists)
        if element(app, "seyal-pane-region-1").value as? String != "focused" {
            app.typeKey(.rightArrow, modifierFlags: [.command, .option])
        }
        waitForFocus(app, region: 1)
        let sibling = element(app, "seyal-secondary-live-pane")
        XCTAssertTrue(sibling.waitForExistence(timeout: 12), "bound sibling has a live surface before zoom")
        let tab = element(app, "seyal-pane-region-0").frame.union(element(app, "seyal-pane-region-1").frame)
        let splitFrame = element(app, "seyal-pane-region-1").frame

        app.typeKey("\r", modifierFlags: [.command, .shift])
        waitForAbsence(element(app, "seyal-pane-region-0"), "zoom hides the occluded region")
        waitForAbsence(sibling, "the occluded leaf presents no surface")
        XCTAssertFalse(element(app, "seyal-pane-divider-0").exists, "no divider while zoomed")
        XCTAssertEqual(element(app, "seyal-pane-region-1").value as? String, "focused")
        assertFrame(element(app, "seyal-pane-region-1").frame, matches: tab, "zoomed leaf fills the Tab")
        assertSingleLiveSurface(in: app, focusedRegion: 1)
        XCTAssertEqual(app.state, .runningForeground)

        app.typeKey("\r", modifierFlags: [.command, .shift])
        XCTAssertTrue(element(app, "seyal-pane-region-0").waitForExistence(timeout: 8), "unzoom restores the sibling region")
        XCTAssertTrue(element(app, "seyal-pane-divider-0").waitForExistence(timeout: 8))
        XCTAssertTrue(sibling.waitForExistence(timeout: 12), "the sibling execution survived zoom")
        XCTAssertEqual(try focusedRegionIndex(app, count: 2), 1)
        assertFrame(element(app, "seyal-pane-region-1").frame, matches: splitFrame, "split ratio unchanged")
        waitForUsablePty(in: app)
    }

    // MARK: - Assertions

    private func focusedRegionIndex(_ app: XCUIApplication, count: Int) throws -> Int {
        let focused = (0..<count).filter { element(app, "seyal-pane-region-\($0)").value as? String == "focused" }
        XCTAssertEqual(focused.count, 1, "exactly one focused region among \(count)")
        return try XCTUnwrap(focused.first)
    }

    /// Focused Flow presentation sits inside the focused region. Multipane
    /// Flow (#1424) owns chrome through durable pane hosts; Raw/TUI may still
    /// expose a single `terminal-input` inside liveContent.
    private func assertSingleLiveSurface(in app: XCUIApplication, focusedRegion index: Int) {
        let region = element(app, "seyal-pane-region-\(index)").frame
        let host = element(app, "seyal-pane-presentation-host")
        if host.waitForExistence(timeout: 4) {
            XCTAssertTrue(
                region.insetBy(dx: -1, dy: -1).contains(host.frame),
                "presentation host \(host.frame) must sit inside focused region \(region)"
            )
            return
        }
        let terminals = app.descendants(matching: .any).matching(identifier: "terminal-input")
        let live = expectation(for: NSPredicate(format: "count == 1"), evaluatedWith: terminals, handler: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [live], timeout: 12), .completed, "exactly one live terminal surface")
        let terminal = terminals.firstMatch.frame
        XCTAssertTrue(
            region.insetBy(dx: -1, dy: -1).contains(terminal),
            "live surface \(terminal) must sit inside focused region \(region)"
        )
    }

    // MARK: - Helpers

    private func element(_ app: XCUIApplication, _ identifier: String) -> XCUIElement {
        app.descendants(matching: .any)[identifier].firstMatch
    }

    private func element(
        _ app: XCUIApplication,
        matchingIdentifier identifierPattern: String,
        in region: XCUIElement
    ) -> XCUIElement? {
        let regionFrame = region.frame
        return app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier MATCHES %@", identifierPattern)
        ).allElementsBoundByIndex.first { candidate in
            let frame = candidate.frame
            return regionFrame.insetBy(dx: -1, dy: -1)
                .contains(CGPoint(x: frame.midX, y: frame.midY))
        }
    }

    private func waitForElement(
        _ app: XCUIApplication,
        matchingIdentifier identifierPattern: String,
        in region: XCUIElement,
        timeout: TimeInterval
    ) throws -> XCUIElement {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if let match = element(app, matchingIdentifier: identifierPattern, in: region) {
                return match
            }
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        }
        return try XCTUnwrap(
            element(app, matchingIdentifier: identifierPattern, in: region),
            "expected \(identifierPattern) inside \(region.identifier)"
        )
    }

    private func click(_ app: XCUIApplication, _ identifier: String) {
        let matches = app.descendants(matching: .any).matching(identifier: identifier)
        XCTAssertTrue(matches.firstMatch.waitForExistence(timeout: 8), "\(identifier) is shown")
        // Prefer a laid-out hittable match when duplicate ids exist (per-pane
        // strips). Zero-frame firstMatch clicks spawned extra Seyal windows and
        // stranded Block markers on the wrong surface (#1474).
        var control = matches.firstMatch
        let count = matches.count
        if count > 1 {
            for index in 0..<count {
                let candidate = matches.element(boundBy: index)
                let frame = candidate.frame
                if candidate.exists, candidate.isHittable, frame.width > 1, frame.height > 1 {
                    control = candidate
                    break
                }
            }
        }
        let framed = expectation(
            for: NSPredicate { _, _ in
                let frame = control.frame
                return frame.width > 1 && frame.height > 1
            },
            evaluatedWith: nil,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [framed], timeout: 4),
            .completed,
            "\(identifier) must have a non-zero frame before click; frame=\(control.frame)"
        )
        control.click()
    }

    private func splitPane(_ app: XCUIApplication, _ paneIdentifier: String, direction: String) {
        let region = element(app, paneIdentifier)
        XCTAssertTrue(region.waitForExistence(timeout: 8), "\(paneIdentifier) is shown")
        // Split chrome is focus-scoped (C05). Focus the target leaf first so the
        // strip under this region actually exposes seyal-split-*.
        if (region.value as? String) != "focused" {
            region.click()
            let suffix = paneIdentifier.replacingOccurrences(of: "seyal-pane-region-", with: "")
            if let index = Int(suffix) {
                waitForFocus(app, region: index)
            }
        }
        guard let strip = app.descendants(matching: .any)
            .matching(NSPredicate(format: "identifier CONTAINS %@", "-terminal-tabs"))
            .allElementsBoundByIndex.first(where: {
                let frame = $0.frame
                return region.frame.contains(CGPoint(x: frame.midX, y: frame.midY))
                    || region.frame.intersects(frame)
            })
        else {
            XCTFail("terminal tab strip must belong to \(paneIdentifier)")
            return
        }
        let button = strip.buttons.matching(identifier: "seyal-split-\(direction)").firstMatch
        XCTAssertTrue(button.waitForExistence(timeout: 8), "Split \(direction) is shown in \(paneIdentifier)")
        XCTAssertTrue(button.isHittable, "Split \(direction) must be hittable on the focused strip")
        button.click()
    }

    private func waitForFocus(_ app: XCUIApplication, region index: Int) {
        let region = element(app, "seyal-pane-region-\(index)")
        let deadline = Date().addingTimeInterval(8)
        var currentValue: String?
        while Date() < deadline {
            currentValue = region.value as? String
            if currentValue == "focused" { return }
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        }
        currentValue = region.value as? String
        XCTAssertEqual(currentValue, "focused", "region \(index) takes Rust focus")
    }

    private func waitForExactlyOneFocusedRegion(_ app: XCUIApplication, count: Int) {
        let regions = (0..<count).map { element(app, "seyal-pane-region-\($0)") }
        let deadline = Date().addingTimeInterval(8)
        var focusedCount = 0
        while Date() < deadline {
            focusedCount = regions.filter { $0.value as? String == "focused" }.count
            if focusedCount == 1 { return }
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        }
        focusedCount = regions.filter { $0.value as? String == "focused" }.count
        XCTAssertEqual(focusedCount, 1, "exactly one region takes Rust focus")
    }

    private func waitForSelectedTerminalTab(_ strip: XCUIElement, identifier: String) {
        let button = strip.buttons.matching(identifier: identifier).firstMatch
        let deadline = Date().addingTimeInterval(8)
        var value: String?
        while Date() < deadline {
            value = button.value as? String
            if value == "selected" { return }
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        }
        value = button.value as? String
        XCTAssertEqual(value, "selected", "terminal tab \(identifier) must become selected")
    }

    private func waitForSelectedTerminalTab(_ app: XCUIApplication, identifier: String) {
        let button = app.buttons.matching(identifier: identifier).firstMatch
        let deadline = Date().addingTimeInterval(8)
        var value: String?
        while Date() < deadline {
            value = button.value as? String
            if value == "selected" { return }
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        }
        value = button.value as? String
        XCTAssertEqual(value, "selected", "terminal tab \(identifier) must become selected")
    }

    private func waitForPaneExecution(
        _ app: XCUIApplication,
        presentationID: String,
        execution: String,
        timeout: TimeInterval = 12
    ) {
        let presentation = element(app, presentationID)
        let deadline = Date().addingTimeInterval(timeout)
        var actual: String?
        while Date() < deadline {
            actual = executionIdentity(presentation.value as? String)
            if actual == execution { return }
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        }
        XCTFail("\(presentationID) should bind execution \(execution), actual=\(actual ?? "nil")")
    }

    private func waitForAnyPaneBlock(_ command: String, in app: XCUIApplication, expected: Bool) {
        let deadline = Date().addingTimeInterval(8)
        var exists = anyPaneBlock(in: app, containing: command).exists
        while Date() < deadline, exists != expected {
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
            exists = anyPaneBlock(in: app, containing: command).exists
        }
        XCTAssertEqual(
            exists,
            expected,
            "Block command \(command) should \(expected ? "be visible" : "stay out of the selected terminal tab")"
        )
    }

    private func waitForUsablePty(in app: XCUIApplication, timeout: TimeInterval = 20) {
        // Multipane Flow hides liveContent/`terminal-input`; readiness is the
        // focused presentation host + composer (#1424). Raw resting is cold
        // default (#1445), so latch Flow before requiring composer.
        app.ensureRunningForegroundForAttach()
        let presentation = element(app, "seyal-pane-presentation-host")
        if presentation.waitForExistence(timeout: 2) {
            app.ensureFlowBlocksPresentation(timeout: timeout)
            XCTAssertTrue(
                element(app, "seyal-composer").waitForExistence(timeout: timeout),
                "focused Flow presentation host must expose a composer"
            )
            // Composer AX alone can win before the focused Controller is bound.
            // Wait for a real execution identity so Execute reaches a live PTY
            // (#1474 pane-local tabs / focus-churn marker routing). All-zero
            // placeholders from unbound hosts must not count as ready.
            let unbound = "execution=" + String(repeating: "0", count: 32)
            let bound = expectation(
                for: NSPredicate(
                    format: "value CONTAINS %@ AND NOT value CONTAINS %@",
                    "execution=",
                    unbound
                ),
                evaluatedWith: presentation,
                handler: nil
            )
            XCTAssertEqual(
                XCTWaiter.wait(for: [bound], timeout: timeout),
                .completed,
                "focused presentation host must bind an execution before submit; AX=\(presentation.value ?? "nil")"
            )
            app.ensureRunningForegroundForAttach(
                message: "Seyal.app left foreground while waiting for multipane Flow"
            )
            return
        }
        let terminal = app.descendants(matching: .any)["terminal-input"]
        XCTAssertTrue(terminal.waitForExistence(timeout: 10), "terminal surface missing")
        let usable = NSPredicate(format: "value CONTAINS 'connection=usable'")
        let arrived = expectation(for: usable, evaluatedWith: terminal, handler: nil)
        let result = XCTWaiter.wait(for: [arrived], timeout: timeout)
        app.ensureRunningForegroundForAttach()
        XCTAssertEqual(result, .completed, "PTY/runtime never became usable; terminal AX=\(terminal.value ?? "nil")")
        app.ensureFlowBlocksPresentation(timeout: timeout)
    }

    private func executionIdentity(_ accessibilityValue: String?) -> String? {
        guard let accessibilityValue,
            let range = accessibilityValue.range(of: "execution="),
            let end = accessibilityValue[range.upperBound...].firstIndex(of: " ")
        else { return nil }
        return String(accessibilityValue[range.upperBound..<end])
    }

    private func submitComposerCommand(
        _ command: String,
        in app: XCUIApplication,
        expectBlock: Bool = true,
        blockMarker: String? = nil
    ) {
        // Every pane's editor keeps the stable child ID for host reuse. Scope
        // lookup through the focused composer so a duplicate inactive editor
        // cannot steal synthetic input after a Pane focus change.
        let focusedComposer = element(app, "seyal-composer")
        XCTAssertTrue(focusedComposer.waitForExistence(timeout: 8), "focused composer missing")
        let editor = focusedComposer.descendants(matching: .any)
            .matching(identifier: "seyal-composer-editor")
            .firstMatch
        XCTAssertTrue(editor.waitForExistence(timeout: 8), "composer editor missing")
        editor.click()
        // Prefer paste: XCUI typeText mangles quotes/`$()`/paths and only the
        // first character (`/`) may land in the draft under multipane Flow.
        pasteIntoComposerEditor(editor, text: command)
        // Paste can clear keyboard focus; re-click so Return reaches the editor.
        editor.click()
        let draftMatched = expectation(
            for: NSPredicate(format: "value CONTAINS %@", String(command.prefix(12))),
            evaluatedWith: editor,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [draftMatched], timeout: 5),
            .completed,
            "composer draft missing after paste; value=\(editor.value ?? "nil")"
        )
        let submit = focusedComposer.descendants(matching: .any)
            .matching(identifier: "seyal-composer-execute")
            .firstMatch
        let enabled = expectation(for: NSPredicate(format: "isEnabled == true"), evaluatedWith: submit, handler: nil)
        if XCTWaiter.wait(for: [enabled], timeout: 4) == .completed {
            submit.click()
        } else {
            // submit() pushDraft()s first; Return works even when AX still shows
            // execute disabled after a synthetic paste.
            editor.typeKey("\r", modifierFlags: [])
        }
        guard expectBlock else { return }
        let marker = blockMarker ?? blockMarkerToken(in: command) ?? command
        let block = anyPaneBlock(in: app, containing: marker)
        if block.waitForExistence(timeout: 10) { return }
        let composerAX = (focusedComposer.value as? String) ?? "nil"
        let hostAX = (element(app, "seyal-pane-presentation-host").value as? String) ?? "nil"
        let editorAX = (editor.value as? String) ?? "nil"
        let blockLabels = app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier MATCHES %@", "^(seyal-block|seyal-secondary-block)-[0-9]+$")
        ).allElementsBoundByIndex.prefix(8).map {
            "\($0.identifier):\(($0.label as String?) ?? "")"
        }.joined(separator: " | ")
        XCTFail(
            "the selected execution records its own submitted command; marker=\(marker) "
                + "composer=\(composerAX) host=\(hostAX) editor=\(editorAX) blocks=[\(blockLabels)]"
        )
    }

    /// Paste avoids XCUI key synthesis gaps for shell metacharacters.
    private func pasteIntoComposerEditor(_ editor: XCUIElement, text: String) {
        let previous = NSPasteboard.general.string(forType: .string)
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
        // Ensure first-responder before synthetic Cmd-A/V; paste can clear focus.
        editor.click()
        editor.typeKey("a", modifierFlags: [.command])
        editor.typeKey("v", modifierFlags: [.command])
        editor.click()
        let needle = String(text.prefix(12))
        let pasted = NSPredicate(format: "value CONTAINS %@", needle).evaluate(with: editor)
        if !pasted {
            // Retry once — XCUI paste can miss under multipane focus churn.
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(text, forType: .string)
            editor.click()
            editor.typeKey("a", modifierFlags: [.command])
            editor.typeKey("v", modifierFlags: [.command])
            editor.click()
        }
        if let previous {
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(previous, forType: .string)
        }
    }

    /// Prefer a stable marker embedded in the command over the full shell line.
    private func blockMarkerToken(in command: String) -> String? {
        let patterns = [
            #"seyal-1424-[A-Za-z0-9_-]+"#,
            #"SEYAL_[A-Z0-9_]+"#,
            #"stream-marker-[A-Za-z0-9_-]+"#,
            #"after-vim-marker-[A-Za-z0-9_-]+"#,
        ]
        for pattern in patterns {
            if let match = command.range(of: pattern, options: .regularExpression) {
                return String(command[match])
            }
        }
        return nil
    }

    private func blockCommand(in app: XCUIApplication, containing command: String) -> XCUIElement {
        app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier BEGINSWITH %@ AND label CONTAINS %@", "seyal-block-", command)
        ).firstMatch
    }

    private func anyPaneBlock(in app: XCUIApplication, containing command: String) -> XCUIElement {
        app.descendants(matching: .any).matching(
            NSPredicate(
                format: "identifier MATCHES %@ AND label CONTAINS %@",
                "^(seyal-block|seyal-secondary-block)-[0-9]+$",
                command
            )
        ).firstMatch
    }

    private func blockBody(
        for block: XCUIElement,
        in app: XCUIApplication,
        expectedMarker: String? = nil,
        paneRegion: XCUIElement? = nil
    ) -> XCUIElement {
        let bodyId = "\(block.identifier)-body"
        let matchingBodies = app.descendants(matching: .any).matching(identifier: bodyId)
        let blockFrame = block.frame
        let paneFrame = paneRegion?.frame
        // Focused/secondary Block row identifiers are intentionally stable for
        // older UI assertions, so several panes can expose the same body id.
        // Bind a body to its owning card by geometry before using a global AX
        // firstMatch, which may otherwise read a sibling pane's same-index row.
        if let body = matchingBodies.allElementsBoundByIndex.first(where: { candidate in
            let frame = candidate.frame
            let belongsToPane = paneFrame.map {
                $0.contains(CGPoint(x: frame.midX, y: frame.midY))
            } ?? true
            let text = "\(candidate.label) \(candidate.value ?? "")"
            let containsMarker = expectedMarker.map { text.contains($0) } ?? true
            return belongsToPane && containsMarker && frame.intersects(blockFrame)
        }) {
            return body
        }
        // Fallback: any body descendant under the same pane Block stack.
        return block.descendants(matching: .any).matching(
            NSPredicate(format: "identifier ENDSWITH %@", "-body")
        ).firstMatch
    }

    private func waitForBodyMarker(
        _ body: XCUIElement,
        marker: String,
        timeout: TimeInterval = 8,
        message: String? = nil
    ) {
        // XCTest's predicate expectation can retain the previous accessibility
        // snapshot while a live Block is updating. Read the element directly
        // on each bounded poll so the wait observes the newest AX projection.
        let deadline = Date().addingTimeInterval(timeout)
        var actual = ""
        while Date() < deadline {
            actual = "\(body.label) \(body.value ?? "")"
            if actual.contains(marker) { return }
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        }
        actual = "\(body.label) \(body.value ?? "")"
        XCTAssertTrue(
            actual.contains(marker),
            "\(message ?? "Block body must visibly contain its output marker") \(marker); body=\(actual)"
        )
    }

    private func waitForPaintedHistoryRegion(_ paneHost: XCUIElement, timeout: TimeInterval) {
        let deadline = Date().addingTimeInterval(timeout)
        var diagnostics = ""
        while Date() < deadline {
            diagnostics = paneHost.value as? String ?? ""
            let countText = diagnostics.components(separatedBy: "history-regions-painted=").last?
                .prefix(while: \.isNumber) ?? ""
            let visibleCountText = diagnostics.components(separatedBy: "visible-history-clips=").last?
                .prefix(while: \.isNumber) ?? ""
            let pixelCountText = diagnostics.components(separatedBy: "history-visible-pixels=").last?
                .prefix(while: \.isNumber) ?? ""
            if let count = Int(countText), count > 0,
               let visibleCount = Int(visibleCountText), visibleCount > 0,
               let pixelCount = Int(pixelCountText), pixelCount > 0
            {
                return
            }
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        }
        diagnostics = paneHost.value as? String ?? diagnostics
        let countText = diagnostics.components(separatedBy: "history-regions-painted=").last?
            .prefix(while: \.isNumber) ?? ""
        let count = Int(countText) ?? 0
        XCTAssertGreaterThan(
            count,
            0,
            "the source Pane must paint its retained Block output; pane=\(diagnostics)"
        )
        let visibleCountText = diagnostics.components(separatedBy: "visible-history-clips=").last?
            .prefix(while: \.isNumber) ?? ""
        let visibleCount = Int(visibleCountText) ?? 0
        XCTAssertGreaterThan(
            visibleCount,
            0,
            "the retained Block clip must intersect its pane's Metal surface; pane=\(diagnostics)"
        )
        let pixelCountText = diagnostics.components(separatedBy: "history-visible-pixels=").last?
            .prefix(while: \.isNumber) ?? ""
        let pixelCount = Int(pixelCountText) ?? 0
        XCTAssertGreaterThan(
            pixelCount,
            0,
            "the retained Block must produce opaque pixels inside its Metal clip; pane=\(diagnostics)"
        )
    }

    private func recognizedText(in screenshot: XCUIScreenshot) throws -> String {
        let source = try XCTUnwrap(
            CGImageSourceCreateWithData(screenshot.pngRepresentation as CFData, nil)
        )
        let image = try XCTUnwrap(CGImageSourceCreateImageAtIndex(source, 0, nil))
        let request = VNRecognizeTextRequest()
        request.recognitionLevel = .accurate
        request.usesLanguageCorrection = false
        try VNImageRequestHandler(cgImage: image).perform([request])
        return (request.results ?? []).compactMap { $0.topCandidates(1).first?.string }.joined(separator: "\n")
    }

    private func waitForRenderedText(
        _ expected: String,
        in element: XCUIElement,
        timeout: TimeInterval = 5
    ) throws -> String {
        let deadline = Date().addingTimeInterval(timeout)
        var renderedText = ""
        while Date() < deadline {
            renderedText = try recognizedText(in: element.screenshot())
            if renderedText.contains(expected) { return renderedText }
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        }
        return renderedText
    }

    private func waitForAbsence(_ element: XCUIElement, _ message: String) {
        let gone = expectation(for: NSPredicate(format: "exists == false"), evaluatedWith: element, handler: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [gone], timeout: 8), .completed, message)
    }

    private func assertFrame(_ actual: CGRect, matches expected: CGRect, _ message: String) {
        XCTAssertEqual(actual.minX, expected.minX, accuracy: 1, message)
        XCTAssertEqual(actual.minY, expected.minY, accuracy: 1, message)
        XCTAssertEqual(actual.width, expected.width, accuracy: 1, message)
        XCTAssertEqual(actual.height, expected.height, accuracy: 1, message)
    }
}
