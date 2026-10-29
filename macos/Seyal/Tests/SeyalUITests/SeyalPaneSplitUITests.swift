import XCTest

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

        for strip in paneStrips {
            let splitRight = strip.buttons.matching(identifier: "seyal-split-right").firstMatch
            let splitDown = strip.buttons.matching(identifier: "seyal-split-down").firstMatch
            XCTAssertTrue(splitRight.exists)
            XCTAssertTrue(splitDown.exists)
            XCTAssertGreaterThan(splitRight.frame.minX, strip.frame.midX, "split controls sit on the Pane toolbar’s right side")
            XCTAssertLessThanOrEqual(splitRight.frame.maxX, splitDown.frame.minX, "split controls keep a consistent icon order")
            XCTAssertTrue(strip.frame.contains(splitRight.frame), "Split Right stays inside its Pane strip")
            XCTAssertTrue(strip.frame.contains(splitDown.frame), "Split Down stays inside its Pane strip")
            let tabViewport = strip.descendants(matching: .any).matching(
                identifier: "\(strip.identifier.replacingOccurrences(of: "-terminal-tabs", with: "-terminal-tab-viewport"))"
            ).firstMatch
            XCTAssertTrue(tabViewport.exists)
            XCTAssertTrue(strip.frame.contains(tabViewport.frame), "tab viewport stays inside its Pane strip")
            XCTAssertLessThanOrEqual(tabViewport.frame.maxX, splitRight.frame.minX - 8, "tab viewport reserves clear space before the toolbar")
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
        XCTAssertTrue(newTerminal.waitForExistence(timeout: 8))
        newTerminal.click()
        let twoTabs = expectation(for: NSPredicate(format: "count == 2"), evaluatedWith: terminalTabs, handler: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [twoTabs], timeout: 20), .completed, "New Terminal adds a pane-local tab")
        XCTAssertFalse(element(app, "seyal-pane-region-1").exists, "New Terminal does not split the PaneTree")
        waitForUsablePty(in: app, timeout: 20)

        let original = terminalTabs.element(boundBy: 0)
        let created = terminalTabs.element(boundBy: 1)
        XCTAssertNotEqual(original.identifier, created.identifier)
        original.click()
        XCTAssertEqual(original.value as? String, "selected", "selection returns to the original live terminal")
        XCTAssertEqual(created.value as? String, "", "the other terminal remains unselected")
        XCTAssertEqual(
            try XCTUnwrap(executionIdentity(terminalSurface.value as? String)),
            originalExecution,
            "the focused terminal surface returns to the original execution"
        )
        submitComposerCommand("echo SEYAL_TAB_ONE_MARKER", in: app)
        created.click()
        XCTAssertEqual(created.value as? String, "selected")
        let createdExecution = try XCTUnwrap(executionIdentity(terminalSurface.value as? String))
        XCTAssertNotEqual(createdExecution, originalExecution, "the selected terminal surface follows the selected tab")
        submitComposerCommand("echo SEYAL_TAB_TWO_MARKER", in: app)
        original.click()
        XCTAssertEqual(
            try XCTUnwrap(executionIdentity(terminalSurface.value as? String)),
            originalExecution,
            "switching back restores the original execution without replacing it"
        )
        XCTAssertTrue(blockCommand(in: app, containing: "SEYAL_TAB_ONE_MARKER").exists)
        XCTAssertFalse(blockCommand(in: app, containing: "SEYAL_TAB_TWO_MARKER").exists)
        created.click()
        XCTAssertTrue(blockCommand(in: app, containing: "SEYAL_TAB_TWO_MARKER").exists)
        XCTAssertFalse(blockCommand(in: app, containing: "SEYAL_TAB_ONE_MARKER").exists)

        click(app, "seyal-split-right")
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12))
        let secondPaneStrips = app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier CONTAINS %@ AND identifier CONTAINS %@", "-terminal-tabs", "seyal-pane-")
        )
        XCTAssertEqual(secondPaneStrips.count, 2, "each Pane owns its own terminal-tab strip")
        waitForFocus(app, region: 1)
        waitForUsablePty(in: app)
        let secondPaneExecution = try XCTUnwrap(executionIdentity(terminalSurface.value as? String))
        XCTAssertNotEqual(secondPaneExecution, originalExecution, "Split creates an independent terminal execution")
        XCTAssertNotEqual(secondPaneExecution, createdExecution, "split Pane does not reuse either terminal tab execution")
        XCTAssertEqual(terminalTabs.count, 3, "two terminal tabs stay in Pane 1 and Pane 2 starts with one")
        submitComposerCommand("echo SEYAL_PANE_TWO_MARKER", in: app)

        element(app, "seyal-pane-region-0").click()
        waitForFocus(app, region: 0)
        XCTAssertEqual(
            try XCTUnwrap(executionIdentity(terminalSurface.value as? String)),
            createdExecution,
            "returning to Pane 1 preserves its selected terminal"
        )
        XCTAssertFalse(blockCommand(in: app, containing: "SEYAL_PANE_TWO_MARKER").exists,
                       "Pane 2 command history stays out of Pane 1")
        element(app, "seyal-pane-region-1").click()
        waitForFocus(app, region: 1)
        XCTAssertEqual(
            try XCTUnwrap(executionIdentity(terminalSurface.value as? String)),
            secondPaneExecution,
            "returning to Pane 2 restores its independent execution"
        )
        XCTAssertTrue(blockCommand(in: app, containing: "SEYAL_PANE_TWO_MARKER").exists)

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

    /// One live terminal surface exists, and it sits inside the focused region.
    private func assertSingleLiveSurface(in app: XCUIApplication, focusedRegion index: Int) {
        let terminals = app.descendants(matching: .any).matching(identifier: "terminal-input")
        let live = expectation(for: NSPredicate(format: "count == 1"), evaluatedWith: terminals, handler: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [live], timeout: 12), .completed, "exactly one live terminal surface")
        let region = element(app, "seyal-pane-region-\(index)").frame
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

    private func click(_ app: XCUIApplication, _ identifier: String) {
        let control = element(app, identifier)
        XCTAssertTrue(control.waitForExistence(timeout: 8), "\(identifier) is shown")
        control.click()
    }

    private func waitForFocus(_ app: XCUIApplication, region index: Int) {
        let focused = expectation(
            for: NSPredicate(format: "value == 'focused'"),
            evaluatedWith: element(app, "seyal-pane-region-\(index)"),
            handler: nil
        )
        XCTAssertEqual(XCTWaiter.wait(for: [focused], timeout: 8), .completed, "region \(index) takes Rust focus")
    }

    private func waitForUsablePty(in app: XCUIApplication, timeout: TimeInterval = 20) {
        XCTAssertTrue(app.wait(for: .runningForeground, timeout: 10))
        let terminal = app.descendants(matching: .any)["terminal-input"]
        XCTAssertTrue(terminal.waitForExistence(timeout: 10), "terminal surface missing")
        let usable = NSPredicate(format: "value CONTAINS 'connection=usable'")
        let arrived = expectation(for: usable, evaluatedWith: terminal, handler: nil)
        let result = XCTWaiter.wait(for: [arrived], timeout: timeout)
        XCTAssertEqual(app.state, .runningForeground, "Seyal.app crashed while waiting for PTY attach")
        XCTAssertEqual(result, .completed, "PTY/runtime never became usable; terminal AX=\(terminal.value ?? "nil")")
    }

    private func executionIdentity(_ accessibilityValue: String?) -> String? {
        guard let accessibilityValue,
            let range = accessibilityValue.range(of: "execution="),
            let end = accessibilityValue[range.upperBound...].firstIndex(of: " ")
        else { return nil }
        return String(accessibilityValue[range.upperBound..<end])
    }

    private func submitComposerCommand(_ command: String, in app: XCUIApplication) {
        let editor = element(app, "seyal-composer-editor")
        editor.click()
        editor.typeText(command)
        let submit = element(app, "seyal-composer-execute")
        let enabled = expectation(for: NSPredicate(format: "isEnabled == true"), evaluatedWith: submit, handler: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [enabled], timeout: 5), .completed)
        submit.click()
        let block = blockCommand(in: app, containing: command)
        XCTAssertTrue(block.waitForExistence(timeout: 10), "the selected execution records its own submitted command")
    }

    private func blockCommand(in app: XCUIApplication, containing command: String) -> XCUIElement {
        app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier BEGINSWITH %@ AND label CONTAINS %@", "seyal-block-", command)
        ).firstMatch
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
