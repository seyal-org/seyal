import XCTest

/// Headed #824 workload evidence on the ADR-015 thin host.
/// Metal does not expose PTY bytes as AX text. These cases prove Flow/Blocks
/// stays the headed oracle through high-volume output, alternate-screen return,
/// Unicode composer submit, and GUI relaunch reconnect. They are not a
/// raw-terminal Vim/htop/tmux oracle.
@MainActor
final class SeyalHostWorkloadUITests: XCTestCase {
    override func setUp() {
        continueAfterFailure = false
    }

    private func hostedApp() -> XCUIApplication {
        let app = XCUIApplication()
        app.launchIsolatedHost()
        return app
    }

    func testHighVolumeComposerOutputStaysOnFlowBlocks() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        submitComposerCommand(app, "printf '%s\\n' $(seq 1 60) 'seyal-824-high-volume'")
        XCTAssertEqual(app.state, .runningForeground, "Seyal.app crashed on high-volume output")
        assertFlowBlocksOrFail(in: app)
        attachScreenshot(app, name: "824-high-volume")
    }

    func testUnicodeComposerSubmitAndResizeStayOnFlowBlocks() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        submitComposerCommand(app, "printf 'seyal-824-unicode 日本語 👩‍💻\\n'")
        XCTAssertEqual(app.state, .runningForeground, "Seyal.app crashed on Unicode composer submit")
        assertFlowBlocksOrFail(in: app)
        resizeFrontWindow(app, dx: -180, dy: -80)
        waitBriefly(0.5)
        XCTAssertEqual(app.state, .runningForeground, "Seyal.app crashed on Unicode resize")
        assertFlowBlocksOrFail(in: app)
        attachScreenshot(app, name: "824-unicode-resize")
    }

    func testAlternateScreenReturnRestoresFlowNotRawTerminal() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        submitComposerCommand(app, "printf 'seyal-824-primary\\n'")
        assertFlowBlocksOrFail(in: app)
        try exerciseBoundedAlternateScreen(in: app) { command in
            submitComposerCommand(app, command)
        }
        assertFlowBlocksOrFail(in: app)
        attachScreenshot(app, name: "824-altscreen-return")
    }

    func testGuiRelaunchReconnectsWithoutKillingExecution() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        let terminal = app.descendants(matching: .any)["terminal-input"]
        XCTAssertTrue(terminal.waitForExistence(timeout: 5))
        let before = terminal.firstMatch.value as? String ?? ""
        XCTAssertTrue(before.contains("connection=usable"))
        XCTAssertFalse(before.contains("execution=none"), "usable attach must name an execution")
        let executionBefore = identityToken(before, key: "execution=")

        app.terminate()
        XCTAssertTrue(app.wait(for: .notRunning, timeout: 8))

        app.launch()
        waitForUsablePty(in: app)
        let afterValue = terminal.firstMatch.value as? String ?? ""
        XCTAssertTrue(afterValue.contains("connection=usable"), "GUI relaunch must reconnect")
        XCTAssertFalse(afterValue.contains("execution=none"))
        let executionAfter = identityToken(afterValue, key: "execution=")
        XCTAssertEqual(
            executionAfter,
            executionBefore,
            "M001 detach/reconnect must keep the same Runtime execution after GUI relaunch"
        )
        assertFlowBlocksOrFail(in: app)
        attachScreenshot(app, name: "824-relaunch-reconnect")
    }

    private func identityToken(_ value: String, key: String) -> String {
        guard let range = value.range(of: key) else { return "" }
        let rest = value[range.upperBound...]
        let token = rest.split(whereSeparator: { $0.isWhitespace }).first.map(String.init) ?? ""
        return token
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
            "PTY/runtime never became usable; terminal AX=\(terminal.value ?? "nil"). If another Runtime holds control.sock this run is INCONCLUSIVE."
        )
        assertFlowBlocksOrFail(in: app)
    }

    private func assertFlowBlocksOrFail(in app: XCUIApplication, timeout: TimeInterval = 8) {
        let composer = app.descendants(matching: .any)["seyal-composer"]
        let blocks = app.descendants(matching: .any)["seyal-blocks"]
        let transcript = app.descendants(matching: .any)["seyal-blocks-scroll"]
        XCTAssertTrue(
            composer.waitForExistence(timeout: timeout),
            "XCUI must stay on Flow/Blocks; composer missing — UI looks like a normal terminal"
        )
        XCTAssertTrue(composer.firstMatch.isHittable, "XCUI must keep composer hittable")
        XCTAssertTrue(blocks.waitForExistence(timeout: 5), "XCUI must keep seyal-blocks")
        XCTAssertTrue(transcript.waitForExistence(timeout: 5), "XCUI must keep the transcript")
        XCTAssertGreaterThan(transcript.firstMatch.frame.height, 120, "transcript must remain a full Flow surface")
        XCTAssertTrue(
            app.descendants(matching: .any)["seyal-left-workspaces"].firstMatch.isHittable,
            "Core Terminal left panel stays visible alongside Flow composer/Blocks (#922)"
        )
    }

    private func resizeFrontWindow(_ app: XCUIApplication, dx: CGFloat, dy: CGFloat) {
        let window = app.windows.firstMatch
        XCTAssertTrue(window.waitForExistence(timeout: 5), "no headed window to resize")
        let frame = window.frame
        let origin = window.coordinate(withNormalizedOffset: CGVector(dx: 1, dy: 1))
        let dest = origin.withOffset(CGVector(dx: dx, dy: dy))
        origin.click()
        origin.press(forDuration: 0.05, thenDragTo: dest)
        _ = frame
    }

    private func waitBriefly(_ seconds: TimeInterval) {
        RunLoop.current.run(until: Date().addingTimeInterval(seconds))
    }

    private func attachScreenshot(_ app: XCUIApplication, name: String) {
        let shot = XCTAttachment(screenshot: app.screenshot())
        shot.name = name
        shot.lifetime = .keepAlways
        add(shot)
    }
}
