import XCTest

/// Issue #928 headed evidence: with production pane splitting (C3), dragging
/// a Split divider in Seyal.app updates the Rust ratio (projected as the
/// divider's AX value) and the region frames, clamps at Rust's bounds, and
/// keeps exactly one live terminal surface inside the focused region.
@MainActor
final class SeyalDividerDragUITests: XCTestCase {
    override func setUp() {
        continueAfterFailure = false
    }

    private func hostedApp() -> XCUIApplication {
        let app = XCUIApplication()
        app.launchIsolatedHost()
        return app
    }

    func testDraggingDividerResizesRegionsThroughRust() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        click(app, "seyal-split-right")
        let divider = element(app, "seyal-pane-divider-0")
        XCTAssertTrue(divider.waitForExistence(timeout: 12), "Split Right projects a divider")
        XCTAssertEqual(divider.value as? String, "0.50")
        let leading = element(app, "seyal-pane-region-0")
        let trailing = element(app, "seyal-pane-region-1")
        let area = leading.frame.union(trailing.frame)
        let leadingBefore = leading.frame.width

        drag(divider, toX: area.minX + area.width * 0.3)
        waitForValue(divider, "0.30")
        XCTAssertLessThan(leading.frame.width, leadingBefore, "the leading region narrows")
        XCTAssertGreaterThan(trailing.frame.width, leading.frame.width, "the trailing region takes the space")
        XCTAssertEqual(app.state, .runningForeground, "a divider drag must not crash Seyal.app")
        assertSingleLiveSurfaceInFocusedRegion(app, count: 2)
    }

    func testDividerDragClampsAtRustBounds() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        click(app, "seyal-split-right")
        let divider = element(app, "seyal-pane-divider-0")
        XCTAssertTrue(divider.waitForExistence(timeout: 12))
        let area = element(app, "seyal-pane-region-0").frame.union(element(app, "seyal-pane-region-1").frame)

        drag(divider, toX: area.minX + 2)
        waitForValue(divider, "0.10")
        drag(divider, toX: area.maxX - 2)
        waitForValue(divider, "0.90")
        XCTAssertGreaterThan(element(app, "seyal-pane-region-1").frame.width, 0, "a clamped Split keeps both regions")
        assertSingleLiveSurfaceInFocusedRegion(app, count: 2)
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

    /// Press on the divider's centre and drag horizontally to screen `x`.
    private func drag(_ divider: XCUIElement, toX x: CGFloat) {
        let start = divider.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
        let end = start.withOffset(CGVector(dx: x - divider.frame.midX, dy: 0))
        start.press(forDuration: 0.2, thenDragTo: end, withVelocity: 300, thenHoldForDuration: 0.3)
    }

    private func waitForValue(_ element: XCUIElement, _ value: String) {
        let matched = expectation(
            for: NSPredicate(format: "value == %@", value),
            evaluatedWith: element,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [matched], timeout: 8),
            .completed,
            "divider ratio should be \(value); got \(element.value ?? "nil")"
        )
    }

    private func assertSingleLiveSurfaceInFocusedRegion(_ app: XCUIApplication, count: Int) {
        let focused = (0..<count).filter { element(app, "seyal-pane-region-\($0)").value as? String == "focused" }
        XCTAssertEqual(focused.count, 1, "exactly one focused region")
        guard let index = focused.first else { return }
        let terminals = app.descendants(matching: .any).matching(identifier: "terminal-input")
        let one = expectation(for: NSPredicate(format: "count == 1"), evaluatedWith: terminals, handler: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [one], timeout: 12), .completed, "exactly one live terminal surface")
        let region = element(app, "seyal-pane-region-\(index)").frame
        XCTAssertTrue(
            region.insetBy(dx: -1, dy: -1).contains(terminals.firstMatch.frame),
            "live surface must sit inside the focused region \(region)"
        )
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
}
