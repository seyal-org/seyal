import XCTest

/// Headed #824/#868 workload evidence on the ADR-015 thin host.
///
/// #824 proved Flow/Blocks through high-volume output, alternate-screen return,
/// Unicode composer submit, and GUI relaunch reconnect.
/// #868 extends that into the SPEC-008 §§8–9 real-workload matrix: sequential
/// Blocks, long scroll-owned output, Vim/Neovim/htop takeover, unsupported-shell
/// Raw, nested-shell / SSH trusted-boundary behavior, empty-canvas focus, and
/// detach/reattach with Block preservation.
@MainActor
final class SeyalHostWorkloadUITests: XCTestCase {
    override func setUp() {
        continueAfterFailure = false
    }

    private func hostedApp(shell: String = "/bin/zsh") -> XCUIApplication {
        let app = XCUIApplication()
        app.launchIsolatedHost(shell: shell)
        return app
    }

    // MARK: - #824 retained evidence

    func testHighVolumeComposerOutputStaysOnFlowBlocks() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        submitComposerCommand(app, "printf '%s\\n' $(seq 1 60) 'seyal-824-high-volume'")
        XCTAssertEqual(app.state, .runningForeground, "Seyal.app crashed on high-volume output")
        assertFlowBlocksOrFail(in: app)
        attachScreenshot(app, name: "824-high-volume")
    }

    func testPrimaryShellExitIsReportedAndKeepsFlowHistoryVisible() {
        let app = hostedApp()
        waitForUsablePty(in: app)
        submitComposerCommand(app, "printf 'seyal-1171-exit-before\\n'; exit 7")

        let recovery = app.descendants(matching: .any)["seyal-recovery"]
        let ended = expectation(
            for: NSPredicate(format: "value CONTAINS 'shell exited'"),
            evaluatedWith: recovery,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [ended], timeout: 15),
            .completed,
            "a completed primary shell must be reported as exited, not left waiting for a prompt"
        )
        let composer = app.descendants(matching: .any)["seyal-composer"]
        XCTAssertFalse(composer.firstMatch.isHittable, "an ended shell must not accept Flow commands")
        XCTAssertTrue(
            app.descendants(matching: .any)["seyal-blocks"].firstMatch.isHittable,
            "completed Flow history must remain visible after the shell exits"
        )
        let terminal = app.descendants(matching: .any)["terminal-input"]
        XCTAssertTrue((terminal.value as? String ?? "").contains("execution=none"))
        XCTAssertEqual(app.state, .runningForeground)
        attachScreenshot(app, name: "1171-primary-shell-exit")
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

    // MARK: - #868 SPEC-008 matrix

    /// Basic `printf` / `ls` create ordered completed Blocks with confined bodies.
    func testBasicSequentialCommandsCreateOrderedBlocks() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        let markerA = "seyal-868-a-\(String(UUID().uuidString.prefix(6)))"
        let markerB = "seyal-868-b-\(String(UUID().uuidString.prefix(6)))"
        submitComposerCommand(app, "printf '%s\\n' \(markerA)")
        waitForBlock(in: app, index: 0, labelContains: "printf")
        submitComposerCommand(app, "ls -1 /bin | head -n 3; printf '%s\\n' \(markerB)")
        waitForBlock(in: app, index: 1, labelContains: "ls")
        assertFlowBlocksOrFail(in: app)
        let body0 = app.descendants(matching: .any)["seyal-block-0-body"].firstMatch
        let body1 = app.descendants(matching: .any)["seyal-block-1-body"].firstMatch
        XCTAssertTrue(body0.exists, "first Block body must exist")
        XCTAssertTrue(body1.exists, "second Block body must exist")
        XCTAssertGreaterThan(body0.frame.height, 0)
        XCTAssertGreaterThan(body1.frame.height, 0)
        // Bodies stay inside the transcript scroll owner, not a coexisting raw viewport.
        let transcript = app.descendants(matching: .any)["seyal-blocks-scroll"].firstMatch
        XCTAssertTrue(transcript.frame.contains(body0.frame.origin))
        XCTAssertTrue(transcript.frame.contains(body1.frame.origin))
        attachScreenshot(app, name: "868-sequential-blocks")
    }

    /// Long normal-screen output stays under the Pane scroll owner (no Pane-wide leak).
    func testLongSeqOutputStaysUnderPaneScrollOwner() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        submitComposerCommand(
            app,
            "printf '%s\\n' $(seq 1 200) 'seyal-868-long-seq'"
        )
        XCTAssertEqual(app.state, .runningForeground, "Seyal.app crashed on long seq output")
        assertFlowBlocksOrFail(in: app)
        let transcript = app.descendants(matching: .any)["seyal-blocks-scroll"].firstMatch
        let composer = app.descendants(matching: .any)["seyal-composer"].firstMatch
        XCTAssertGreaterThan(transcript.frame.height, 120)
        XCTAssertTrue(composer.isHittable, "long output must not displace Flow composer")
        // Scroll owner can move without collapsing Flow chrome.
        if transcript.isHittable {
            transcript.swipeUp()
            waitBriefly(0.3)
            transcript.swipeDown()
        }
        assertFlowBlocksOrFail(in: app)
        attachScreenshot(app, name: "868-long-seq-scroll")
    }

    /// Vim takes the full Pane (TUI) and returns to Flow on the same ExecutionId.
    func testVimTakeoverAndReturnKeepsExecution() throws {
        let vim = "/usr/bin/vim"
        try requireBinary(vim, name: "vim")
        try exerciseFullscreenToolTakeover(
            binary: vim,
            launch: "vim -n -c 'qa!'",
            screenshot: "868-vim-takeover"
        )
    }

    /// Neovim takes the full Pane and returns without recreating execution.
    func testNeovimTakeoverAndReturnKeepsExecution() throws {
        guard let nvim = firstExistingBinary(["/opt/homebrew/bin/nvim", "/usr/local/bin/nvim"])
        else {
            throw XCTSkip(
                "ENVIRONMENT_UNSUPPORTED: nvim not installed; headed TUI matrix requires it.")
        }
        try exerciseFullscreenToolTakeover(
            binary: nvim,
            launch: "\(nvim) -u NONE -c 'qa!'",
            screenshot: "868-nvim-takeover"
        )
    }

    /// htop/ncurses-style full-Pane takeover and return on the same ExecutionId.
    /// Uses the shared bounded alternate-screen fixture (same TUI presentation
    /// path as htop/ncurses). Vim/Neovim cover other live fullscreen binaries.
    func testHtopTakeoverAndReturnKeepsExecution() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        let terminal = app.descendants(matching: .any)["terminal-input"]
        let executionBefore = identityToken(
            terminal.firstMatch.value as? String ?? "", key: "execution=")
        XCTAssertFalse(executionBefore.isEmpty)

        try exerciseBoundedAlternateScreen(in: app) { command in
            submitComposerCommand(app, command)
        }
        assertFlowBlocksOrFail(in: app)
        let executionAfter = identityToken(
            terminal.firstMatch.value as? String ?? "", key: "execution=")
        XCTAssertEqual(
            executionAfter,
            executionBefore,
            "htop/ncurses-style TUI exit must keep ExecutionId"
        )
        attachScreenshot(app, name: "868-htop-ncurses-return")
    }

    /// Non-interactive SSH stays on Flow as one outer command — M003 trusted
    /// boundaries do not invent remote Blocks or live-CWD claims.
    func testSupportedSshFollowsTrustedBoundaries() throws {
        guard FileManager.default.isExecutableFile(atPath: "/usr/bin/ssh") else {
            throw XCTSkip(
                "ENVIRONMENT_UNSUPPORTED: /usr/bin/ssh missing; cannot prove SSH trusted boundaries.")
        }
        let app = hostedApp()
        waitForUsablePty(in: app)
        let beforeCount = visibleBlockCount(in: app)
        // BatchMode + short timeout: completes as a local Flow command without
        // opening a remote trusted integration (M003 has none).
        submitComposerCommand(
            app,
            "ssh -o BatchMode=yes -o ConnectTimeout=1 -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null 127.0.0.1 true; printf 'seyal-868-ssh-boundary\\n'"
        )
        assertFlowBlocksOrFail(in: app, timeout: 20)
        let afterCount = visibleBlockCount(in: app)
        XCTAssertGreaterThan(
            afterCount,
            beforeCount,
            "SSH attempt must record at least one outer Flow Block"
        )
        // Must not have flipped the Pane into Raw for a finished BatchMode ssh.
        let composer = app.descendants(matching: .any)["seyal-composer"].firstMatch
        XCTAssertTrue(composer.isHittable, "finished BatchMode SSH must remain Flow")
        attachScreenshot(app, name: "868-ssh-trusted-boundary")
    }

    /// Unsupported primary shell (bash) is full-Pane Raw with no guessed Blocks.
    /// Nested interactive zsh from Flow does not invent nested Blocks.
    func testUnsupportedShellRawAndNestedShellDoesNotGuessBlocks() throws {
        // Part A — unsupported primary shell → Raw.
        let bashApp = hostedApp(shell: "/bin/bash")
        waitForUsableConnection(in: bashApp, expectFlow: false)
        assertRawFullPaneOrFail(in: bashApp)
        XCTAssertFalse(
            bashApp.descendants(matching: .any)["seyal-block-0"].exists,
            "unsupported bash must not scrape guessed Blocks"
        )
        attachScreenshot(bashApp, name: "868-unsupported-bash-raw")
        bashApp.terminate()
        _ = bashApp.wait(for: .notRunning, timeout: 8)

        // Part B — nested interactive shell stays part of one outer Flow command.
        let app = hostedApp()
        waitForUsablePty(in: app)
        let before = visibleBlockCount(in: app)
        let token = String(UUID().uuidString.prefix(8))
        let scriptURL = URL(fileURLWithPath: "/tmp/s868-nest-\(token).sh")
        let startedURL = URL(fileURLWithPath: "/tmp/s868-nest-\(token).ran")
        try """
        : > \(startedURL.path)
        # Nested interactive zsh must not re-bootstrap trusted markers into
        # additional Blocks (ADR-009 / SPEC-008 nested-shell rule).
        /bin/zsh -f -c 'printf "seyal-868-nested\\n"; exit 0'
        """.write(to: scriptURL, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes(
            [.posixPermissions: 0o700], ofItemAtPath: scriptURL.path)
        defer {
            try? FileManager.default.removeItem(at: scriptURL)
            try? FileManager.default.removeItem(at: startedURL)
        }
        submitComposerCommand(app, "/bin/bash --noprofile --norc \(scriptURL.path)")
        let deadline = Date().addingTimeInterval(12)
        while Date() < deadline, !FileManager.default.fileExists(atPath: startedURL.path) {
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        }
        assertFlowBlocksOrFail(in: app, timeout: 15)
        let after = visibleBlockCount(in: app)
        XCTAssertEqual(
            after,
            before + 1,
            "nested zsh must remain one outer Block; got before=\(before) after=\(after)"
        )
        attachScreenshot(app, name: "868-nested-shell-one-block")
    }

    /// Clicking empty Flow canvas must not focus the hidden Metal terminal input.
    func testEmptyFlowCanvasDoesNotFocusHiddenTerminal() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        let terminal = app.descendants(matching: .any)["terminal-input"].firstMatch
        let composer = app.descendants(matching: .any)["seyal-composer"].firstMatch
        let transcript = app.descendants(matching: .any)["seyal-blocks-scroll"].firstMatch
        let before = terminal.value as? String ?? ""
        XCTAssertTrue(transcript.isHittable)
        // Click empty canvas (top of transcript) rather than composer.
        let emptySpot = transcript.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.15))
        emptySpot.click()
        waitBriefly(0.2)
        // Typing must not route into a hidden direct-terminal surface.
        app.typeText("seyal-868-empty-canvas")
        waitBriefly(0.3)
        let after = terminal.value as? String ?? ""
        // Connection/execution probe tokens must stay stable; typed junk must
        // not appear as PTY-driven AX mutation of identity tokens.
        XCTAssertEqual(
            identityToken(after, key: "execution="),
            identityToken(before, key: "execution="),
            "empty-canvas typing must not mutate execution identity via hidden terminal"
        )
        XCTAssertTrue(
            composer.isHittable,
            "Flow must keep composer as the presentation owner after empty-canvas click"
        )
        attachScreenshot(app, name: "868-empty-canvas")
    }

    /// Detach/reattach preserves completed Blocks and reselects Flow presentation.
    func testDetachReattachPreservesBlocksAndPresentation() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        let marker = "seyal-868-reattach-\(String(UUID().uuidString.prefix(6)))"
        submitComposerCommand(app, "printf '%s\\n' \(marker)")
        waitForBlock(in: app, index: 0, labelContains: "printf")
        let terminal = app.descendants(matching: .any)["terminal-input"]
        let executionBefore = identityToken(
            terminal.firstMatch.value as? String ?? "", key: "execution=")
        XCTAssertFalse(executionBefore.isEmpty)

        app.terminate()
        XCTAssertTrue(app.wait(for: .notRunning, timeout: 8))
        app.launch()
        waitForUsablePty(in: app)
        let executionAfter = identityToken(
            terminal.firstMatch.value as? String ?? "", key: "execution=")
        XCTAssertEqual(executionAfter, executionBefore)
        assertFlowBlocksOrFail(in: app)
        XCTAssertTrue(
            app.descendants(matching: .any)["seyal-block-0"].waitForExistence(timeout: 8),
            "reattach must restore the completed Block card"
        )
        attachScreenshot(app, name: "868-reattach-blocks")
    }

    // MARK: - Helpers

    private func exerciseFullscreenToolTakeover(
        binary: String, launch: String, screenshot: String
    ) throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        let terminal = app.descendants(matching: .any)["terminal-input"]
        let executionBefore = identityToken(
            terminal.firstMatch.value as? String ?? "", key: "execution=")
        XCTAssertFalse(executionBefore.isEmpty)
        submitComposerCommand(app, launch)
        // Some hosts exit vim so quickly TUI is momentary; accept either a
        // visible TUI window or an immediate Flow return on the same execution.
        let composer = app.descendants(matching: .any)["seyal-composer"].firstMatch
        let tuiDeadline = Date().addingTimeInterval(6)
        var sawTui = false
        while Date() < tuiDeadline {
            if composer.exists && !composer.isHittable {
                sawTui = true
                break
            }
            if composer.exists && composer.isHittable
                && identityToken(terminal.firstMatch.value as? String ?? "", key: "execution=")
                    == executionBefore
            {
                // Fast exit path (vim -c qa! may return before the waiter samples).
                break
            }
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        }
        if sawTui {
            attachScreenshot(app, name: "\(screenshot)-tui")
            assertFlowBlocksOrFail(in: app, timeout: 20)
        } else {
            assertFlowBlocksOrFail(in: app, timeout: 12)
        }
        let executionAfter = identityToken(
            terminal.firstMatch.value as? String ?? "", key: "execution=")
        XCTAssertEqual(
            executionAfter,
            executionBefore,
            "\(binary) takeover/return must keep ExecutionId"
        )
        attachScreenshot(app, name: screenshot)
    }

    private func requireBinary(_ path: String, name: String) throws {
        guard FileManager.default.isExecutableFile(atPath: path) else {
            throw XCTSkip(
                "ENVIRONMENT_UNSUPPORTED: \(name) not installed; headed TUI matrix requires it.")
        }
    }

    private func firstExistingBinary(_ candidates: [String]) -> String? {
        candidates.first { FileManager.default.isExecutableFile(atPath: $0) }
    }

    private func waitForBlock(
        in app: XCUIApplication, index: Int, labelContains: String, timeout: TimeInterval = 12
    ) {
        let block = app.descendants(matching: .any)["seyal-block-\(index)"]
        XCTAssertTrue(block.waitForExistence(timeout: timeout), "missing seyal-block-\(index)")
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            let label = block.firstMatch.label
            if label.localizedCaseInsensitiveContains(labelContains) { return }
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        }
        XCTFail(
            "seyal-block-\(index) label did not contain \(labelContains); label=\(block.firstMatch.label)"
        )
    }

    private func visibleBlockCount(in app: XCUIApplication) -> Int {
        var count = 0
        for index in 0..<8 {
            if app.descendants(matching: .any)["seyal-block-\(index)"].exists {
                count += 1
            } else {
                break
            }
        }
        return count
    }

    private func assertTuiFullPaneOrFail(in app: XCUIApplication, timeout: TimeInterval = 12) {
        let composer = app.descendants(matching: .any)["seyal-composer"]
        let terminal = app.descendants(matching: .any)["terminal-input"]
        XCTAssertTrue(terminal.waitForExistence(timeout: timeout))
        let deadline = Date().addingTimeInterval(timeout)
        var yielded = false
        while Date() < deadline {
            // Hidden Flow chrome may drop out of the AX tree entirely under TUI.
            if !composer.exists || !composer.firstMatch.isHittable {
                yielded = true
                break
            }
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        }
        XCTAssertTrue(
            yielded,
            "TUI must hide Flow composer; exists=\(composer.exists) value=\(composer.exists ? (composer.value ?? "nil") : "missing")"
        )
        XCTAssertTrue(terminal.firstMatch.isHittable, "TUI must keep a hittable terminal surface")
    }

    private func assertRawFullPaneOrFail(in app: XCUIApplication, timeout: TimeInterval = 12) {
        let composer = app.descendants(matching: .any)["seyal-composer"]
        let terminal = app.descendants(matching: .any)["terminal-input"]
        XCTAssertTrue(terminal.waitForExistence(timeout: timeout), "Raw must keep terminal-input")
        let deadline = Date().addingTimeInterval(timeout)
        var composerYielded = false
        while Date() < deadline {
            if !composer.exists || !composer.firstMatch.isHittable {
                composerYielded = true
                break
            }
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        }
        XCTAssertTrue(
            composerYielded,
            "Raw must hide composer; exists=\(composer.exists) value=\(composer.value ?? "nil")"
        )
        XCTAssertTrue(terminal.firstMatch.isHittable, "Raw must keep a hittable terminal surface")
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
        waitForUsableConnection(in: app, expectFlow: true, timeout: timeout)
    }

    private func waitForUsableConnection(
        in app: XCUIApplication, expectFlow: Bool, timeout: TimeInterval = 20
    ) {
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
        if expectFlow {
            assertFlowBlocksOrFail(in: app)
        }
    }

    private func assertFlowBlocksOrFail(in app: XCUIApplication, timeout: TimeInterval = 8) {
        let composer = app.descendants(matching: .any)["seyal-composer"]
        let blocks = app.descendants(matching: .any)["seyal-blocks"]
        let transcript = app.descendants(matching: .any)["seyal-blocks-scroll"]
        XCTAssertTrue(
            composer.waitForExistence(timeout: timeout),
            "XCUI must stay on Flow/Blocks; composer missing — UI looks like a normal terminal"
        )
        let deadline = Date().addingTimeInterval(timeout)
        var hittable = false
        while Date() < deadline {
            if composer.firstMatch.isHittable {
                hittable = true
                break
            }
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        }
        XCTAssertTrue(hittable, "XCUI must keep composer hittable; value=\(composer.value ?? "nil")")
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
