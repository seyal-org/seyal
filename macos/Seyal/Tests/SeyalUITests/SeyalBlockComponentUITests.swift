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

    /// Keyboard reachability: select a Block via AX press (Rust SelectBlock),
    /// then confirm seam actions are labelled and in the accessibility tree.
    /// Block-to-Block ⌘↑/⌘↓ is deliberately not covered — deferred to SPEC-024.
    /// #1010 Done gate (design §11): the headed Block-content matrix in both
    /// appearances, with Metal-composited output cells painted in the same
    /// Rust canvas colour as the Block surface (#1062 / PR #1305).
    func testBlockContentMatrixInDarkAndLightAppearance() throws {
        try requireZshLoginShell()
        for appearance in ["dark", "light"] {
            let dir = FileManager.default.temporaryDirectory
                .appendingPathComponent("seyal-1010-\(appearance)-\(UUID().uuidString)", isDirectory: true)
            try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
            defer { try? FileManager.default.removeItem(at: dir) }
            let config = dir.appendingPathComponent("config.toml")
            try "[ui]\nappearance = \"\(appearance)\"\n".write(to: config, atomically: true, encoding: .utf8)

            let app = XCUIApplication()
            app.launchIsolatedHost(environment: ["SEYAL_CONFIG": config.path])
            waitForUsablePty(in: app)
            submitComposerCommand(app, "printf 'seyal-1010-alpha\\nseyal-1010-beta\\n'")
            submitComposerCommand(app, "ls /seyal-1010-missing")
            let success = app.descendants(matching: .any)["seyal-block-0"].firstMatch
            let failed = app.descendants(matching: .any)["seyal-block-1"].firstMatch
            XCTAssertTrue(success.waitForExistence(timeout: 10), "\(appearance): success Block missing")
            XCTAssertTrue(failed.waitForExistence(timeout: 10), "\(appearance): failed Block missing")
            waitBriefly(1.0)
            assertMetalCellsMatchBlockCanvas(app, block: success, appearance: appearance)
            attachScreenshot(app, name: "1010-content-\(appearance)-rest")

            success.hover()
            let copy = success.descendants(matching: .any)["seyal-block-action-copy"].firstMatch
            XCTAssertTrue(copy.waitForExistence(timeout: 3), "\(appearance): hover reveals quick actions")
            attachScreenshot(app, name: "1010-content-\(appearance)-hover")
            copy.click()
            attachScreenshot(app, name: "1010-content-\(appearance)-copy-menu")
            app.typeKey(.escape, modifierFlags: [])

            success.click()
            let selected = expectation(
                for: NSPredicate(format: "value == 'selected'"), evaluatedWith: success, handler: nil)
            XCTAssertEqual(XCTWaiter.wait(for: [selected], timeout: 5), .completed)
            attachScreenshot(app, name: "1010-content-\(appearance)-selected")

            submitComposerCommand(app, "sleep 30")
            let running = app.descendants(matching: .any)["seyal-block-2"].firstMatch
            XCTAssertTrue(running.waitForExistence(timeout: 10), "\(appearance): running Block missing")
            waitBriefly(0.5)
            attachScreenshot(app, name: "1010-content-\(appearance)-running")
            app.terminate()
        }
    }

    func testBlockKeyboardAndVoiceOverReachability() throws {
        try requireZshLoginShell()
        let app = XCUIApplication()
        app.launchIsolatedHost()
        waitForUsablePty(in: app)
        submitComposerCommand(app, "printf 'seyal-1010-a11y\\n'")
        let first = app.descendants(matching: .any)["seyal-block-0"].firstMatch
        XCTAssertTrue(first.waitForExistence(timeout: 10), "first Block missing")
        XCTAssertFalse(first.label.isEmpty, "VoiceOver label must come from the Block command")
        first.press(forDuration: 0.05)
        let selected = NSPredicate(format: "value == 'selected'")
        let becameSelected = expectation(for: selected, evaluatedWith: first, handler: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [becameSelected], timeout: 5), .completed)
        let copy = first.descendants(matching: .any)["seyal-block-action-copy"].firstMatch
        XCTAssertTrue(copy.waitForExistence(timeout: 3), "selected Block must expose actions")
        XCTAssertEqual(copy.label, "Copy")
        let rerun = first.descendants(matching: .any)["seyal-block-action-rerun"].firstMatch
        XCTAssertTrue(rerun.waitForExistence(timeout: 2))
        XCTAssertEqual(rerun.label, "Rerun")
        // Tab into the key-view loop of the selected Block's actions.
        app.typeKey("\t", modifierFlags: [])
        XCTAssertTrue(copy.exists)
        XCTAssertEqual(app.state, .runningForeground)
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

    /// Compare a Metal-composited output cell (an empty cell right of the
    /// printed text on the Block's last output row) with the Block surface
    /// (top-line padding, AppKit). Both must be the Rust canvas colour, and
    /// the canvas brightness must match the requested appearance.
    private func assertMetalCellsMatchBlockCanvas(
        _ app: XCUIApplication, block: XCUIElement, appearance: String,
        file: StaticString = #filePath, line: UInt = #line
    ) {
        let window = app.windows.firstMatch
        guard let image = window.screenshot().image.cgImage(forProposedRect: nil, context: nil, hints: nil)
        else { return XCTFail("window screenshot unavailable", file: file, line: line) }
        let bitmap = NSBitmapImageRep(cgImage: image)
        let frame = window.frame
        let scale = CGFloat(image.width) / frame.width
        func sample(_ point: CGPoint) -> NSColor? {
            let x = Int(((point.x - frame.minX) * scale).rounded(.down))
            let y = Int(((point.y - frame.minY) * scale).rounded(.down))
            return bitmap.colorAt(x: x, y: y)?.usingColorSpace(.sRGB)
        }
        let rect = block.frame
        guard
            let canvas = sample(CGPoint(x: rect.minX + 6, y: rect.minY + 6)),
            let cell = sample(CGPoint(x: rect.minX + 14 + 260, y: rect.maxY - 14))
        else { return XCTFail("pixels outside the screenshot", file: file, line: line) }
        let delta = max(
            abs(canvas.redComponent - cell.redComponent),
            abs(canvas.greenComponent - cell.greenComponent),
            abs(canvas.blueComponent - cell.blueComponent))
        XCTAssertLessThanOrEqual(
            delta, 8.0 / 255.0,
            "\(appearance): Metal cell \(cell) must match the Block canvas \(canvas)",
            file: file, line: line)
        let luminance = 0.2126 * canvas.redComponent + 0.7152 * canvas.greenComponent
            + 0.0722 * canvas.blueComponent
        if appearance == "light" {
            XCTAssertGreaterThan(luminance, 0.6, "light canvas expected; got \(canvas)", file: file, line: line)
        } else {
            XCTAssertLessThan(luminance, 0.4, "dark canvas expected; got \(canvas)", file: file, line: line)
        }
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
