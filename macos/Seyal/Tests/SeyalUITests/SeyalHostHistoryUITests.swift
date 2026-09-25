import XCTest

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
        let app = hostedApp()
        waitForUsablePty(in: app)

        // ~10s paced seq 1 1000 keeps PRIMARY_CLIP active for mid-run capture
        // (Issue acceptance workload class). Assert running body exists and is
        // taller than a single line while the producer is still live.
        submitComposerCommand(
            app,
            "for i in $(seq 1 1000); do printf '%s\\n' \"$i\"; sleep 0.01; done"
        )
        waitBriefly(1.0)
        XCTAssertEqual(app.state, .runningForeground, "Seyal.app crashed while seq live-tail ran")
        assertFlowBlocksOrFail(in: app)
        let runningBody = app.descendants(matching: .any)["seyal-block-0-body"]
        XCTAssertTrue(
            runningBody.waitForExistence(timeout: 4),
            "running Block body missing during live-tail"
        )
        let runningFrame = runningBody.frame
        XCTAssertGreaterThan(
            runningFrame.height,
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
        XCTAssertTrue(app.wait(for: .runningForeground, timeout: 10))
        let terminal = app.descendants(matching: .any)["terminal-input"]
        XCTAssertTrue(terminal.waitForExistence(timeout: 10), "terminal surface missing")
        let usable = NSPredicate(format: "value CONTAINS 'connection=usable'")
        let arrived = expectation(for: usable, evaluatedWith: terminal, handler: nil)
        let result = XCTWaiter.wait(for: [arrived], timeout: timeout)
        XCTAssertEqual(app.state, .runningForeground, "Seyal.app crashed while waiting for PTY attach")
        XCTAssertEqual(
            result,
            .completed,
            "PTY/runtime never became usable; terminal AX=\(terminal.value ?? "nil")"
        )
        assertFlowBlocksOrFail(in: app)
    }

    private func assertFlowBlocksOrFail(in app: XCUIApplication, timeout: TimeInterval = 8) {
        let composer = app.descendants(matching: .any)["seyal-composer"]
        let blocks = app.descendants(matching: .any)["seyal-blocks"]
        let transcript = app.descendants(matching: .any)["seyal-blocks-scroll"]
        XCTAssertTrue(composer.waitForExistence(timeout: timeout), "history XCUI must stay on Flow/Blocks")
        XCTAssertTrue(composer.firstMatch.isHittable, "history XCUI must keep composer hittable")
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
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    private func waitBriefly(_ seconds: TimeInterval) {
        RunLoop.current.run(until: Date().addingTimeInterval(seconds))
    }
}
