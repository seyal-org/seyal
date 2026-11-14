import Darwin
import ImageIO
import XCTest
import Vision

/// Headed HistoryStore / reflow evidence for #842. Metal does not expose PTY
/// bytes as AX text, so these cases prove the user-visible Flow/Blocks host
/// stays attached, resizes, and returns from alternate-screen without leaving
/// a raw-terminal chrome. They do not close the physical #673 matrix.
@MainActor
final class SeyalHostHistoryUITests: XCTestCase {
    override func setUp() {
        continueAfterFailure = false
    }

    private func hostedApp() -> XCUIApplication {
        let app = XCUIApplication()
        app.launchIsolatedHost()
        return app
    }

    func testLongOutputAndResizeStayOnFlowBlocks() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)

        submitComposerCommand(
            app,
            "printf '%s\\n' $(seq 1 40) 'seyal-842-reflow 日本語 👩‍💻 wrapping-line-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'"
        )
        XCTAssertEqual(app.state, .runningForeground, "Seyal.app crashed on long history output")
        assertFlowBlocksOrFail(in: app)
        attachScreenshot(app, name: "842-history-before-resize")

        resizeFrontWindow(app, dx: -220, dy: -140)
        waitBriefly(0.6)
        XCTAssertEqual(app.state, .runningForeground, "Seyal.app crashed on narrow resize")
        assertFlowBlocksOrFail(in: app)
        attachScreenshot(app, name: "842-history-narrow")

        resizeFrontWindow(app, dx: 220, dy: 140)
        waitBriefly(0.6)
        XCTAssertEqual(app.state, .runningForeground, "Seyal.app crashed on wide resize")
        assertFlowBlocksOrFail(in: app)
        attachScreenshot(app, name: "842-history-wide")
    }

    /// #865: a long running normal-screen command must stay on one Flow Block
    /// surface under the Pane scroll owner (no Pane-wide live grid chrome).
    /// Uses a paced producer so the capture lands while the Block is still
    /// running (PRIMARY_CLIP), then waits for completion handoff.
    func testRunningSeqLiveTailStaysOnFlowBlocks() throws {
        // The headed host spawns the account pw_shell. Only trusted zsh
        // integration publishes a Block (OSC 133). Bash CI accounts never
        // get one; skipping here matches testComposerReadsBusyWhileCommandRunsAndAvailableAtNextPrompt.
        // Clip geometry on that path is SeyalHostComponentTests.
        guard loginShellIsZsh() else {
            throw XCTSkip(
                "Running Block live-tail requires a zsh pw_shell. Hosted runners use bash, so no OSC 133 Block is published."
            )
        }
        let app = hostedApp()
        waitForUsablePty(in: app)

        // Paced seq stays running long enough for the body-height check.
        submitComposerCommand(
            app,
            "for i in $(seq 1 1000); do printf '%s\\n' \"$i\"; sleep 0.01; done"
        )
        XCTAssertEqual(app.state, .runningForeground, "Seyal.app crashed while seq live-tail ran")
        assertFlowBlocksOrFail(in: app)
        let runningBody = app.descendants(matching: .any)["seyal-block-0-body"]
        XCTAssertTrue(
            runningBody.waitForExistence(timeout: 12),
            "zsh integration must publish a running Block body"
        )
        XCTAssertGreaterThan(
            runningBody.frame.height,
            8,
            "running live-tail body must be taller than a one-line stub while seq runs"
        )
        attachScreenshot(app, name: "865-live-tail-running-seq")

        // Completion handoff must remain on Flow Blocks, not a raw Metal viewport.
        // seq 1 1000 @ 10ms ≈ 10s; allow headroom under CI load.
        waitBriefly(14.0)
        XCTAssertEqual(app.state, .runningForeground)
        assertFlowBlocksOrFail(in: app)
        attachScreenshot(app, name: "865-live-tail-after-seq")
    }

    /// #865 / SPEC-008 §3.1: Flow transcript scroll must leave prepared-frame
    /// PTY size and cursor tokens unchanged on the terminal AX probe.
    /// Hosted CI without a zsh `pw_shell` is ENVIRONMENT_UNSUPPORTED (this
    /// case `XCTSkip`s); local zsh evidence must still prove clip motion.
    func testFlowTranscriptScrollDoesNotMutatePtySizeOrCursor() throws {
        guard loginShellIsZsh() else {
            throw XCTSkip(
                "Flow scroll→PTY/cursor invariant requires a zsh pw_shell (OSC 133 Blocks). ENVIRONMENT_UNSUPPORTED on hosted CI without zsh."
            )
        }
        let app = hostedApp()
        waitForUsablePty(in: app)

        submitComposerCommand(app, "printf '%s\\n' $(seq 1 120)")
        waitBriefly(1.2)
        assertFlowBlocksOrFail(in: app)

        let terminal = app.descendants(matching: .any)["terminal-input"].firstMatch
        XCTAssertTrue(terminal.waitForExistence(timeout: 5))
        let beforeValue = terminal.value as? String ?? ""
        let beforeGeom = try XCTUnwrap(
            parseTerminalGeometryProbe(beforeValue),
            "terminal AX must expose rows/columns/cursor before scroll; value=\(beforeValue)"
        )
        XCTAssertGreaterThan(beforeGeom.rows, 0)
        XCTAssertGreaterThan(beforeGeom.columns, 0)

        let transcript = app.descendants(matching: .any)["seyal-blocks-scroll"].firstMatch
        XCTAssertTrue(transcript.waitForExistence(timeout: 5))
        let body = app.descendants(matching: .any)["seyal-block-0-body"].firstMatch
        XCTAssertTrue(body.waitForExistence(timeout: 5), "the overflowing Block body must be addressable")
        let yBefore = body.frame.minY
        // Positive XCTest delta moves toward older transcript content on this
        // flipped AppKit scroll view. A negative delta at live-tail was a
        // no-op and made the former assertion report a product defect.
        transcript.scroll(byDeltaX: 0, deltaY: 320)
        waitBriefly(0.6)
        let yAfter = body.frame.minY
        XCTAssertNotEqual(
            yBefore,
            yAfter,
            "the Block body must move inside its transcript viewport after scrolling toward history"
        )

        XCTAssertEqual(app.state, .runningForeground, "Seyal.app must stay up after Flow scroll")
        assertFlowBlocksOrFail(in: app)
        let afterValue = terminal.value as? String ?? ""
        let afterGeom = try XCTUnwrap(
            parseTerminalGeometryProbe(afterValue),
            "terminal AX must expose rows/columns/cursor after scroll; value=\(afterValue)"
        )
        XCTAssertEqual(afterGeom.rows, beforeGeom.rows, "Flow scroll must not change PTY rows")
        XCTAssertEqual(afterGeom.columns, beforeGeom.columns, "Flow scroll must not change PTY columns")
        XCTAssertEqual(afterGeom.cursorRow, beforeGeom.cursorRow, "Flow scroll must not change cursor row")
        XCTAssertEqual(
            afterGeom.cursorColumn,
            beforeGeom.cursorColumn,
            "Flow scroll must not change cursor column"
        )
        XCTAssertEqual(
            afterGeom.cursorVisible,
            beforeGeom.cursorVisible,
            "Flow scroll must not change cursor visibility"
        )
        attachScreenshot(app, name: "865-flow-scroll-pty-cursor-invariant")
    }

    /// M003 daily-driver scrollback acceptance: use a real local shell/PTy and
    /// 100,000 output lines, then prove the final output remains available and
    /// the transcript can move away from and back to its live end.
    func testHundredThousandOutputLinesRemainScrollableAndDocked() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        submitComposerCommand(
            app,
            "printf 'M003-OLD-ROW\\n'; seq 1 100000; printf 'M003-TAIL-ROW\\n'"
        )

        let composer = app.descendants(matching: .any)["seyal-composer"].firstMatch
        let available = expectation(
            for: NSPredicate(format: "value == 'available'"),
            evaluatedWith: composer,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [available], timeout: 60),
            .completed,
            "the 100,000-line command must finish and return the composer to service"
        )
        XCTAssertEqual(app.state, .runningForeground, "Seyal.app must stay up after 100,000 lines")
        assertFlowBlocksOrFail(in: app)

        let body = app.descendants(matching: .any)["seyal-block-0-body"].firstMatch
        XCTAssertTrue(body.waitForExistence(timeout: 12), "the large-output Block body must remain mounted")
        let tail = expectation(
            for: NSPredicate(format: "label CONTAINS 'M003-TAIL-ROW' OR value CONTAINS 'M003-TAIL-ROW'"),
            evaluatedWith: body,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [tail], timeout: 20),
            .completed,
            "the Block's retained transcript must expose the final output line"
        )
        let transcript = app.descendants(matching: .any)["seyal-blocks-scroll"].firstMatch
        XCTAssertTrue(transcript.exists)
        let composerTop = composer.frame.minY
        XCTAssertLessThanOrEqual(
            composerTop - transcript.frame.maxY,
            32,
            "the transcript viewport must stay docked immediately above the composer"
        )
        let liveEndY = body.frame.minY
        let liveEndScreenshot = app.windows.firstMatch.screenshot()
        attachScreenshot(liveEndScreenshot, name: "m003-100k-live-end")
        let liveEndText = try recognizedText(in: liveEndScreenshot)
        XCTAssertTrue(
            liveEndText.contains("M003-TAIL-ROW"),
            "the live Block tail must be visibly painted, not only present in AX; OCR=\(liveEndText)"
        )

        transcript.scroll(byDeltaX: 0, deltaY: 520)
        waitBriefly(0.6)
        let historyY = body.frame.minY
        XCTAssertGreaterThan(
            abs(historyY - liveEndY),
            80,
            "100,000 output lines must be scrollable within the Block transcript"
        )
        let historyScreenshot = app.windows.firstMatch.screenshot()
        attachScreenshot(historyScreenshot, name: "m003-100k-scrolled-history")
        var historyText = try recognizedText(in: historyScreenshot)
        if !historyText.contains("M003-OLD-ROW") {
            // Drive the Pane scroll to its oldest retained content; large
            // deltas are intentional here because the real document has
            // 100,000 rows and only a bounded viewport may be prepared.
            transcript.scroll(byDeltaX: 0, deltaY: 1_000_000)
            waitBriefly(0.6)
            historyText = try recognizedText(in: app.windows.firstMatch.screenshot())
        }
        XCTAssertTrue(
            historyText.contains("M003-OLD-ROW"),
            "scrolling to the oldest retained page must visibly paint the first output row; OCR=\(historyText)"
        )
        transcript.scroll(byDeltaX: 0, deltaY: -1_000_000)
        waitBriefly(0.6)
        XCTAssertEqual(
            body.frame.minY,
            liveEndY,
            accuracy: 48,
            "scrolling toward the live end must restore the original Block position"
        )
        let restoredText = try recognizedText(in: app.windows.firstMatch.screenshot())
        XCTAssertTrue(
            restoredText.contains("M003-TAIL-ROW"),
            "returning to the live end must repaint the tail page; OCR=\(restoredText)"
        )
        XCTAssertEqual(app.state, .runningForeground)
    }

    /// #865: a second running command must keep Flow Blocks (preceding output
    /// stays owned by earlier Block chrome, not a Pane-wide live grid).
    func testSequentialCommandsKeepFlowLiveTailOnBlocks() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)

        submitComposerCommand(app, "printf 'seyal-865-first\\n'")
        waitBriefly(0.8)
        assertFlowBlocksOrFail(in: app)

        submitComposerCommand(
            app,
            "for i in $(seq 1 80); do printf 'second-%s\\n' \"$i\"; sleep 0.02; done"
        )
        waitBriefly(0.5)
        XCTAssertEqual(app.state, .runningForeground)
        assertFlowBlocksOrFail(in: app)
        attachScreenshot(app, name: "865-live-tail-second-command")

        waitBriefly(3.0)
        assertFlowBlocksOrFail(in: app)
        attachScreenshot(app, name: "865-live-tail-after-second")
    }

    func testAlternateScreenExitRestoresFlowHistorySurface() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        submitComposerCommand(app, "printf 'seyal-842-primary\\n'")
        assertFlowBlocksOrFail(in: app)

        try exerciseBoundedAlternateScreen(in: app) { command in
            submitComposerCommand(app, command)
        }
        assertFlowBlocksOrFail(in: app)
        attachScreenshot(app, name: "842-history-after-altscreen")
    }

    /// #1442 historically used Cmd+Enter → Toggle Raw. Accepted keybinding
    /// policy now keeps `presentation.toggle_raw` out of builtins and the
    /// View menu (`supported_shell_menu_and_builtins_do_not_expose_raw_presentation`).
    /// Raw/Flow restore coverage lives in presentation unit tests; a headed
    /// Raw-admission E2E needs its own Issue against the current resting model.
    func testCmdEnterRawToggleRestoresFlowBlocks() throws {
        throw XCTSkip(
            "Toggle Raw has no builtin Cmd+Enter / View menu admission; "
                + "see keybinding projection policy. Out of scope for #1457 chrome."
        )
    }

    private func submitComposerCommand(_ app: XCUIApplication, _ command: String) {
        let composer = app.descendants(matching: .any)["seyal-composer"]
        XCTAssertTrue(composer.waitForExistence(timeout: 12))
        let composerReady = NSPredicate(format: "value == 'available'")
        let becameReady = expectation(for: composerReady, evaluatedWith: composer, handler: nil)
        XCTAssertEqual(
            XCTWaiter.wait(for: [becameReady], timeout: 12),
            .completed,
            "composer never became available; value=\(composer.value ?? "nil")"
        )
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
        let cleared = expectation(
            for: NSPredicate(format: "value == nil OR value == ''"),
            evaluatedWith: editor.firstMatch,
            handler: nil
        )
        _ = XCTWaiter.wait(for: [cleared], timeout: 8)
    }

    private func waitForUsablePty(in app: XCUIApplication, timeout: TimeInterval = 20) {
        app.ensureRunningForegroundForAttach()
        let terminal = app.descendants(matching: .any)["terminal-input"]
        XCTAssertTrue(terminal.waitForExistence(timeout: 10), "terminal surface missing")
        let usable = NSPredicate(format: "value CONTAINS 'connection=usable'")
        let arrived = expectation(for: usable, evaluatedWith: terminal, handler: nil)
        let result = XCTWaiter.wait(for: [arrived], timeout: timeout)
        app.ensureRunningForegroundForAttach()
        XCTAssertEqual(
            result,
            .completed,
            "PTY/runtime never became usable; terminal AX=\(terminal.value ?? "nil")"
        )
        assertFlowBlocksOrFail(in: app)
    }

    private func assertFlowBlocksOrFail(in app: XCUIApplication, timeout: TimeInterval = 8) {
        app.ensureFlowBlocksPresentation(timeout: timeout)
        let composer = app.descendants(matching: .any)["seyal-composer"]
        let blocks = app.descendants(matching: .any)["seyal-blocks"]
        let transcript = app.descendants(matching: .any)["seyal-blocks-scroll"]
        XCTAssertTrue(composer.waitForExistence(timeout: timeout), "history XCUI must stay on Flow/Blocks")
        let hittable = expectation(
            for: NSPredicate(format: "isHittable == true"),
            evaluatedWith: composer.firstMatch,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [hittable], timeout: timeout),
            .completed,
            "history XCUI must keep composer hittable; value=\(composer.value ?? "nil")"
        )
        XCTAssertTrue(blocks.waitForExistence(timeout: 5), "history XCUI must keep seyal-blocks")
        XCTAssertTrue(transcript.waitForExistence(timeout: 5), "history XCUI must keep the transcript")
        XCTAssertGreaterThan(transcript.firstMatch.frame.height, 120, "transcript must remain a full Flow surface")
    }

    private func resizeFrontWindow(_ app: XCUIApplication, dx: CGFloat, dy: CGFloat) {
        let window = app.windows.firstMatch
        XCTAssertTrue(window.waitForExistence(timeout: 5), "no headed window to resize")
        let start = window.coordinate(withNormalizedOffset: CGVector(dx: 0.98, dy: 0.98))
        let end = start.withOffset(CGVector(dx: dx, dy: dy))
        start.click(forDuration: 0.15, thenDragTo: end)
    }

    private func attachScreenshot(_ app: XCUIApplication, name: String) {
        attachScreenshot(app.screenshot(), name: name)
    }

    private func attachScreenshot(_ screenshot: XCUIScreenshot, name: String) {
        let attachment = XCTAttachment(screenshot: screenshot)
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
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
        return (request.results ?? []).compactMap { $0.topCandidates(1).first?.string }
            .joined(separator: "\n")
    }

    private func waitBriefly(_ seconds: TimeInterval) {
        RunLoop.current.run(until: Date().addingTimeInterval(seconds))
    }

    /// Same predicate as shell-integration support: account `pw_shell`, not `SHELL`.
    private func loginShellIsZsh() -> Bool {
        guard let account = getpwuid(geteuid()), let shell = account.pointee.pw_shell else {
            return false
        }
        return URL(fileURLWithPath: String(cString: shell)).lastPathComponent == "zsh"
    }

    private struct TerminalGeometryProbe: Equatable {
        var rows: Int
        var columns: Int
        var cursorRow: Int
        var cursorColumn: Int
        var cursorVisible: Bool
    }

    private func parseTerminalGeometryProbe(_ value: String) -> TerminalGeometryProbe? {
        func intToken(_ name: String) -> Int? {
            guard let range = value.range(of: "\(name)=") else { return nil }
            let rest = value[range.upperBound...]
            let digits = rest.prefix(while: { $0.isNumber || $0 == "-" })
            return Int(digits)
        }
        guard let rows = intToken("rows"),
            let columns = intToken("columns"),
            let cursorRange = value.range(of: "cursor=")
        else { return nil }
        let cursorRest = value[cursorRange.upperBound...]
        let parts = cursorRest.split(separator: " ", maxSplits: 1, omittingEmptySubsequences: true)
        guard let pair = parts.first?.split(separator: ","), pair.count == 2,
            let cursorRow = Int(pair[0]),
            let cursorColumn = Int(pair[1])
        else { return nil }
        let visible: Bool
        if let visRange = value.range(of: "cursor-visible=") {
            let token = value[visRange.upperBound...].prefix(while: { $0 != " " })
            visible = token == "true"
        } else {
            return nil
        }
        return TerminalGeometryProbe(
            rows: rows,
            columns: columns,
            cursorRow: cursorRow,
            cursorColumn: cursorColumn,
            cursorVisible: visible
        )
    }
}
