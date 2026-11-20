import XCTest

/// #1490 / ADR-029: Window → Workspaces sidebar → Pane → Surfaces.
/// Headed coverage for Create Workspace in the current window, identity-keyed
/// workspace rows, Raw resting, and focus staying on the presentation host.
@MainActor
final class SeyalSidebarWorkspaceUITests: XCTestCase {
    override func setUp() {
        continueAfterFailure = false
    }

    private func hostedApp() -> XCUIApplication {
        let app = XCUIApplication()
        app.launchIsolatedHost()
        return app
    }

    func testCreateWorkspaceOpensSecondSidebarRowInSameWindow() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        XCTAssertEqual(app.windows.count, 1)

        let create = app.descendants(matching: .any)["seyal-create-workspace"].firstMatch
        XCTAssertTrue(create.waitForExistence(timeout: 5), "Create Workspace chrome must project")
        XCTAssertTrue(create.isHittable)
        let before = workspaceRows(in: app).count
        XCTAssertGreaterThanOrEqual(before, 1)
        create.click()

        let deadline = Date().addingTimeInterval(12)
        var after = before
        while Date() < deadline {
            after = workspaceRows(in: app).count
            if after >= before + 1 {
                break
            }
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        }
        XCTAssertGreaterThanOrEqual(
            after,
            before + 1,
            "Create Workspace must add an open sidebar row without a second NSWindow"
        )
        XCTAssertEqual(app.windows.count, 1, "Create Workspace stays in the presentation host")
        XCTAssertEqual(app.state, .runningForeground)

        // Resting Raw: interactive terminal surface, no Flow composer route.
        let terminal = app.descendants(matching: .any)["terminal-input"].firstMatch
        XCTAssertTrue(terminal.waitForExistence(timeout: 8))
        let composer = app.descendants(matching: .any)["seyal-composer"].firstMatch
        if composer.exists {
            XCTAssertFalse(
                composer.isHittable,
                "Create Workspace must not restore a hittable Flow composer at rest"
            )
        }
    }

    func testWorkspaceRowSelectionAndPaneTerminalStayOnPresentationHost() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)

        let create = app.descendants(matching: .any)["seyal-create-workspace"].firstMatch
        XCTAssertTrue(create.waitForExistence(timeout: 5))
        create.click()
        waitForWorkspaceRowCount(in: app, atLeast: 2)

        let rows = workspaceRows(in: app)
        XCTAssertGreaterThanOrEqual(rows.count, 2)
        let first = rows.element(boundBy: 0)
        let second = rows.element(boundBy: 1)
        XCTAssertTrue(second.waitForExistence(timeout: 5))
        second.click()
        XCTAssertEqual(app.windows.count, 1)
        XCTAssertEqual(app.state, .runningForeground)

        // Pane-local New Terminal on the selected foreign/new workspace.
        let newTerminal = app.menuItems["New Terminal"]
        XCTAssertTrue(newTerminal.waitForExistence(timeout: 5))
        newTerminal.click()
        waitForTerminalTabCount(in: app, atLeast: 2)
        XCTAssertEqual(
            app.windows.count,
            1,
            "pane terminal create must keep the presentation host WindowId"
        )

        first.click()
        XCTAssertEqual(app.windows.count, 1)
        let terminal = app.descendants(matching: .any)["terminal-input"].firstMatch
        XCTAssertTrue(terminal.waitForExistence(timeout: 8))
        terminal.click()
        app.typeText("pwd\r")
        XCTAssertEqual(app.state, .runningForeground)
    }

    private func workspaceRows(in app: XCUIApplication) -> XCUIElementQuery {
        app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier MATCHES %@", #"^seyal-workspace-[0-9]+-[0-9]+$"#)
        )
    }

    private func waitForWorkspaceRowCount(in app: XCUIApplication, atLeast: Int) {
        let deadline = Date().addingTimeInterval(12)
        while Date() < deadline {
            if workspaceRows(in: app).count >= atLeast {
                return
            }
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        }
        XCTFail("expected >= \(atLeast) workspace rows; got \(workspaceRows(in: app).count)")
    }

    private func waitForTerminalTabCount(in app: XCUIApplication, atLeast: Int) {
        let tabs = app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier MATCHES %@", #"^seyal-terminal-tab-[0-9]+-[0-9]+$"#)
        )
        let deadline = Date().addingTimeInterval(12)
        while Date() < deadline {
            if tabs.count >= atLeast {
                return
            }
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        }
        XCTFail("expected >= \(atLeast) pane terminal tabs; got \(tabs.count)")
    }

    private func waitForUsablePty(in app: XCUIApplication, timeout: TimeInterval = 20) {
        XCTAssertTrue(app.wait(for: .runningForeground, timeout: 10))
        let presentation = app.descendants(matching: .any)["seyal-pane-presentation-host"].firstMatch
        let terminal = app.descendants(matching: .any)["terminal-input"]
        if presentation.waitForExistence(timeout: 2) {
            let hostUsable = NSPredicate(format: "value CONTAINS 'execution='")
            let hostReady = expectation(for: hostUsable, evaluatedWith: presentation, handler: nil)
            if XCTWaiter.wait(for: [hostReady], timeout: min(timeout, 8)) != .completed,
               terminal.waitForExistence(timeout: 2)
            {
                let usable = NSPredicate(format: "value CONTAINS 'connection=usable'")
                let arrived = expectation(for: usable, evaluatedWith: terminal, handler: nil)
                XCTAssertEqual(
                    XCTWaiter.wait(for: [arrived], timeout: timeout),
                    .completed,
                    "PTY/runtime never became usable; terminal AX=\(terminal.value ?? "nil")"
                )
            }
        } else {
            XCTAssertTrue(terminal.waitForExistence(timeout: 10), "terminal surface missing")
            let usable = NSPredicate(format: "value CONTAINS 'connection=usable'")
            let arrived = expectation(for: usable, evaluatedWith: terminal, handler: nil)
            XCTAssertEqual(
                XCTWaiter.wait(for: [arrived], timeout: timeout),
                .completed,
                "PTY/runtime never became usable; terminal AX=\(terminal.value ?? "nil")"
            )
        }
        XCTAssertEqual(app.state, .runningForeground)
    }
}
