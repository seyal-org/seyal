import AppKit
import XCTest

/// Headed coverage for the Seyal Block Component (#1010): hover/selection
/// reveal the quick actions, Copy puts Rust-built text on the pasteboard and
/// Rerun submits through the composer. Screenshots feed the design doc's
/// visual-regression matrix (M003-BLOCK-COMPONENT-DESIGN §11).
@MainActor
final class SeyalBlockComponentUITests: XCTestCase {
    override func setUp() {
        continueAfterFailure = false
    }

    func testBlockQuickActionsCopyAndRerun() throws {
        try requireZshLoginShell()
        let app = XCUIApplication()
        app.launchIsolatedHost()
        waitForUsablePty(in: app)

        submitComposerCommand(app, "printf 'seyal-1010-alpha\\nseyal-1010-beta\\n'")
        submitComposerCommand(app, "ls /seyal-1010-missing")
        let first = app.descendants(matching: .any)["seyal-block-0"].firstMatch
        XCTAssertTrue(first.waitForExistence(timeout: 10), "first Block missing")
        XCTAssertTrue(
            app.descendants(matching: .any)["seyal-block-1"].firstMatch.waitForExistence(timeout: 10))
        waitBriefly(1.0)
        attachScreenshot(app, name: "1010-rest")

        first.hover()
        let copy = first.descendants(matching: .any)["seyal-block-action-copy"].firstMatch
        XCTAssertTrue(copy.waitForExistence(timeout: 3), "hover must reveal quick actions")
        attachScreenshot(app, name: "1010-hover")

        NSPasteboard.general.clearContents()
        copy.click()
        attachScreenshot(app, name: "1010-copy-menu")
        app.menuItems["Copy command"].firstMatch.click()
        XCTAssertEqual(
            waitForPasteboard { $0 == "printf 'seyal-1010-alpha\\nseyal-1010-beta\\n'" },
            "printf 'seyal-1010-alpha\\nseyal-1010-beta\\n'")

        NSPasteboard.general.clearContents()
        first.hover()
        copy.click()
        app.menuItems["Copy output"].firstMatch.click()
        let output = waitForPasteboard { $0.contains("seyal-1010-beta") }
        XCTAssertTrue(output.contains("seyal-1010-alpha\nseyal-1010-beta"), "output=\(output)")

        first.click()
        let selected = NSPredicate(format: "value == 'selected'")
        let becameSelected = expectation(for: selected, evaluatedWith: first, handler: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [becameSelected], timeout: 5), .completed)
        attachScreenshot(app, name: "1010-selected")

        let rerun = first.descendants(matching: .any)["seyal-block-action-rerun"].firstMatch
        XCTAssertTrue(rerun.waitForExistence(timeout: 3))
        XCTAssertTrue(rerun.isEnabled, "completed Block offers Rerun while the composer is available")
        rerun.click()
        XCTAssertTrue(
            app.descendants(matching: .any)["seyal-block-2"].firstMatch.waitForExistence(timeout: 10),
            "Rerun must submit a new Block through the composer")
        XCTAssertEqual(app.state, .runningForeground)
    }

    func testRunningBlockWithholdsRerun() throws {
        try requireZshLoginShell()
        let app = XCUIApplication()
        app.launchIsolatedHost()
        waitForUsablePty(in: app)
        submitComposerCommand(app, "sleep 20")
        let running = app.descendants(matching: .any)["seyal-block-0"].firstMatch
        XCTAssertTrue(running.waitForExistence(timeout: 10))
        // Resolve through the app each time: running Block cards are rebuilt
        // as Rust republishes rows, so element references must not be cached.
        let block = { app.descendants(matching: .any)["seyal-block-0"].firstMatch }
        block().hover()
        let status = block().descendants(matching: .any)["seyal-block-status"].firstMatch
        XCTAssertTrue(status.waitForExistence(timeout: 5), "running status missing")
        XCTAssertEqual(status.label, "Running")
        block().hover()
        let rerun = block().descendants(matching: .any)["seyal-block-action-rerun"].firstMatch
        XCTAssertTrue(rerun.waitForExistence(timeout: 5), "hover must reveal actions on a running Block")
        XCTAssertFalse(rerun.isEnabled, "a running Block never offers Rerun")
        attachScreenshot(app, name: "1010-running")
        app.typeKey("c", modifierFlags: [.control])
    }

    /// Blocks exist only under Seyal's trusted zsh integration (ADR-009
    /// mechanism 6); any other login shell stays on the raw path by design.
    /// Hosted runners log in with bash, so their Block evidence is the Rust
    /// app/FFI tests plus the live-zsh Runtime tests; this headed case runs
    /// on zsh machines.
    private func requireZshLoginShell() throws {
        guard let account = getpwuid(geteuid()), let shell = account.pointee.pw_shell,
            URL(fileURLWithPath: String(cString: shell)).lastPathComponent == "zsh"
        else {
            throw XCTSkip("Block quick actions need a zsh login shell (trusted integration)")
        }
    }

    private func waitForPasteboard(
        timeout: TimeInterval = 8, _ matches: (String) -> Bool
    ) -> String {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if let text = NSPasteboard.general.string(forType: .string), matches(text) {
                return text
            }
            waitBriefly(0.1)
        }
        return NSPasteboard.general.string(forType: .string) ?? ""
    }

    private func submitComposerCommand(_ app: XCUIApplication, _ command: String) {
        let composer = app.descendants(matching: .any)["seyal-composer"]
        XCTAssertTrue(composer.waitForExistence(timeout: 12))
        let ready = expectation(
            for: NSPredicate(format: "value == 'available'"), evaluatedWith: composer, handler: nil)
        XCTAssertEqual(
            XCTWaiter.wait(for: [ready], timeout: 12), .completed,
            "composer never became available; value=\(composer.value ?? "nil")")
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
            evaluatedWith: editor.firstMatch, handler: nil)
        _ = XCTWaiter.wait(for: [cleared], timeout: 8)
    }

    private func waitForUsablePty(in app: XCUIApplication, timeout: TimeInterval = 20) {
        XCTAssertTrue(app.wait(for: .runningForeground, timeout: 10))
        let terminal = app.descendants(matching: .any)["terminal-input"]
        XCTAssertTrue(terminal.waitForExistence(timeout: 10), "terminal surface missing")
        let usable = expectation(
            for: NSPredicate(format: "value CONTAINS 'connection=usable'"),
            evaluatedWith: terminal, handler: nil)
        XCTAssertEqual(
            XCTWaiter.wait(for: [usable], timeout: timeout), .completed,
            "PTY/runtime never became usable; terminal AX=\(terminal.value ?? "nil")")
    }

    private func attachScreenshot(_ app: XCUIApplication, name: String) {
        let attachment = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    private func waitBriefly(_ seconds: TimeInterval) {
        RunLoop.current.run(until: Date().addingTimeInterval(seconds))
    }
}
