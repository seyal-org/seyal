import XCTest

@MainActor
extension XCTestCase {
    func exerciseBoundedAlternateScreen(
        in app: XCUIApplication, submitCommand: (String) -> Void
    ) throws {
        let token = String(UUID().uuidString.prefix(8))
        let scriptURL = URL(fileURLWithPath: "/tmp/s1049-\(token).sh")
        let startedURL = URL(fileURLWithPath: "/tmp/s1049-\(token).ran")
        try """
        trap 'printf "\\033[?1049l"' EXIT
        : > \(startedURL.path)
        printf '\\033[?1049h'
        read -r -t 20 answer
        """.write(to: scriptURL, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes(
            [.posixPermissions: 0o700], ofItemAtPath: scriptURL.path)
        let composer = app.descendants(matching: .any)["seyal-composer"].firstMatch
        let terminal = app.descendants(matching: .any)["terminal-input"].firstMatch
        defer {
            if app.state == .runningForeground, composer.exists, !composer.isHittable {
                if terminal.isHittable {
                    terminal.click()
                }
                app.typeKey("\r", modifierFlags: [])
                let stopped = expectation(
                    for: NSPredicate(format: "isHittable == true"),
                    evaluatedWith: composer, handler: nil)
                XCTAssertEqual(XCTWaiter.wait(for: [stopped], timeout: 5), .completed)
            }
            do {
                try FileManager.default.removeItem(at: scriptURL)
                if FileManager.default.fileExists(atPath: startedURL.path) {
                    try FileManager.default.removeItem(at: startedURL)
                }
            } catch {
                XCTFail("Could not remove alternate-screen fixture: \(error)")
            }
        }
        submitCommand("/bin/bash --noprofile --norc \(scriptURL.path)")
        let startedDeadline = Date().addingTimeInterval(8)
        while Date() < startedDeadline,
            !FileManager.default.fileExists(atPath: startedURL.path)
        {
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        }
        let started = FileManager.default.fileExists(atPath: startedURL.path)
        let entered = expectation(
            for: NSPredicate(format: "isHittable == false"), evaluatedWith: composer, handler: nil)
        XCTAssertEqual(
            XCTWaiter.wait(for: [entered], timeout: 12),
            .completed,
            "alt-screen must hide composer; started=\(started) composer=\(composer.value ?? "nil") terminal=\(terminal.value ?? "nil") hittable=\(composer.isHittable)"
        )
        XCTAssertEqual(app.state, .runningForeground)
        XCTAssertTrue(terminal.waitForExistence(timeout: 5))
        terminal.click()
        app.typeKey("\r", modifierFlags: [])
        let returned = expectation(
            for: NSPredicate(format: "isHittable == true"), evaluatedWith: composer, handler: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [returned], timeout: 12), .completed)
    }
}

@MainActor
final class SeyalHostUITests: XCTestCase {
    override func setUp() {
        continueAfterFailure = false
    }

    /// Kill a leftover Seyal.app from the previous case before launch.
    /// Must run on the test's MainActor isolation, not XCTest's sync tearDown.
    private func hostedApp() -> XCUIApplication {
        let app = XCUIApplication()
        app.launchIsolatedHost()
        return app
    }

    func testApplicationLaunchesOnePaneHost() throws {
        let app = hostedApp()
        XCTAssertTrue(app.wait(for: .runningForeground, timeout: 10))
        // Attach/resize used to recurse into SIGSEGV within ~250ms of a real
        // `open`. Chrome existence is not enough; the process must stay up.
        RunLoop.current.run(until: Date().addingTimeInterval(2))
        XCTAssertEqual(app.state, .runningForeground, "Seyal.app crashed after launch")
        let chrome = app.descendants(matching: .any)["seyal-product-chrome"]
        XCTAssertTrue(chrome.waitForExistence(timeout: 10))
        let pane = app.descendants(matching: .any)["seyal-thin-pane"]
        XCTAssertTrue(pane.waitForExistence(timeout: 10))
        XCTAssertTrue(app.descendants(matching: .any)["seyal-recovery"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.descendants(matching: .any)["terminal-input"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.descendants(matching: .any)["seyal-composer"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.descendants(matching: .any)["seyal-blocks"].waitForExistence(timeout: 5))
        let transcript = app.descendants(matching: .any)["seyal-blocks-scroll"]
        XCTAssertTrue(transcript.waitForExistence(timeout: 5))
        XCTAssertGreaterThan(
            transcript.firstMatch.frame.height,
            120,
            "Flow transcript must fill the Pane, not sit above a Metal viewport"
        )
        XCTAssertLessThan(
            transcript.firstMatch.frame.width,
            chrome.firstMatch.frame.width,
            "Core Terminal left panel/inspector are visible by default and reclaim real width"
        )
        XCTAssertGreaterThan(
            transcript.firstMatch.frame.width,
            300,
            "Flow transcript must remain a usable center column width"
        )
        XCTAssertTrue(
            app.descendants(matching: .any)["seyal-left-workspaces"].firstMatch.isHittable,
            "Core Terminal left panel is visible by default (#922)"
        )
        waitForUsablePty(in: app)
    }

    /// #993: cold `SEYAL_CONFIG` TOML must drive Rust-resolved appearance /
    /// font size / padding / material preference into the headed host.
    func testColdConfigTomlDrivesVisibleAppearanceFontsAndPadding() throws {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("seyal-993-ui-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let config = dir.appendingPathComponent("config.toml")
        try """
        [ui]
        appearance = "light"
        reduced-material = false
        window-padding = 12
        [ui.font]
        size = 16
        [terminal]
        padding = 14
        [terminal.font]
        size = 18
        """.write(to: config, atomically: true, encoding: .utf8)

        let app = XCUIApplication()
        app.launchIsolatedHost(environment: ["SEYAL_CONFIG": config.path])
        XCTAssertTrue(app.wait(for: .runningForeground, timeout: 10))
        let chrome = app.descendants(matching: .any)["seyal-product-chrome"]
        XCTAssertTrue(chrome.waitForExistence(timeout: 10))
        // Probe lives on a dedicated AX element (not the chrome group value).
        let expectedId = "seyal-cold-visual-probe.light.16.18.12.14.frosted"
        let probe = app.descendants(matching: .any)[expectedId]
        let deadline = Date().addingTimeInterval(8)
        while Date() < deadline {
            if probe.exists { break }
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        }
        XCTAssertTrue(
            probe.exists,
            "headed host must realize Rust cold-config visual snapshot (\(expectedId))"
        )
        // AppKit often exposes an empty string rather than nil for unset AX values.
        let chromeValue = (chrome.firstMatch.value as? String) ?? ""
        XCTAssertTrue(
            chromeValue.isEmpty || !chromeValue.contains("seyal-cold-visual-probe"),
            "product chrome must not expose the encoded test probe as its AX value; got \(chromeValue)"
        )
        XCTAssertTrue(app.descendants(matching: .any)["seyal-composer"].waitForExistence(timeout: 5))
        XCTAssertEqual(app.state, .runningForeground)
    }

    /// Hygiene #962: after removing orphaned Metal self-test scaffolding, the
    /// headed host still exposes the interactive terminal input surface.
    func testInteractiveMetalSurfaceRemainsAvailableWithoutSelfTestScaffolding() throws {
        let app = hostedApp()
        XCTAssertTrue(app.wait(for: .runningForeground, timeout: 10))
        let terminal = app.descendants(matching: .any)["terminal-input"]
        XCTAssertTrue(terminal.waitForExistence(timeout: 10))
        XCTAssertTrue(app.descendants(matching: .any)["seyal-thin-pane"].waitForExistence(timeout: 5))
        RunLoop.current.run(until: Date().addingTimeInterval(1))
        XCTAssertEqual(app.state, .runningForeground, "Seyal.app crashed while hosting Metal input")
        waitForUsablePty(in: app)
        XCTAssertTrue(terminal.exists)
    }

    func testFlowSurfaceIsComposerAndBlocksAlongsideCoreTerminalChrome() throws {
        let app = hostedApp()
        XCTAssertTrue(app.descendants(matching: .any)["seyal-composer"].waitForExistence(timeout: 10))
        XCTAssertTrue(app.descendants(matching: .any)["seyal-blocks-scroll"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.descendants(matching: .any)["seyal-blocks"].waitForExistence(timeout: 5))
        // Core Terminal chrome (#922): left panel, tab strip, and inspector
        // are visible by default alongside the Flow composer/Blocks surface.
        XCTAssertTrue(app.descendants(matching: .any)["seyal-inspector"].firstMatch.isHittable)
        XCTAssertTrue(app.descendants(matching: .any)["seyal-tab-strip"].firstMatch.isHittable)
        XCTAssertTrue(app.descendants(matching: .any)["seyal-left-tabs"].firstMatch.isHittable)
        XCTAssertTrue(app.descendants(matching: .any)["seyal-composer-execute"].waitForExistence(timeout: 5))
    }

    func testComposerSubmitAndTerminalFocusStayOnRustEligibility() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        let composer = app.descendants(matching: .any)["seyal-composer"]
        XCTAssertTrue(composer.waitForExistence(timeout: 12))
        let composerReady = NSPredicate(format: "value == 'available'")
        let becameReady = expectation(for: composerReady, evaluatedWith: composer, handler: nil)
        XCTAssertEqual(
            XCTWaiter.wait(for: [becameReady], timeout: 12),
            .completed,
            "composer never became submittable after PTY attach; value=\(composer.value ?? "nil")"
        )
        composer.firstMatch.click()
        let editor = app.descendants(matching: .any)["seyal-composer-editor"]
        if editor.waitForExistence(timeout: 2), editor.firstMatch.isHittable {
            editor.firstMatch.click()
            editor.firstMatch.typeText("echo seyal-usable-gate")
            editor.firstMatch.typeKey("\r", modifierFlags: [])
        } else {
            composer.firstMatch.typeText("echo seyal-usable-gate")
            app.typeKey("\r", modifierFlags: [])
        }
        XCTAssertEqual(app.state, .runningForeground, "Seyal.app crashed on composer submit")
        XCTAssertTrue(app.descendants(matching: .any)["seyal-thin-pane"].exists)
        XCTAssertTrue(app.descendants(matching: .any)["seyal-blocks"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.descendants(matching: .any)["seyal-blocks-scroll"].waitForExistence(timeout: 5))
        // Command blocks are a Rust composer projection of Runtime
        // timeline metadata (OSC 133 / SPEC-008), not PTY bytes copied into
        // Swift. Echo in the Metal pane is not a Block. This test proves the
        // host accepted the composer submit; Blocks appear when Runtime
        // publishes a timeline revision.
        let accepted = NSPredicate(format: "value == nil OR value == ''")
        let cleared = expectation(for: accepted, evaluatedWith: editor.firstMatch, handler: nil)
        XCTAssertEqual(
            XCTWaiter.wait(for: [cleared], timeout: 8),
            .completed,
            "composer submit did not accept the draft; editor=\(editor.firstMatch.value ?? "nil")"
        )
        waitForUsablePty(in: app, timeout: 8)
        let output = app.descendants(matching: .any)["seyal-block-0-body"]
        if output.waitForExistence(timeout: 6) {
            XCTAssertGreaterThan(
                output.firstMatch.frame.height,
                8,
                "Block body must reserve a Metal output region, not only the command header"
            )
        }
    }

    /// #978 demo procedure: the composer is enabled only by Runtime's
    /// published eligibility. It reads `available` at the prompt, `busy` while
    /// `sleep 2` occupies the shell, and `available` again at the next prompt.
    ///
    /// Requires a zsh login shell: the headed host spawns the account's
    /// `pw_shell` (`BundledRuntimeLauncher`), and only trusted zsh integration
    /// publishes `Busy`; any other shell is `Unsupported` and keeps the composer
    /// on the raw path by design (ADR-009 mechanism 6). Hosted runners use a
    /// bash account, so their busy/available evidence is the Rust live tests,
    /// which spawn `/bin/zsh` explicitly.
    func testComposerReadsBusyWhileCommandRunsAndAvailableAtNextPrompt() throws {
        guard loginShellIsZsh() else {
            throw XCTSkip(
                "This composer-eligibility case requires a zsh login shell; the host spawns pw_shell.")
        }
        let app = hostedApp()
        waitForUsablePty(in: app)
        let composer = app.descendants(matching: .any)["seyal-composer"]
        XCTAssertTrue(composer.waitForExistence(timeout: 12))
        let available = NSPredicate(format: "value == 'available'")
        let busy = NSPredicate(format: "value == 'busy'")
        XCTAssertEqual(
            XCTWaiter.wait(
                for: [expectation(for: available, evaluatedWith: composer, handler: nil)],
                timeout: 12
            ),
            .completed,
            "composer never became available at the first prompt; value=\(composer.value ?? "nil")"
        )
        composer.firstMatch.click()
        let editor = app.descendants(matching: .any)["seyal-composer-editor"]
        if editor.waitForExistence(timeout: 2), editor.firstMatch.isHittable {
            editor.firstMatch.click()
            editor.firstMatch.typeText("sleep 2")
            editor.firstMatch.typeKey("\r", modifierFlags: [])
        } else {
            composer.firstMatch.typeText("sleep 2")
            app.typeKey("\r", modifierFlags: [])
        }
        // Runtime publishes Busy at admission; the host only relays it.
        XCTAssertEqual(
            XCTWaiter.wait(
                for: [expectation(for: busy, evaluatedWith: composer, handler: nil)],
                timeout: 4
            ),
            .completed,
            "composer stayed \(composer.value ?? "nil") while sleep 2 owned the shell"
        )
        XCTAssertEqual(app.state, .runningForeground, "Seyal.app crashed during busy relay")
        // The next trusted prompt re-enables it, with the draft cleared by
        // the accepted submit.
        XCTAssertEqual(
            XCTWaiter.wait(
                for: [expectation(for: available, evaluatedWith: composer, handler: nil)],
                timeout: 10
            ),
            .completed,
            "composer did not return to available after sleep 2; value=\(composer.value ?? "nil")"
        )
        assertFlowBlocksOrFail(in: app)
    }

    func testAlternateScreenTakeoverDoesNotCrashTheHost() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        try exerciseBoundedAlternateScreen(in: app) { command in
            submitComposerCommand(app, command)
        }
        assertFlowBlocksOrFail(in: app)
    }

    /// Metal does not expose PTY bytes as AX text. The live connection token on
    /// `terminal-input` is the host-observable proof that Runtime attached.

    func testComposerHistoryRecallOpensFiltersInsertsAndDismisses() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        let composer = app.descendants(matching: .any)["seyal-composer"]
        XCTAssertTrue(composer.waitForExistence(timeout: 12))
        let composerReady = NSPredicate(format: "value == 'available'")
        let becameReady = expectation(for: composerReady, evaluatedWith: composer, handler: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [becameReady], timeout: 12), .completed)
        let editor = app.descendants(matching: .any)["seyal-composer-editor"]
        XCTAssertTrue(editor.waitForExistence(timeout: 5))
        editor.firstMatch.click()
        let overlay = app.descendants(matching: .any)["seyal-composer-history"]
        let toggle = app.descendants(matching: .any)["seyal-composer-history-toggle"]
        XCTAssertTrue(toggle.waitForExistence(timeout: 5))
        XCTAssertFalse(toggle.firstMatch.isEnabled, "no accepted submit yet: recall is disabled")
        editor.firstMatch.typeKey("r", modifierFlags: [.control])
        XCTAssertFalse(overlay.firstMatch.isHittable, "Rust rejects open with empty history")

        editor.firstMatch.typeText("echo seyal-history-recall")
        editor.firstMatch.typeKey("\r", modifierFlags: [])
        let cleared = expectation(
            for: NSPredicate(format: "value == nil OR value == ''"),
            evaluatedWith: editor.firstMatch,
            handler: nil
        )
        XCTAssertEqual(XCTWaiter.wait(for: [cleared], timeout: 8), .completed)
        let readyAgain = expectation(for: composerReady, evaluatedWith: composer, handler: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [readyAgain], timeout: 12), .completed)

        editor.firstMatch.click()
        editor.firstMatch.typeKey("r", modifierFlags: [.control])
        XCTAssertTrue(overlay.waitForExistence(timeout: 5))
        XCTAssertTrue(overlay.firstMatch.isHittable, "⌃R opens the Rust history overlay")
        let query = app.descendants(matching: .any)["seyal-composer-history-query"]
        XCTAssertTrue(query.waitForExistence(timeout: 5))
        let row = app.descendants(matching: .any)["seyal-composer-history-row-0"]
        XCTAssertTrue(row.waitForExistence(timeout: 5))
        XCTAssertEqual(row.firstMatch.label, "echo seyal-history-recall")

        query.firstMatch.typeText("zzz-no-match")
        let noRows = expectation(for: NSPredicate(format: "value == '0'"), evaluatedWith: overlay.firstMatch, handler: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [noRows], timeout: 5), .completed, "type-to-filter runs in Rust")
        app.typeKey(.escape, modifierFlags: [])
        let dismissed = expectation(for: NSPredicate(format: "isHittable == false"), evaluatedWith: overlay.firstMatch, handler: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [dismissed], timeout: 5), .completed, "Escape closes the overlay")

        toggle.firstMatch.click()
        XCTAssertTrue(overlay.firstMatch.waitForExistence(timeout: 5))
        query.firstMatch.typeText("recall")
        app.typeKey("\r", modifierFlags: [])
        let inserted = expectation(
            for: NSPredicate(format: "value == 'echo seyal-history-recall'"),
            evaluatedWith: editor.firstMatch,
            handler: nil
        )
        XCTAssertEqual(XCTWaiter.wait(for: [inserted], timeout: 5), .completed, "Enter inserts into the draft")
        XCTAssertFalse(overlay.firstMatch.isHittable)
        XCTAssertEqual(app.state, .runningForeground)
    }

    func testSelectingABlockRevealsRustBlockDetailsInInspector() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        let composer = app.descendants(matching: .any)["seyal-composer"]
        XCTAssertTrue(composer.waitForExistence(timeout: 12))
        let composerReady = NSPredicate(format: "value == 'available'")
        let becameReady = expectation(for: composerReady, evaluatedWith: composer, handler: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [becameReady], timeout: 12), .completed)
        // The inspector is visible by default (#922); "no Block selected" is
        // proven by the absence of Block-details rows, not by hittability.
        let inspector = app.descendants(matching: .any)["seyal-inspector"]
        let commandRow = inspector.descendants(matching: .staticText)["Block · Command"]
        XCTAssertFalse(commandRow.exists, "no Block details before any selection")
        // Hide the inspector through Rust first so "selecting a Block reveals
        // the inspector" below observes a real hidden -> visible transition.
        hideInspectorThroughPalette(in: app, inspector: inspector)
        let editor = app.descendants(matching: .any)["seyal-composer-editor"]
        XCTAssertTrue(editor.waitForExistence(timeout: 5))
        editor.firstMatch.click()
        let submitted = "echo seyal-block-details-\(UUID().uuidString)"
        editor.firstMatch.typeText(submitted)
        editor.firstMatch.typeKey("\r", modifierFlags: [])
        let cleared = expectation(
            for: NSPredicate(format: "value == nil OR value == ''"),
            evaluatedWith: editor.firstMatch,
            handler: nil
        )
        XCTAssertEqual(XCTWaiter.wait(for: [cleared], timeout: 8), .completed)

        // Blocks are Runtime timeline metadata (OSC 133); the card appears when
        // Runtime publishes the revision. Without shell integration there is no
        // Block and therefore nothing to select: the test proves the host never
        // fabricates one.
        //
        // Do not click `seyal-block-0`: hostedApp reconnects to the exclusive
        // Runtime, so index 0 is often an earlier tall card from this suite.
        let card = app.descendants(matching: .any).matching(
            NSPredicate(
                format: "identifier MATCHES %@ AND label == %@",
                "seyal-block-[0-9]+",
                submitted
            )
        ).firstMatch
        guard card.waitForExistence(timeout: 10) else {
            XCTAssertFalse(commandRow.exists, "no Block, no Block details")
            return
        }
        // Async history-range replies grow earlier cards after the initial
        // live-end scroll. Wait until the submitted card is hittable so the
        // click lands on the Block chrome rather than an off-clip ghost.
        let hittable = expectation(
            for: NSPredicate(format: "isHittable == true"),
            evaluatedWith: card,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [hittable], timeout: 8),
            .completed,
            "submitted Block must remain in the visible transcript clip"
        )
        card.click()
        XCTAssertTrue(inspector.waitForExistence(timeout: 5))
        let revealed = expectation(for: NSPredicate(format: "isHittable == true"), evaluatedWith: inspector.firstMatch, handler: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [revealed], timeout: 5), .completed, "selecting a Block reveals the inspector")
        let selected = expectation(for: NSPredicate(format: "value == 'selected'"), evaluatedWith: card, handler: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [selected], timeout: 5), .completed, "card reflects the Rust selected flag")
        XCTAssertTrue(commandRow.waitForExistence(timeout: 5), "inspector shows Rust Block rows")
        XCTAssertTrue(inspector.descendants(matching: .staticText)[submitted].waitForExistence(timeout: 5))
        XCTAssertFalse(inspector.descendants(matching: .staticText)["Block · Duration"].exists, "no fabricated telemetry")

        card.click()
        let deselected = expectation(for: NSPredicate(format: "value == nil OR value == ''"), evaluatedWith: card, handler: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [deselected], timeout: 5), .completed, "clicking again clears the selection")
        XCTAssertFalse(commandRow.exists, "Block rows leave with the selection")
        XCTAssertEqual(app.state, .runningForeground)
    }

    private func hideInspectorThroughPalette(in app: XCUIApplication, inspector: XCUIElement) {
        XCTAssertTrue(inspector.firstMatch.isHittable, "Core Terminal inspector is visible by default (#922)")
        let palette = app.descendants(matching: .any)["seyal-command-palette"]
        app.typeKey("k", modifierFlags: .command)
        XCTAssertTrue(palette.waitForExistence(timeout: 5), "⌘K opens the Rust-backed palette")
        let query = app.descendants(matching: .any)["seyal-command-palette-query"]
        XCTAssertTrue(query.waitForExistence(timeout: 5))
        query.firstMatch.click()
        query.firstMatch.typeText("Hide Inspector")
        let row = app.descendants(matching: .any)["seyal-command-palette-row-0"]
        XCTAssertTrue(row.waitForExistence(timeout: 5))
        XCTAssertEqual(row.label, "Hide Inspector")
        app.typeKey("\r", modifierFlags: [])
        let hidden = expectation(
            for: NSPredicate(format: "isHittable == false"),
            evaluatedWith: inspector.firstMatch,
            handler: nil
        )
        XCTAssertEqual(XCTWaiter.wait(for: [hidden], timeout: 5), .completed, "Rust hid the inspector before selection")
    }

    private func waitForUsablePty(in app: XCUIApplication, timeout: TimeInterval = 20) {
        XCTAssertTrue(app.wait(for: .runningForeground, timeout: 10))
        let terminal = app.descendants(matching: .any)["terminal-input"]
        XCTAssertTrue(terminal.waitForExistence(timeout: 10), "terminal surface missing")
        let usable = NSPredicate(format: "value CONTAINS 'connection=usable'")
        let arrived = expectation(for: usable, evaluatedWith: terminal, handler: nil)
        let result = XCTWaiter.wait(for: [arrived], timeout: timeout)
        XCTAssertEqual(
            app.state,
            .runningForeground,
            "Seyal.app crashed while waiting for the PTY/runtime attach"
        )
        XCTAssertEqual(
            result,
            .completed,
            "PTY/runtime never became usable; terminal AX=\(terminal.value ?? "nil")"
        )
        assertFlowBlocksOrFail(in: app)
    }

    /// Headed XCUI must stay on Flow/Blocks. A leftover Runtime in alternate
    /// screen, or a Raw-only launch, looks like a normal terminal and is a fail.
    private func assertFlowBlocksOrFail(in app: XCUIApplication, timeout: TimeInterval = 8) {
        let composer = app.descendants(matching: .any)["seyal-composer"]
        let blocks = app.descendants(matching: .any)["seyal-blocks"]
        let transcript = app.descendants(matching: .any)["seyal-blocks-scroll"]
        XCTAssertTrue(
            composer.waitForExistence(timeout: timeout),
            "XCUI must run as Flow/Blocks; composer missing — UI looks like a normal terminal"
        )
        XCTAssertTrue(
            composer.firstMatch.isHittable,
            "XCUI must run as Flow/Blocks; composer not hittable — UI looks like a normal terminal"
        )
        XCTAssertTrue(
            blocks.waitForExistence(timeout: 5),
            "XCUI must run as Flow/Blocks; seyal-blocks missing — UI looks like a normal terminal"
        )
        XCTAssertTrue(
            transcript.waitForExistence(timeout: 5),
            "XCUI must run as Flow/Blocks; transcript missing — UI looks like a normal terminal"
        )
        XCTAssertGreaterThan(
            transcript.firstMatch.frame.height,
            120,
            "Flow transcript must fill the Pane; a short or hidden transcript is a raw-terminal launch"
        )
        XCTAssertTrue(
            app.descendants(matching: .any)["seyal-left-workspaces"].firstMatch.isHittable,
            "Core Terminal left panel stays visible alongside Flow composer/Blocks (#922)"
        )
    }

    /// Headed #823 smoke: composer submit plus Cmd-C / ArrowUp must stay on
    /// Flow/Blocks. This is not a TerminalKeyV2 byte oracle; headed key-to-PTY
    /// and the six-step IME/Neovim/TUI matrix remain open.
    func testComposerSubmitAndHostShortcutsStayOnFlowBlocks() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        assertFlowBlocksOrFail(in: app)

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
            editor.firstMatch.typeText("echo seyal-823-flow-blocks")
            editor.firstMatch.typeKey("\r", modifierFlags: [])
        } else {
            composer.firstMatch.typeText("echo seyal-823-flow-blocks")
            app.typeKey("\r", modifierFlags: [])
        }
        XCTAssertEqual(app.state, .runningForeground, "Seyal.app crashed on composer submit")
        assertFlowBlocksOrFail(in: app)

        app.typeKey("c", modifierFlags: .command)
        waitBriefly(0.3)
        app.typeKey(.upArrow, modifierFlags: [])
        XCTAssertEqual(app.state, .runningForeground, "Seyal.app crashed on Cmd-C / ArrowUp")
        assertFlowBlocksOrFail(in: app)
        XCTAssertTrue(
            composer.firstMatch.isHittable,
            "Cmd-C / ArrowUp must not leave Flow/Blocks for a raw terminal"
        )
    }

    func testSystemABCDeadKeyCommitAndCancelReachRealPty() throws {
        let sources = UserDefaults(suiteName: "com.apple.HIToolbox")?
            .array(forKey: "AppleSelectedInputSources") as? [[String: Any]]
        guard sources?.contains(where: { $0["KeyboardLayout Name"] as? String == "ABC" }) == true else {
            throw XCTSkip("This system-input-source case requires the macOS ABC keyboard layout.")
        }
        let app = hostedApp()
        waitForUsablePty(in: app)
        assertFlowBlocksOrFail(in: app)
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("seyal-ime-system-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
        defer {
            do {
                try FileManager.default.removeItem(at: directory)
            } catch {
                XCTFail("Could not remove system IME capture: \(error)")
            }
        }
        let capture = directory.appendingPathComponent("committed.bin")
        let script = """
        import os, select, sys, termios, time, tty
        fd = sys.stdin.fileno()
        saved = termios.tcgetattr(fd)
        data = bytearray()
        try:
            tty.setraw(fd)
            os.write(1, b"\\x1b[?1049h\\x1b[3;5HIME")
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline:
                if not select.select([fd], [], [], max(0, deadline - time.monotonic()))[0]:
                    break
                byte = os.read(fd, 1)
                if not byte or byte == b"\\x04":
                    break
                data.extend(byte)
        finally:
            try:
                with open(\(String(reflecting: capture.path)), "wb") as output:
                    output.write(data)
            finally:
                termios.tcsetattr(fd, termios.TCSANOW, saved)
                os.write(1, b"\\x1b[?1049l")
        """
        let scriptURL = directory.appendingPathComponent("receive.py")
        try script.write(to: scriptURL, atomically: true, encoding: .utf8)
        submitComposerCommand(app, "/usr/bin/python3 '\(scriptURL.path)'")
        let composer = app.descendants(matching: .any)["seyal-composer"].firstMatch
        let enteredTui = expectation(
            for: NSPredicate(format: "isHittable == false"), evaluatedWith: composer, handler: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [enteredTui], timeout: 12), .completed)
        let terminal = app.descendants(matching: .any)["terminal-input"].firstMatch
        XCTAssertTrue(terminal.waitForExistence(timeout: 5))
        terminal.click()
        defer {
            if app.state == .runningForeground, !composer.isHittable {
                app.typeKey("d", modifierFlags: .control)
            }
        }
        app.typeKey("e", modifierFlags: .option)
        app.typeKey("e", modifierFlags: [])
        app.typeKey("e", modifierFlags: .option)
        app.typeKey(.escape, modifierFlags: [])
        app.typeKey("x", modifierFlags: [])
        app.typeKey(.escape, modifierFlags: [])
        app.typeKey("d", modifierFlags: .control)
        let returned = expectation(
            for: NSPredicate(format: "isHittable == true"), evaluatedWith: composer, handler: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [returned], timeout: 12), .completed)
        let captured = try Data(contentsOf: capture)
        XCTAssertEqual(
            captured, Data("éx\u{1b}".utf8),
            "Unexpected PTY bytes: \(captured.map { String(format: "%02x", $0) }.joined(separator: " "))")
        assertFlowBlocksOrFail(in: app)
    }

    /// Flow eligibility routes keys to the composer, not the PTY. A headed
    /// byte oracle that enters Raw/TUI looks like a normal terminal and is
    /// withdrawn. This case proves ArrowUp / Cmd-C on Flow do not write PTY
    /// bytes, while composer + Blocks stay visible. Runtime IPC remains the
    /// encoder→PTY proof; six-step IME/Neovim/TUI stays manual.
    func testFlowBlocksDoesNotForwardArrowUpIntoWaitingPty() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        assertFlowBlocksOrFail(in: app)

        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("seyal-flow-input-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
        let capture = directory.appendingPathComponent("captured.bin")
        let active = directory.appendingPathComponent("active")
        let ready = directory.appendingPathComponent("ready")
        let done = directory.appendingPathComponent("done")
        try Data().write(to: active)
        // A waiting reader must not consume the next test's shell command.
        defer {
            do {
                try FileManager.default.removeItem(at: active)
                let stopped = expectation(
                    for: NSPredicate { _, _ in
                        FileManager.default.fileExists(atPath: done.path)
                    }, evaluatedWith: nil, handler: nil)
                XCTAssertEqual(XCTWaiter.wait(for: [stopped], timeout: 5), .completed)
                try FileManager.default.removeItem(at: directory)
            } catch {
                XCTFail("Could not clean up Flow input capture: \(error)")
            }
        }
        let script = """
        import os, select, sys, termios, time, tty
        fd = sys.stdin.fileno()
        saved = termios.tcgetattr(fd)
        try:
            tty.setraw(fd)
            with open(\(String(reflecting: capture.path)), "wb", buffering=0) as output:
                open(\(String(reflecting: ready.path)), "wb").close()
                deadline = time.monotonic() + 30
                while os.path.exists(\(String(reflecting: active.path))) and time.monotonic() < deadline:
                    if select.select([fd], [], [], 0.1)[0]:
                        data = os.read(fd, 4096)
                        if not data:
                            break
                        output.write(data)
        finally:
            termios.tcsetattr(fd, termios.TCSANOW, saved)
            open(\(String(reflecting: done.path)), "wb").close()
        """
        let scriptURL = directory.appendingPathComponent("receive.py")
        try script.write(to: scriptURL, atomically: true, encoding: .utf8)
        submitComposerCommand(app, "/usr/bin/python3 '\(scriptURL.path)'")
        let receiving = expectation(
            for: NSPredicate { _, _ in
                FileManager.default.fileExists(atPath: ready.path)
            }, evaluatedWith: nil, handler: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [receiving], timeout: 10), .completed)
        XCTAssertEqual(app.state, .runningForeground)
        assertFlowBlocksOrFail(in: app)

        let terminal = app.descendants(matching: .any)["terminal-input"]
        if terminal.waitForExistence(timeout: 2), terminal.firstMatch.isHittable {
            terminal.firstMatch.click()
        }
        app.typeKey("c", modifierFlags: .command)
        waitBriefly(0.3)
        app.typeKey(.upArrow, modifierFlags: [])
        waitBriefly(0.8)
        XCTAssertEqual(app.state, .runningForeground, "Seyal.app crashed while Flow rejected PTY keys")
        assertFlowBlocksOrFail(in: app)

        let data = try Data(contentsOf: capture)
        XCTAssertTrue(
            data.isEmpty,
            "Flow must not forward ArrowUp/Cmd-C into a waiting PTY; captured \(data.count) bytes"
        )
    }

    func testCopyPasteAndQuitMenusAreWired() throws {
        let app = hostedApp()
        XCTAssertTrue(app.wait(for: .runningForeground, timeout: 10))
        waitForUsablePty(in: app)
        app.typeKey("c", modifierFlags: .command)
        app.typeKey("v", modifierFlags: .command)
        XCTAssertEqual(app.state, .runningForeground, "Seyal.app crashed on Cmd-C / Cmd-V")
        assertFlowBlocksOrFail(in: app)
        app.typeKey("q", modifierFlags: .command)
        XCTAssertTrue(app.wait(for: .notRunning, timeout: 8))
    }

    func testHostMouseClickStaysOnFlowBlocks() throws {
        let app = hostedApp()
        XCTAssertTrue(app.wait(for: .runningForeground, timeout: 10))
        waitForUsablePty(in: app)
        let surface = app.descendants(matching: .any)["terminal-input"]
        XCTAssertTrue(surface.waitForExistence(timeout: 5))
        surface.firstMatch.click()
        XCTAssertEqual(app.state, .runningForeground, "Seyal.app crashed on host mouse click")
        assertFlowBlocksOrFail(in: app)
    }

    func testCommandPaletteOpensFiltersRunsAndDismisses() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        let inspector = app.descendants(matching: .any)["seyal-inspector"]
        XCTAssertTrue(inspector.firstMatch.isHittable, "Core Terminal inspector is visible by default (#922)")

        let palette = app.descendants(matching: .any)["seyal-command-palette"]
        app.typeKey("k", modifierFlags: .command)
        XCTAssertTrue(palette.waitForExistence(timeout: 5), "⌘K opens the Rust-backed palette")
        XCTAssertTrue(palette.firstMatch.isHittable)

        let query = app.descendants(matching: .any)["seyal-command-palette-query"]
        XCTAssertTrue(query.waitForExistence(timeout: 5))
        query.firstMatch.click()
        // The available toggle command is "Hide Inspector" since the
        // inspector is already visible by default.
        query.firstMatch.typeText("Hide Inspector")
        let row = app.descendants(matching: .any)["seyal-command-palette-row-0"]
        let filtered = expectation(
            for: NSPredicate(format: "exists == true"),
            evaluatedWith: row,
            handler: nil
        )
        XCTAssertEqual(XCTWaiter.wait(for: [filtered], timeout: 5), .completed, "type-to-filter runs in Rust")
        XCTAssertEqual(row.label, "Hide Inspector")

        app.typeKey("\r", modifierFlags: [])
        let dismissed = expectation(
            for: NSPredicate(format: "isHittable == false"),
            evaluatedWith: palette.firstMatch,
            handler: nil
        )
        XCTAssertEqual(XCTWaiter.wait(for: [dismissed], timeout: 5), .completed, "Enter runs the row and closes")
        let hidden = expectation(
            for: NSPredicate(format: "isHittable == false"),
            evaluatedWith: inspector.firstMatch,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [hidden], timeout: 5),
            .completed,
            "the resolved command actually ran, not just an overlay animation"
        )
        XCTAssertEqual(app.state, .runningForeground)
    }

    func testCommandPaletteEscapeAndClickOutsideBothDismissWithoutRunning() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        let palette = app.descendants(matching: .any)["seyal-command-palette"]
        let scrim = app.descendants(matching: .any)["seyal-command-palette-scrim"]

        app.typeKey("k", modifierFlags: .command)
        XCTAssertTrue(palette.waitForExistence(timeout: 5))
        app.typeKey(.escape, modifierFlags: [])
        let closedByEscape = expectation(
            for: NSPredicate(format: "isHittable == false"),
            evaluatedWith: palette.firstMatch,
            handler: nil
        )
        XCTAssertEqual(XCTWaiter.wait(for: [closedByEscape], timeout: 5), .completed)

        app.typeKey("k", modifierFlags: .command)
        XCTAssertTrue(palette.waitForExistence(timeout: 5))
        // Click near the top-left corner of the scrim, well outside the
        // centered card, to exercise click-outside-to-close.
        scrim.firstMatch.coordinate(withNormalizedOffset: CGVector(dx: 0.02, dy: 0.02)).click()
        let closedByClick = expectation(
            for: NSPredicate(format: "isHittable == false"),
            evaluatedWith: palette.firstMatch,
            handler: nil
        )
        XCTAssertEqual(XCTWaiter.wait(for: [closedByClick], timeout: 5), .completed)
        XCTAssertEqual(app.state, .runningForeground)
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

    private func waitBriefly(_ seconds: TimeInterval) {
        RunLoop.current.run(until: Date().addingTimeInterval(seconds))
    }

    /// The same predicate as `ShellIntegrationPolicy::supports` applied to the
    /// shell the headed host will spawn (`pw_shell`, never inherited `SHELL`).
    private func loginShellIsZsh() -> Bool {
        guard let account = getpwuid(geteuid()), let shell = account.pointee.pw_shell else {
            return false
        }
        return URL(fileURLWithPath: String(cString: shell)).lastPathComponent == "zsh"
    }
}
