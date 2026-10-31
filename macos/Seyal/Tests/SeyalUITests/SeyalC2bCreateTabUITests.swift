import XCTest

/// Issue #1175 / M003 C2b headed evidence: live CreateTab, generation-fenced
/// detach-only Close Tab, and explicit P4 Terminate Execution. Does not enable
/// pane splitting.
@MainActor
final class SeyalC2bCreateTabUITests: XCTestCase {
    override func setUp() {
        continueAfterFailure = false
    }

    private func hostedApp() -> XCUIApplication {
        let app = XCUIApplication()
        app.launchIsolatedHost()
        return app
    }

    func testCreateTabProjectsSecondTabAfterLiveBind() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        clickNewTab(in: app)
        waitForRunLoop(5)
        XCTAssertEqual(app.state, .runningForeground, "CreateTab must not crash Seyal.app")
        showTabs(in: app)
        XCTAssertTrue(
            app.descendants(matching: .any)["seyal-tab-1"].firstMatch.waitForExistence(timeout: 12),
            "CreateTab must project a second tab after live bind"
        )
        XCTAssertTrue(app.descendants(matching: .any)["seyal-close-tab"].firstMatch.exists)
    }

    func testCommandTCreatesPaneTerminalAndCommandNCreatesWindow() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        XCTAssertEqual(app.windows.count, 1)

        XCTAssertEqual(
            terminalTabButtons(in: app).count,
            1,
            "Expected one visible Pane terminal tab before ⌘T:\n\(app.debugDescription)"
        )
        app.typeKey("t", modifierFlags: .command)
        waitForWindowCount(app, 1, timeout: 5)
        waitForTerminalTabCount(app, 2, timeout: 12)
        XCTAssertEqual(app.windows.count, 1, "⌘T creates a Pane-local terminal, not a Layout or Window")

        app.typeKey("n", modifierFlags: .command)
        waitForWindowCount(app, 2, timeout: 10)

        app.typeKey("n", modifierFlags: [.command, .shift])
        waitForWindowCount(app, 3, timeout: 10)
    }

    func testCloseTabRemovesChromeWithoutCrashingTheHost() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        clickNewTab(in: app)
        waitForRunLoop(5)
        showTabs(in: app)
        XCTAssertTrue(app.descendants(matching: .any)["seyal-tab-1"].firstMatch.waitForExistence(timeout: 12))
        let closeTab = app.descendants(matching: .any)["seyal-close-tab"].firstMatch
        XCTAssertTrue(closeTab.waitForExistence(timeout: 8))
        XCTAssertTrue(closeTab.isHittable, "Rust-enabled close control must be actionable")
        closeTab.click()
        XCTAssertEqual(app.state, .runningForeground)
        let removedTab = app.descendants(matching: .any)["seyal-tab-1"].firstMatch
        let removed = XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "exists == false"),
            object: removedTab
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [removed], timeout: 5),
            .completed,
            "a current containment fence closes the intended Tab"
        )
        waitForUsablePty(in: app, timeout: 12)
    }

    func testTerminateExecutionPaletteVerbKeepsHostAndTabChromeAlive() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        clickNewTab(in: app)
        waitForRunLoop(5)
        showTabs(in: app)
        XCTAssertTrue(
            app.descendants(matching: .any)["seyal-tab-1"].firstMatch.waitForExistence(timeout: 12),
            "second tab must be projected before the palette terminate verb"
        )
        waitForUsablePty(in: app, timeout: 12)
        runPaletteCommand(in: app, query: "Terminate Execution")
        waitForRunLoop(1)
        XCTAssertEqual(app.state, .runningForeground)
        showTabs(in: app)
        XCTAssertTrue(
            app.descendants(matching: .any)["seyal-tab-1"].firstMatch.waitForExistence(timeout: 8),
            "terminate does not close tab chrome; wire TerminationRequested is proven in the portable harness"
        )
    }

    private func clickNewTab(in app: XCUIApplication) {
        let newTab = app.descendants(matching: .any)["seyal-new-tab"].firstMatch
        XCTAssertTrue(newTab.waitForExistence(timeout: 8), "production composition shows New Tab")
        newTab.click()
    }

    private func showTabs(in app: XCUIApplication) {
        let tabs = app.descendants(matching: .any)["seyal-left-tabs"].firstMatch
        XCTAssertTrue(tabs.waitForExistence(timeout: 5))
        if tabs.isHittable {
            tabs.click()
        }
    }

    private func runPaletteCommand(in app: XCUIApplication, query: String) {
        let palette = app.descendants(matching: .any)["seyal-command-palette"]
        app.typeKey("k", modifierFlags: .command)
        XCTAssertTrue(palette.waitForExistence(timeout: 5), "⌘K opens the palette")
        let queryField = app.descendants(matching: .any)["seyal-command-palette-query"]
        XCTAssertTrue(queryField.waitForExistence(timeout: 5))
        queryField.firstMatch.click()
        queryField.firstMatch.typeText(query)
        let row = app.descendants(matching: .any)["seyal-command-palette-row-0"]
        XCTAssertTrue(row.waitForExistence(timeout: 5))
        XCTAssertEqual(row.label, query)
        app.typeKey("\r", modifierFlags: [])
        let closed = expectation(
            for: NSPredicate(format: "exists == false"),
            evaluatedWith: palette,
            handler: nil
        )
        XCTAssertEqual(XCTWaiter.wait(for: [closed], timeout: 5), .completed)
    }

    private func terminalTabButtons(in app: XCUIApplication) -> XCUIElementQuery {
        app.descendants(matching: .any).matching(
            NSPredicate(
                format: "identifier BEGINSWITH %@ AND label BEGINSWITH %@",
                "seyal-terminal-tab-",
                "Terminal "
            )
        )
    }

    private func waitForTerminalTabCount(
        _ app: XCUIApplication,
        _ count: Int,
        timeout: TimeInterval
    ) {
        let arrived = expectation(
            for: NSPredicate(format: "count == %d", count),
            evaluatedWith: terminalTabButtons(in: app),
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [arrived], timeout: timeout),
            .completed,
            "Expected \(count) Pane-local terminal tabs after the shortcut"
        )
    }

    private func waitForWindowCount(
        _ app: XCUIApplication,
        _ count: Int,
        timeout: TimeInterval
    ) {
        let arrived = expectation(
            for: NSPredicate(format: "count == %d", count),
            evaluatedWith: app.windows,
            handler: nil
        )
        XCTAssertEqual(XCTWaiter.wait(for: [arrived], timeout: timeout), .completed)
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
    }

    private func waitForRunLoop(_ seconds: TimeInterval) {
        RunLoop.current.run(until: Date().addingTimeInterval(seconds))
    }
}
