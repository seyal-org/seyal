import AppKit
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

    /// #1379: completed Flow Blocks must paint output bodies, not titles-only
    /// chrome with an empty white Metal clip.
    func testCompletedFlowBlockKeepsPaintedOutputBody() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        submitComposerCommand(app, "printf 'seyal-1379-body-paint\\n'")
        waitBriefly(1.0)
        assertFlowBlocksOrFail(in: app)
        let body = app.descendants(matching: .any)["seyal-block-0-body"]
        XCTAssertTrue(body.waitForExistence(timeout: 5), "completed Block must expose a body region")
        assertBlockCardFillsTranscript(app, index: 0)
        XCTAssertGreaterThan(
            body.firstMatch.frame.height,
            8,
            "completed Block body must reserve Metal output height, not titles-only chrome"
        )
        attachScreenshot(app, name: "1379-flow-block-body-paint")
    }

    func testPrimaryShellExitIsReportedAndKeepsFlowHistoryVisible() {
        let app = hostedApp()
        waitForUsablePty(in: app)
        // Establish a completed Block before exit. A single `printf…; exit`
        // can race OSC-133 `C` against primary-child death and leave zero
        // Block cards even when retain-on-disconnect is correct (#1379/#1171).
        submitComposerCommand(app, "printf 'seyal-1171-exit-before\\n'")
        let historyBlock = app.descendants(matching: .any)["seyal-block-0"]
        XCTAssertTrue(
            historyBlock.waitForExistence(timeout: 8),
            "pre-exit Flow command must publish a Block card"
        )
        let beforeExit = expectation(
            for: NSPredicate(format: "label CONTAINS %@", "seyal-1171-exit-before"),
            evaluatedWith: historyBlock.firstMatch,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [beforeExit], timeout: 5),
            .completed,
            "pre-exit Block must show the completed command label"
        )
        submitComposerCommand(app, "printf 'seyal-1171-exit-second\\n'")
        let secondBlock = app.descendants(matching: .any)["seyal-block-1"]
        let secondVisible = expectation(
            for: NSPredicate(format: "label CONTAINS %@", "seyal-1171-exit-second"),
            evaluatedWith: secondBlock.firstMatch,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [secondVisible], timeout: 8),
            .completed,
            "a second completed Block must stay a separate card before exit"
        )

        submitComposerCommand(app, "exit 7")

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
        // #1379: an empty `seyal-blocks` container is still hittable — require the
        // command Block that ran before exit, not merely the transcript host.
        XCTAssertTrue(
            historyBlock.waitForExistence(timeout: 5),
            "completed Flow history must keep at least one Block card after shell exit"
        )
        let exitCommandVisible = expectation(
            for: NSPredicate(format: "label CONTAINS %@", "seyal-1171-exit-before"),
            evaluatedWith: historyBlock.firstMatch,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [exitCommandVisible], timeout: 5),
            .completed,
            "Flow Block content from before exit must remain visible; empty chrome is not enough"
        )
        let body = app.descendants(matching: .any)["seyal-block-0-body"]
        let bodyText = expectation(
            for: NSPredicate(
                format: "value CONTAINS %@ OR label CONTAINS %@",
                "seyal-1171-exit-before",
                "seyal-1171-exit-before"
            ),
            evaluatedWith: body.firstMatch,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [bodyText], timeout: 8),
            .completed,
            "completed Block body must keep the pre-exit transcript, not an empty title card; value=\(body.firstMatch.value ?? "nil") label=\(body.firstMatch.label)"
        )
        XCTAssertGreaterThan(
            body.firstMatch.frame.height,
            8,
            "completed Block body must reserve Metal output height after shell exit"
        )
        let secondBody = app.descendants(matching: .any)["seyal-block-1-body"]
        let secondBodyText = expectation(
            for: NSPredicate(
                format: "value CONTAINS %@ OR label CONTAINS %@",
                "seyal-1171-exit-second",
                "seyal-1171-exit-second"
            ),
            evaluatedWith: secondBody.firstMatch,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [secondBodyText], timeout: 8),
            .completed,
            "each completed Block body must keep its own transcript after exit"
        )
        assertBlockCardFillsTranscript(app, index: 0)
        assertBlockCardFillsTranscript(app, index: 1)
        assertFlowPaintCleanAfterExit(app)
        XCTAssertEqual(app.state, .runningForeground)
        attachScreenshot(app, name: "1171-primary-shell-exit")
    }

    /// #1379 P0: clear → command → exit must keep Block bodies painted.
    /// AX body text alone is false-green when Metal still draws the last
    /// viewport over the cards; require flow-paint=ok from the Metal surface.
    func testClearThenExitKeepsPaintedBlockBodies() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        submitComposerCommand(app, "printf 'seyal-1379-before-clear\\n'")
        let before = app.descendants(matching: .any)["seyal-block-0"]
        XCTAssertTrue(before.waitForExistence(timeout: 8))
        // Wait for history AX on the body before clear — card title alone can
        // match the printf argument while Metal/history is still empty (#1379).
        waitForBlockBodyText(app, index: 0, text: "seyal-1379-before-clear", timeout: 8)
        submitComposerCommand(app, "clear")
        waitBriefly(0.8)
        submitComposerCommand(app, "printf 'seyal-1379-after-clear\\n'")
        waitForBlockBodyText(app, index: 2, text: "seyal-1379-after-clear", timeout: 8)
        submitComposerCommand(app, "exit 7")
        let recovery = app.descendants(matching: .any)["seyal-recovery"]
        let ended = expectation(
            for: NSPredicate(format: "value CONTAINS 'shell exited'"),
            evaluatedWith: recovery,
            handler: nil
        )
        XCTAssertEqual(XCTWaiter.wait(for: [ended], timeout: 15), .completed)

        let beforeBody = app.descendants(matching: .any)["seyal-block-0-body"]
        let beforeText = expectation(
            for: NSPredicate(
                format: "value CONTAINS %@ OR label CONTAINS %@",
                "seyal-1379-before-clear",
                "seyal-1379-before-clear"
            ),
            evaluatedWith: beforeBody.firstMatch,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [beforeText], timeout: 8),
            .completed,
            "pre-clear body AX must remain after clear→exit"
        )
        let afterBody = app.descendants(matching: .any)["seyal-block-2-body"]
        let afterText = expectation(
            for: NSPredicate(
                format: "value CONTAINS %@ OR label CONTAINS %@",
                "seyal-1379-after-clear",
                "seyal-1379-after-clear"
            ),
            evaluatedWith: afterBody.firstMatch,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [afterText], timeout: 8),
            .completed,
            "post-clear body AX must remain after clear→exit"
        )
        XCTAssertGreaterThan(beforeBody.firstMatch.frame.height, 8)
        XCTAssertGreaterThan(afterBody.firstMatch.frame.height, 8)
        assertBlockCardFillsTranscript(app, index: 0)
        assertBlockCardFillsTranscript(app, index: 2)
        // before-clear + after-clear must each paint; collapse onto one card fails.
        assertFlowPaintCleanAfterExit(
            app,
            minimumPaintedRegions: 2,
            inkBodyIndices: [0, 2]
        )
        attachScreenshot(app, name: "1379-clear-then-exit-paint")
    }

    /// #1379 Gate 1: repeat clear→exit→New shell ten times in one headed
    /// session. Cold-launch XCUI flake must not mask the paint regression.
    ///
    /// Opt-in only. Default `make test` / `native-macos-smoke` skips this
    /// (~6–7 min) so the 35-minute smoke job can finish `make bench`. CI
    /// regression stays on `testClearThenExitKeepsPaintedBlockBodies`.
    ///
    /// Enable with either:
    /// - `SEYAL_UI_TEST_CLEAR_EXIT_STRESS=1 make ui-test` (script forwards
    ///   `TEST_RUNNER_…` into the XCUI runner), or
    /// - `TEST_RUNNER_SEYAL_UI_TEST_CLEAR_EXIT_STRESS=1` on a raw `xcodebuild
    ///   test` (xcodebuild only injects `TEST_RUNNER_`-prefixed vars; a bare
    ///   `SEYAL_UI_TEST_…=1` silently skips and still reports suite PASS).
    func testClearThenExitPaintStableAcrossTenCycles() throws {
        let env = ProcessInfo.processInfo.environment
        let stressEnabled = env["SEYAL_UI_TEST_CLEAR_EXIT_STRESS"] == "1"
            || env["TEST_RUNNER_SEYAL_UI_TEST_CLEAR_EXIT_STRESS"] == "1"
        guard stressEnabled else {
            throw XCTSkip(
                "enable Gate 1 10-cycle clear→exit stress via SEYAL_UI_TEST_CLEAR_EXIT_STRESS=1 make ui-test (or TEST_RUNNER_SEYAL_UI_TEST_CLEAR_EXIT_STRESS=1 on xcodebuild); default make test keeps the single-cycle paint case"
            )
        }
        let app = hostedApp()
        waitForUsablePty(in: app)
        for cycle in 1...10 {
            let beforeToken = "seyal-1379-c\(cycle)-before"
            let afterToken = "seyal-1379-c\(cycle)-after"
            submitComposerCommand(app, "printf '\(beforeToken)\\n'")
            waitForBlockBodyText(app, index: 0, text: beforeToken, timeout: 8)
            submitComposerCommand(app, "clear")
            waitBriefly(0.8)
            submitComposerCommand(app, "printf '\(afterToken)\\n'")
            waitForBlockBodyText(app, index: 2, text: afterToken, timeout: 8)
            submitComposerCommand(app, "exit 7")
            let recovery = app.descendants(matching: .any)["seyal-recovery"]
            let ended = expectation(
                for: NSPredicate(format: "value CONTAINS 'shell exited'"),
                evaluatedWith: recovery,
                handler: nil
            )
            XCTAssertEqual(
                XCTWaiter.wait(for: [ended], timeout: 15),
                .completed,
                "cycle \(cycle): shell exit recovery missing"
            )
            assertFlowPaintCleanAfterExit(
                app,
                minimumPaintedRegions: 2,
                inkBodyIndices: [0, 2]
            )
            let newShell = app.descendants(matching: .any)["seyal-new-shell"].firstMatch
            XCTAssertTrue(
                newShell.waitForExistence(timeout: 5),
                "cycle \(cycle): New shell must be offered after exit"
            )
            newShell.click()
            let composer = app.descendants(matching: .any)["seyal-composer"].firstMatch
            let usable = expectation(
                for: NSPredicate(format: "value == 'available'"),
                evaluatedWith: composer,
                handler: nil
            )
            XCTAssertEqual(
                XCTWaiter.wait(for: [usable], timeout: 8),
                .completed,
                "cycle \(cycle): New shell stayed \(composer.value ?? "nil")"
            )
        }
        attachScreenshot(app, name: "1379-clear-then-exit-10x")
    }

    /// #1379: New shell after exit must become a usable Flow composer. The
    /// ended shell's admission must not leave "Waiting for prompt..." up
    /// while the new PTY is already at a prompt.
    func testNewShellAfterExitBecomesUsable() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        submitComposerCommand(app, "exit 0")
        let recovery = app.descendants(matching: .any)["seyal-recovery"]
        let ended = expectation(
            for: NSPredicate(format: "value CONTAINS 'shell exited'"),
            evaluatedWith: recovery,
            handler: nil
        )
        XCTAssertEqual(XCTWaiter.wait(for: [ended], timeout: 15), .completed)
        let newShell = app.descendants(matching: .any)["seyal-new-shell"].firstMatch
        XCTAssertTrue(newShell.waitForExistence(timeout: 5), "New shell must be offered after exit")
        newShell.click()
        let composer = app.descendants(matching: .any)["seyal-composer"].firstMatch
        let usable = expectation(
            for: NSPredicate(format: "value == 'available'"),
            evaluatedWith: composer,
            handler: nil
        )
        // Click→available is ~1–2s on this host; 8s rejects a 15–25s hang
        // without masking it as a soft wait (#1379 P1).
        XCTAssertEqual(
            XCTWaiter.wait(for: [usable], timeout: 8),
            .completed,
            "New shell composer stayed \(composer.value ?? "nil"); prompt wait must not persist"
        )
        submitComposerCommand(app, "printf 'seyal-1379-new-shell\\n'")
        let body = app.descendants(matching: .any)["seyal-block-0-body"]
        let painted = expectation(
            for: NSPredicate(
                format: "value CONTAINS %@ OR label CONTAINS %@",
                "seyal-1379-new-shell",
                "seyal-1379-new-shell"
            ),
            evaluatedWith: body.firstMatch,
            handler: nil
        )
        XCTAssertEqual(XCTWaiter.wait(for: [painted], timeout: 8), .completed)
        assertBlockCardFillsTranscript(app, index: 0)
        XCTAssertEqual(composer.value as? String, "available")
        attachScreenshot(app, name: "1379-new-shell")
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
        app.activate()
        composer.firstMatch.click()
        let editor = app.descendants(matching: .any)["seyal-composer-editor"]
        // Prefer a single typeText with newline — separate typeKey("\r") often
        // times out synthesizing during long headed Gate 1 loops.
        if editor.waitForExistence(timeout: 2), editor.firstMatch.isHittable {
            editor.firstMatch.click()
            editor.firstMatch.typeText(command + "\n")
        } else {
            composer.firstMatch.typeText(command + "\n")
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
        // Headed sequential XCUI can leave the next launch BackgroundSuspended when
        // the IDE steals focus; activate before PTY attach or hittable chrome fails.
        app.activate()
        let terminal = app.descendants(matching: .any)["terminal-input"]
        XCTAssertTrue(terminal.waitForExistence(timeout: 10), "terminal surface missing")
        let usable = NSPredicate(format: "value CONTAINS 'connection=usable'")
        let arrived = expectation(for: usable, evaluatedWith: terminal, handler: nil)
        let result = XCTWaiter.wait(for: [arrived], timeout: timeout)
        if app.state != .runningForeground {
            app.activate()
        }
        XCTAssertEqual(
            app.state,
            .runningForeground,
            "Seyal.app left foreground while waiting for PTY attach (state=\(app.state.rawValue))"
        )
        XCTAssertEqual(
            result,
            .completed,
            "PTY/runtime never became usable; terminal AX=\(terminal.value ?? "nil"). If another Runtime holds control.sock this run is INCONCLUSIVE."
        )
        assertFlowBlocksOrFail(in: app)
    }

    private func assertBlockCardFillsTranscript(_ app: XCUIApplication, index: Int) {
        let blocks = app.descendants(matching: .any)["seyal-blocks"].firstMatch
        let card = app.descendants(matching: .any)["seyal-block-\(index)"].firstMatch
        XCTAssertTrue(blocks.exists, "seyal-blocks missing")
        XCTAssertTrue(card.exists, "seyal-block-\(index) missing")
        XCTAssertGreaterThan(
            card.frame.width,
            blocks.frame.width * 0.85,
            "Flow Block cards must span the transcript, not shrink to the command string"
        )
        XCTAssertLessThan(
            abs(card.frame.minX - blocks.frame.minX),
            24,
            "Flow Block cards must start with the transcript, not hug the trailing edge"
        )
    }

    /// Metal reports flow-paint=ok only when Flow draws history inside Block
    /// clips without a leftover live-grid / out-of-clip overlay. AX body text
    /// can stay correct while the screen is blank or later Block output is
    /// collapsed onto an earlier card; require painted history instances,
    /// multiple painted regions, and distinct clips (#1379 false-green guard).
    private func assertFlowPaintCleanAfterExit(
        _ app: XCUIApplication,
        minimumPaintedRegions: Int = 1,
        inkBodyIndices: [Int] = [0]
    ) {
        let terminal = app.descendants(matching: .any)["terminal-input"].firstMatch
        XCTAssertTrue(terminal.exists, "terminal surface missing for flow-paint probe")
        let clean = expectation(
            for: NSPredicate(format: "value CONTAINS 'flow-paint=ok'"),
            evaluatedWith: terminal,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [clean], timeout: 5),
            .completed,
            "Metal Flow paint must stay clean after exit; AX=\(terminal.value ?? "nil") (flow-paint=leak means viewport overlay survived)"
        )
        let painted = expectation(
            for: NSPredicate(format: "value MATCHES %@", ".*history-instances=[1-9][0-9]*.*"),
            evaluatedWith: terminal,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [painted], timeout: 5),
            .completed,
            "retained Flow Blocks must paint history glyphs after exit; AX=\(terminal.value ?? "nil") (history-instances=0 with AX body text is false-green)"
        )
        let regions = expectation(
            for: NSPredicate(
                format: "value MATCHES %@",
                ".*history-regions-painted=([\(minimumPaintedRegions)-9]|[1-9][0-9]+).*"
            ),
            evaluatedWith: terminal,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [regions], timeout: 5),
            .completed,
            "need ≥\(minimumPaintedRegions) painted history regions; AX=\(terminal.value ?? "nil")"
        )
        let clips = expectation(
            for: NSPredicate(format: "value CONTAINS 'history-clips=ok'"),
            evaluatedWith: terminal,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [clips], timeout: 5),
            .completed,
            "history Block clips must stay distinct; AX=\(terminal.value ?? "nil") (history-clips=collapse is clear→exit mispaint)"
        )
        assertBlockBodyFramesHaveInk(app, indices: inkBodyIndices)
    }

    /// Per-element screenshot ink probe. Uses `body.screenshot()` so sampling
    /// cannot pick up card titles / chrome from a full-screen `app.screenshot`
    /// coordinate mis-map (#1379 Opus false-green).
    private func assertBlockBodyFramesHaveInk(_ app: XCUIApplication, indices: [Int]) {
        _ = app
        for index in indices {
            let body = app.descendants(matching: .any)["seyal-block-\(index)-body"].firstMatch
            XCTAssertTrue(
                body.waitForExistence(timeout: 2),
                "seyal-block-\(index)-body missing for ink probe"
            )
            let frame = body.frame
            XCTAssertGreaterThan(frame.width, 4, "seyal-block-\(index)-body frame too narrow")
            XCTAssertGreaterThan(frame.height, 4, "seyal-block-\(index)-body frame too short")
            let shot = body.screenshot()
            let image = shot.image
            var proposed = CGRect(origin: .zero, size: image.size)
            guard let cgImage = image.cgImage(forProposedRect: &proposed, context: nil, hints: nil),
                  let data = cgImage.dataProvider?.data,
                  let ptr = CFDataGetBytePtr(data)
            else {
                XCTFail("seyal-block-\(index)-body screenshot has no pixels")
                continue
            }
            let bytesPerPixel = max(cgImage.bitsPerPixel / 8, 1)
            let bytesPerRow = cgImage.bytesPerRow
            var ink = 0
            var samples = 0
            let step = max(min(cgImage.width, cgImage.height) / 16, 1)
            var y = 0
            while y < cgImage.height {
                var x = 0
                while x < cgImage.width {
                    let offset = y * bytesPerRow + x * bytesPerPixel
                    let r = Int(ptr[offset])
                    let g = Int(ptr[offset + min(1, bytesPerPixel - 1)])
                    let b = Int(ptr[offset + min(2, bytesPerPixel - 1)])
                    samples += 1
                    // Flow body canvas is near-white; glyph ink is darker.
                    if (r + g + b) < 650 {
                        ink += 1
                    }
                    x += step
                }
                y += step
            }
            XCTAssertGreaterThan(
                samples,
                8,
                "seyal-block-\(index)-body ink probe under-sampled (\(samples))"
            )
            XCTAssertGreaterThan(
                ink,
                0,
                "seyal-block-\(index)-body has AX text but no painted ink in body screenshot (false-green)"
            )
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

    private func waitForBlockBodyText(
        _ app: XCUIApplication,
        index: Int,
        text: String,
        timeout: TimeInterval
    ) {
        let body = app.descendants(matching: .any)["seyal-block-\(index)-body"].firstMatch
        XCTAssertTrue(body.waitForExistence(timeout: timeout), "seyal-block-\(index)-body missing")
        let painted = expectation(
            for: NSPredicate(
                format: "value CONTAINS %@ OR label CONTAINS %@",
                text,
                text
            ),
            evaluatedWith: body,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [painted], timeout: timeout),
            .completed,
            "seyal-block-\(index)-body must show \(text) before continuing (card title alone is false-green)"
        )
    }

    private func attachScreenshot(_ app: XCUIApplication, name: String) {
        let shot = XCTAttachment(screenshot: app.screenshot())
        shot.name = name
        shot.lifetime = .keepAlways
        add(shot)
    }
}
