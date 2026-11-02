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
        showWorkspaces(in: app)
        clickNewTab(in: app)
        waitForRunLoop(5)
        XCTAssertEqual(app.state, .runningForeground, "CreateTab must not crash Seyal.app")
        // Left Tabs mode is TerminalTab inventory (#1412); outer Tab count stays
        // on the Workspaces row secondary / close-tab affordance.
        waitForWorkspaceTabCountHint(in: app, atLeast: 2)
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
        // Prefer the projected File menu over XCUI typeKey: cmd equivalents are
        // asserted in MultiWindowHostTests, and typeKey flakes after chrome focus.
        clickFileMenuItem(in: app, title: "New Terminal")
        waitForWindowCount(app, 1, timeout: 5)
        waitForTerminalTabCount(app, 2, timeout: 12)
        XCTAssertEqual(app.windows.count, 1, "⌘T creates a Pane-local terminal, not a Layout or Window")

        clickFileMenuItem(in: app, title: "New Window")
        waitForWindowCount(app, 2, timeout: 10)

        // cmd+shift+n is an alias; File → New Window remains the primary admission.
        clickFileMenuItem(in: app, title: "New Window")
        waitForWindowCount(app, 3, timeout: 10)
    }

    func testCommandWClosesOnlySelectedPaneTerminal() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        XCTAssertEqual(app.windows.count, 1)
        // Prefer File menu for both create and close: pane-chrome "+" clicks
        // leave XCUI menu synthesis in a state where Close Terminal only hovers.
        clickFileMenuItem(in: app, title: "New Terminal")
        waitForTerminalTabCount(app, 2, timeout: 12)
        waitForUsablePty(in: app, timeout: 12)

        clickFileMenuItem(in: app, title: "Close Terminal")

        waitForTerminalTabCount(app, 1, timeout: 20)
        XCTAssertEqual(app.windows.count, 1, "⌘W closes the selected terminal, not its Window")
        XCTAssertTrue(
            app.descendants(matching: .any)["seyal-pane-region-0"].firstMatch.exists,
            "⌘W keeps the Pane present after removing its selected terminal"
        )
        waitForUsablePty(in: app, timeout: 12)
    }

    func testCloseTabRemovesChromeWithoutCrashingTheHost() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        showWorkspaces(in: app)
        clickNewTab(in: app)
        waitForRunLoop(5)
        waitForWorkspaceTabCountHint(in: app, atLeast: 2)
        let closeTab = app.descendants(matching: .any)["seyal-close-tab"].firstMatch
        XCTAssertTrue(closeTab.waitForExistence(timeout: 8))
        XCTAssertTrue(closeTab.isHittable, "Rust-enabled close control must be actionable")
        closeTab.click()
        XCTAssertEqual(app.state, .runningForeground)
        waitForWorkspaceTabCountHint(in: app, atLeast: 1, atMost: 1)
        waitForUsablePty(in: app, timeout: 12)
    }

    func testTerminateExecutionPaletteVerbKeepsHostAndTabChromeAlive() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        showWorkspaces(in: app)
        clickNewTab(in: app)
        waitForRunLoop(5)
        waitForWorkspaceTabCountHint(in: app, atLeast: 2)
        waitForUsablePty(in: app, timeout: 12)
        runPaletteCommand(in: app, query: "Terminate Execution")
        waitForRunLoop(1)
        XCTAssertEqual(app.state, .runningForeground)
        showWorkspaces(in: app)
        waitForWorkspaceTabCountHint(in: app, atLeast: 2)
    }

    private func clickNewTab(in app: XCUIApplication) {
        // CreateTab lives on the left + while Workspaces mode is selected (#1412
        // retargets + to NewTerminal only in Tabs mode).
        showWorkspaces(in: app)
        let newTab = app.descendants(matching: .any)["seyal-new-tab"].firstMatch
        XCTAssertTrue(newTab.waitForExistence(timeout: 8), "production composition shows New Tab")
        XCTAssertEqual(newTab.label.isEmpty ? newTab.title : newTab.label, "New Tab")
        newTab.click()
    }

    private func clickNewTerminal(in app: XCUIApplication) {
        let newTerminal = app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier BEGINSWITH %@", "seyal-pane-")
        ).matching(
            NSPredicate(format: "identifier ENDSWITH %@", "-new-terminal")
        ).firstMatch
        XCTAssertTrue(newTerminal.waitForExistence(timeout: 8), "New Terminal action must be available")
        newTerminal.click()
    }

    private func clickFileMenuItem(in app: XCUIApplication, title: String) {
        let item = app.menuBars.menuBarItems["File"].menuItems[title]
        XCTAssertTrue(item.waitForExistence(timeout: 5), "File → \(title) must be projected")
        XCTAssertTrue(item.isEnabled, "File → \(title) must be enabled")
        item.click()
    }

    private func showWorkspaces(in app: XCUIApplication) {
        let workspaces = app.descendants(matching: .any)["seyal-left-workspaces"].firstMatch
        XCTAssertTrue(workspaces.waitForExistence(timeout: 5))
        if workspaces.isHittable {
            workspaces.click()
        }
    }


    /// Outer Workspace Tab count is projected on the active workspace row after #1412.
    private func waitForWorkspaceTabCountHint(
        in app: XCUIApplication,
        atLeast: Int,
        atMost: Int? = nil
    ) {
        showWorkspaces(in: app)
        let row = app.descendants(matching: .any)["seyal-workspace-0"].firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 5))
        let deadline = Date().addingTimeInterval(12)
        var last = ""
        while Date() < deadline {
            last = row.label.isEmpty ? row.title : row.label
            let match = last.range(of: #"(\d+)\s+tabs?"#, options: .regularExpression)
            if let match {
                let digits = last[match].prefix { $0.isNumber }
                if let count = Int(digits), count >= atLeast,
                   atMost.map({ count <= $0 }) ?? true
                {
                    return
                }
            }
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        }
        XCTFail(
            "expected workspace secondary tab count >= \(atLeast)"
                + (atMost.map { " and <= \($0)" } ?? "")
                + "; label=\(last)"
        )
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
        // Match title, not AX label: tab buttons set `title` ("Terminal 1") and
        // leave accessibilityLabel unset; close buttons use title "×" / Close label.
        app.descendants(matching: .any).matching(
            NSPredicate(
                format: "identifier BEGINSWITH %@ AND title BEGINSWITH %@",
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
            "Expected \(count) Pane-local terminal tabs; actual=\(terminalTabButtons(in: app).count) windows=\(app.windows.count)\n\(app.debugDescription)"
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
