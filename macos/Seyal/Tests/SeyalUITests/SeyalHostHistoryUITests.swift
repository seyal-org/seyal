import ImageIO
import XCTest
import Vision

/// Headed Raw terminal history, reflow, scrollback and alternate-screen evidence.
/// Metal does not expose PTY bytes as AX text, so cases inspect real screenshots
/// from the packaged host and do not close the physical M002/M003 matrix.
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

    func testRawLongOutputAndResizeStayOnTerminal() throws {
        let app = hostedApp()
        let terminal = waitForUsableRawTerminal(in: app)
        submitRawCommand(
            app,
            terminal: terminal,
            "printf '%s\\n' $(seq 1 40) 'seyal-842-reflow 日本語 👩‍💻 wrapping-line-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'"
        )
        let output = try screenshotContaining("seyal-842-reflow", in: app, timeout: 20)
        attachScreenshot(output, name: "842-raw-history-before-resize")

        resizeFrontWindow(app, dx: -220, dy: -140)
        assertRawTerminal(in: app)
        let narrow = app.windows.firstMatch.screenshot()
        attachScreenshot(narrow, name: "842-raw-history-narrow")
        XCTAssertEqual(app.state, .runningForeground, "Seyal.app crashed on narrow resize")

        resizeFrontWindow(app, dx: 220, dy: 140)
        assertRawTerminal(in: app)
        let wide = app.windows.firstMatch.screenshot()
        attachScreenshot(wide, name: "842-raw-history-wide")
        XCTAssertEqual(app.state, .runningForeground, "Seyal.app crashed on wide resize")
    }

    /// A paced producer proves output reaches the live Raw surface during a
    /// command and continues to update until the shell returns to its prompt.
    func testRawStreamingOutputKeepsLiveTail() throws {
        let app = hostedApp()
        let terminal = waitForUsableRawTerminal(in: app)
        submitRawCommand(
            app,
            terminal: terminal,
            "for i in $(seq 1 30); do printf 'M003-STREAM-%02d\\n' \"$i\"; sleep 0.03; done"
        )
        let first = try screenshotContaining("M003-STREAM-01", in: app, timeout: 10)
        attachScreenshot(first, name: "865-raw-stream-running")
        let tail = try screenshotContaining("M003-STREAM-30", in: app, timeout: 20)
        attachScreenshot(tail, name: "865-raw-stream-live-tail")
        XCTAssertEqual(app.state, .runningForeground, "Seyal.app crashed during streamed output")
        assertRawTerminal(in: app)
    }

    /// Local Raw scrollback must not resize the PTY or move its live cursor.
    func testRawScrollDoesNotMutatePtyGeometryOrCursor() throws {
        let app = hostedApp()
        let terminal = waitForUsableRawTerminal(in: app)
        submitRawCommand(
            app,
            terminal: terminal,
            "printf 'M003-SCROLL-OLD\\n'; seq 1 120; printf 'M003-SCROLL-TAIL\\n'"
        )
        _ = try screenshotContaining("M003-SCROLL-TAIL", in: app, timeout: 20)
        let beforeValue = terminal.value as? String ?? ""
        let before = try XCTUnwrap(parseTerminalGeometryProbe(beforeValue))

        terminal.scroll(byDeltaX: 0, deltaY: 1_000_000)
        let history = try screenshotContaining("M003-SCROLL-OLD", in: app, timeout: 20)
        attachScreenshot(history, name: "865-raw-scroll-history")
        let afterScrollValue = terminal.value as? String ?? ""
        let afterScroll = try XCTUnwrap(parseTerminalGeometryProbe(afterScrollValue))
        XCTAssertEqual(afterScroll, before, "Raw history scroll must preserve PTY geometry and cursor")

        terminal.scroll(byDeltaX: 0, deltaY: -1_000_000)
        let live = try screenshotContaining("M003-SCROLL-TAIL", in: app, timeout: 20)
        attachScreenshot(live, name: "865-raw-scroll-live-end")
        let afterReturn = try XCTUnwrap(parseTerminalGeometryProbe(terminal.value as? String ?? ""))
        XCTAssertEqual(afterReturn, before, "returning to the live end must not mutate PTY state")
        XCTAssertEqual(app.state, .runningForeground)
    }

    /// M003 daily-driver scrollback acceptance on the conventional Raw surface.
    /// A real local shell streams 100,000 lines; history must remain scrollable
    /// and return to the same live tail without changing PTY geometry.
    func testRawHundredThousandOutputLinesRemainScrollableAndStable() throws {
        let app = hostedApp()
        let terminal = waitForUsableRawTerminal(in: app)
        let initialValue = terminal.value as? String ?? ""
        let initialGeometry = try XCTUnwrap(
            parseTerminalGeometryProbe(initialValue),
            "Raw terminal must publish grid geometry: \(initialValue)"
        )
        XCTAssertGreaterThan(initialGeometry.rows, 0)
        XCTAssertGreaterThan(initialGeometry.columns, 0)

        submitRawCommand(
            app,
            terminal: terminal,
            "printf 'M003-OLD-ROW\\n'; seq 1 100000; printf 'M003-TAIL-ROW\\n'"
        )
        let liveEndScreenshot = try screenshotContaining("M003-TAIL-ROW", in: app, timeout: 90)
        attachScreenshot(liveEndScreenshot, name: "m003-raw-100k-live-end")
        let liveEndText = try recognizedText(in: liveEndScreenshot)
        XCTAssertTrue(
            liveEndText.contains("100000"),
            "100k tail must be visible; OCR=\(liveEndText)"
        )
        XCTAssertEqual(app.state, .runningForeground, "Seyal.app must stay up after 100,000 lines")

        terminal.scroll(byDeltaX: 0, deltaY: 1_000_000)
        let historyScreenshot = try screenshotContaining("M003-OLD-ROW", in: app, timeout: 20)
        let historyText = try recognizedText(in: historyScreenshot)
        attachScreenshot(historyScreenshot, name: "m003-raw-100k-scrolled-history")
        XCTAssertTrue(
            historyText.contains("M003-OLD-ROW"),
            "scrolling to retained history must visibly paint the first output row; OCR=\(historyText)"
        )

        terminal.scroll(byDeltaX: 0, deltaY: -1_000_000)
        let restoredScreenshot = try screenshotContaining("M003-TAIL-ROW", in: app, timeout: 20)
        attachScreenshot(restoredScreenshot, name: "m003-raw-100k-restored-live-end")
        let restoredText = try recognizedText(in: restoredScreenshot)
        XCTAssertTrue(
            restoredText.contains("100000"),
            "restored live tail must remain visible; OCR=\(restoredText)"
        )
        let finalValue = terminal.value as? String ?? ""
        let finalGeometry = try XCTUnwrap(parseTerminalGeometryProbe(finalValue))
        XCTAssertEqual(finalGeometry.rows, initialGeometry.rows, "scrollback must not change PTY rows")
        XCTAssertEqual(finalGeometry.columns, initialGeometry.columns, "scrollback must not change PTY columns")
        XCTAssertEqual(app.state, .runningForeground)
    }

    /// #1492 Raw-first acceptance: a large primary-screen stream must remain
    /// stable when later commands update the same live terminal surface.
    func testRawTenThousandLinesRemainStableAcrossFollowingCommands() throws {
        let app = hostedApp()
        let terminal = waitForUsableRawTerminal(in: app)
        let initialGeometry = try XCTUnwrap(
            parseTerminalGeometryProbe(terminal.value as? String ?? ""),
            "Raw terminal must publish grid geometry"
        )
        XCTAssertGreaterThan(initialGeometry.rows, 0, "Raw terminal rows must be nonzero")
        XCTAssertGreaterThan(initialGeometry.columns, 0, "Raw terminal columns must be nonzero")

        submitRawCommand(app, terminal: terminal, "seq 1 10000")
        let streamScreenshot = try screenshotContaining("10000", in: app, timeout: 20)
        attachScreenshot(streamScreenshot, name: "1492-raw-10k-live-end")
        let streamText = try recognizedText(in: streamScreenshot)
        XCTAssertTrue(streamText.contains("10000"), "10k stream tail must be visible; OCR=\(streamText)")

        submitRawCommand(app, terminal: terminal, "pwd")
        submitRawCommand(app, terminal: terminal, "ls -d .")
        submitRawCommand(app, terminal: terminal, "printf 'SEYAL-RAW-10K-DONE'")
        let followupScreenshot = try screenshotContaining("SEYAL-RAW-10K-DONE", in: app, timeout: 20)
        attachScreenshot(followupScreenshot, name: "1492-raw-10k-after-pwd-ls")
        let followupText = try recognizedText(in: followupScreenshot)
        XCTAssertTrue(
            followupText.contains("SEYAL-RAW-10K-DONE"),
            "follow-up commands must complete on the same Raw surface; OCR=\(followupText)"
        )
        XCTAssertTrue(
            followupText.contains("10000"),
            "the prior stream tail must remain visible after pwd/ls; OCR=\(followupText)"
        )
        XCTAssertEqual(app.state, .runningForeground, "Seyal.app must stay up after Raw 10k output")
    }

    func testRawSequentialCommandsPreserveEarlierOutput() throws {
        let app = hostedApp()
        let terminal = waitForUsableRawTerminal(in: app)
        submitRawCommand(app, terminal: terminal, "printf 'M003-FIRST-COMMAND\\n'")
        _ = try screenshotContaining("M003-FIRST-COMMAND", in: app, timeout: 20)
        submitRawCommand(app, terminal: terminal, "printf 'M003-SECOND-COMMAND\\n'")
        let result = try screenshotContaining("M003-SECOND-COMMAND", in: app, timeout: 20)
        let text = try recognizedText(in: result)
        XCTAssertTrue(text.contains("M003-FIRST-COMMAND"), "prior output must remain visible; OCR=\(text)")
        XCTAssertTrue(text.contains("M003-SECOND-COMMAND"), "new output must be visible; OCR=\(text)")
        attachScreenshot(result, name: "865-raw-sequential-commands")
    }

    func testRawAlternateScreenExitRestoresHistorySurface() throws {
        let app = hostedApp()
        let terminal = waitForUsableRawTerminal(in: app)
        submitRawCommand(app, terminal: terminal, "printf 'M003-ALTSCR-PRIMARY\\n'")
        _ = try screenshotContaining("M003-ALTSCR-PRIMARY", in: app, timeout: 20)
        try exerciseBoundedAlternateScreen(in: app) { command in
            submitRawCommand(app, terminal: terminal, command)
        }
        let restored = try screenshotContaining("M003-ALTSCR-PRIMARY", in: app, timeout: 20)
        attachScreenshot(restored, name: "842-raw-history-after-altscreen")
        assertRawTerminal(in: app)
    }

    /// Raw is the only user-selectable resting presentation while Blocks are suspended.
    func testViewMenuDoesNotOfferPresentationSwitches() throws {
        let app = hostedApp()
        _ = waitForUsableRawTerminal(in: app)
        let viewMenu = app.menuBars.menuBarItems["View"]
        XCTAssertTrue(viewMenu.waitForExistence(timeout: 5), "View menu must be present")
        viewMenu.click()
        XCTAssertFalse(app.menuItems["Flow"].exists)
        XCTAssertFalse(app.menuItems["Set Flow"].exists)
        XCTAssertFalse(app.menuItems["Toggle Raw"].exists)
        XCTAssertTrue(app.menuItems["Toggle TUI"].exists)
    }

    private func waitForUsableRawTerminal(
        in app: XCUIApplication, timeout: TimeInterval = 20
    ) -> XCUIElement {
        XCTAssertTrue(app.wait(for: .runningForeground, timeout: 10))
        let terminal = app.descendants(matching: .any)["terminal-input"].firstMatch
        XCTAssertTrue(terminal.waitForExistence(timeout: 10), "Raw terminal surface missing")
        let usable = expectation(
            for: NSPredicate(format: "value CONTAINS 'connection=usable'"),
            evaluatedWith: terminal,
            handler: nil
        )
        XCTAssertEqual(XCTWaiter.wait(for: [usable], timeout: timeout), .completed)
        assertRawTerminal(in: app, terminal: terminal)
        return terminal
    }

    private func assertRawTerminal(in app: XCUIApplication, terminal: XCUIElement? = nil) {
        let surface = terminal ?? app.descendants(matching: .any)["terminal-input"].firstMatch
        XCTAssertTrue(surface.exists, "conventional Raw terminal surface must remain mounted")
        XCTAssertFalse(
            app.descendants(matching: .any)["seyal-composer"].exists,
            "Blocks suspension must leave Raw as the only resting presentation"
        )
        XCTAssertEqual(app.state, .runningForeground)
    }

    private func submitRawCommand(
        _ app: XCUIApplication, terminal: XCUIElement, _ command: String
    ) {
        terminal.click()
        app.typeText(command)
        app.typeKey("\r", modifierFlags: [])
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

    private func screenshotContaining(
        _ text: String, in app: XCUIApplication, timeout: TimeInterval
    ) throws -> XCUIScreenshot {
        let deadline = Date().addingTimeInterval(timeout)
        var screenshot = app.windows.firstMatch.screenshot()
        while Date() < deadline {
            if try recognizedText(in: screenshot).contains(text) { return screenshot }
            waitBriefly(0.25)
            screenshot = app.windows.firstMatch.screenshot()
        }
        XCTFail("Timed out waiting for visible text '\(text)'")
        return screenshot
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
