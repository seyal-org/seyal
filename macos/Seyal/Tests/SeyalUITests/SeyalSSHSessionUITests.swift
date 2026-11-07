import AppKit
import XCTest
import Vision

/// Packaged-app acceptance for ADR-026 spawn-time SSH session Blocks.
/// The local fake SSH transport executes the final remote command with a fresh bash,
/// exercising the Runtime/PTY/Flow path without credentials or remote writes.
@MainActor
final class SeyalSSHSessionUITests: XCTestCase {
    override func setUp() {
        continueAfterFailure = false
    }

    func testSpawnTimeSSHShowsSessionBlockWithoutEnableBlocksCeremony() throws {
        guard let remoteBash = bashWithReadN() else {
            throw XCTSkip("spawn-time SSH delivery requires bash 4.1 or newer")
        }

        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("seyal-1398-\(UUID().uuidString)", isDirectory: true)
        let bin = root.appendingPathComponent("bin", isDirectory: true)
        let localHome = root.appendingPathComponent("local-home", isDirectory: true)
        let remoteHome = root.appendingPathComponent("remote-home", isDirectory: true)
        try FileManager.default.createDirectory(at: bin, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: localHome, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: remoteHome, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let fakeSSH = bin.appendingPathComponent("ssh")
        let invocation = root.appendingPathComponent("fake-ssh-invoked")
        let script = """
        #!/bin/sh
        : > '\(invocation.path)'
        remote=
        for arg do remote=$arg; done
        exec /usr/bin/env -u ZDOTDIR -u SEYAL_NONCE_FD -u HISTFILE HOME='\(remoteHome.path)' SHELL='\(remoteBash)' '\(remoteBash)' -c "$remote"
        """
        try script.write(to: fakeSSH, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes(
            [.posixPermissions: 0o755],
            ofItemAtPath: fakeSSH.path
        )

        let shell = bin.appendingPathComponent("zsh")
        let shellWrapper = """
        #!/bin/sh
        PATH="\(bin.path):/opt/homebrew/bin:/usr/bin:/bin"
        export PATH
        exec /bin/zsh "$@"
        """
        try shellWrapper.write(to: shell, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes(
            [.posixPermissions: 0o755],
            ofItemAtPath: shell.path
        )
        let config = root.appendingPathComponent("config.toml")
        try "[shell]\nprogram = \"\(shell.path)\"\n".write(
            to: config,
            atomically: true,
            encoding: .utf8
        )

        let app = XCUIApplication()
        app.launchIsolatedHost(
            environment: [
                IsolatedHostedRuntime.runtimeConfigEnvironmentKey: config.path,
                "HOME": localHome.path,
                "ZDOTDIR": localHome.path,
            ],
            helperCommand: shell.path
        )
        XCTAssertTrue(app.wait(for: .runningForeground, timeout: 10))
        app.activate()
        let terminal = app.descendants(matching: .any)["terminal-input"].firstMatch
        XCTAssertTrue(terminal.waitForExistence(timeout: 10))
        let usable = expectation(
            for: NSPredicate(format: "value CONTAINS 'connection=usable'"),
            evaluatedWith: terminal,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [usable], timeout: 20),
            .completed,
            "PTY/runtime never became usable; terminal AX=\(terminal.value ?? "nil"); app=\n\(app.debugDescription)"
        )
        app.ensureFlowBlocksPresentation()

        submitComposerCommand(app, "command -v ssh")
        let pathBody = app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier ENDSWITH '-body'")
        ).firstMatch
        XCTAssertTrue(
            pathBody.waitForExistence(timeout: 8),
            "the shell's command lookup must produce a Block body:\n\(app.debugDescription)"
        )
        let resolvedSSH = pathBody.label
        XCTAssertTrue(
            resolvedSSH.contains("seyal-1398-") && resolvedSSH.hasSuffix("/bin/ssh"),
            "the isolated shell must resolve ssh to its fake transport; got \(resolvedSSH)"
        )

        submitComposerCommand(app, "ssh -p 2222 me@test-host")
        let composer = app.descendants(matching: .any)["seyal-composer"].firstMatch
        waitForComposerValue(composer, "available", timeout: 25)
        XCTAssertFalse(
            app.descendants(matching: .any)["seyal-session-bootstrap"].exists,
            "eligible spawn-time SSH must not show the Enable Blocks consent banner"
        )

        let host = blockCommand(app, containing: "ssh -p 2222 me@test-host")
        XCTAssertTrue(
            host.waitForExistence(timeout: 8),
            "the submitted SSH command needs a host Block:\n\(app.debugDescription)"
        )
        XCTAssertTrue(
            waitForFile(invocation.path, timeout: 8),
            "the local fake SSH executable was not invoked"
        )
        submitComposerCommand(app, "echo seyal-1398-session-output")
        let sessionCommand = blockCommand(app, containing: "echo seyal-1398-session-output")
        XCTAssertTrue(
            sessionCommand.waitForExistence(timeout: 12),
            "remote command Block missing:\n\(app.debugDescription)"
        )
        XCTAssertTrue(
            (sessionCommand.value as? String ?? "").contains("session"),
            "the Rust-projected session flag must distinguish remote Blocks:\n\(app.debugDescription)"
        )
        let sessionBody = sessionCommand.descendants(matching: .any).matching(
            NSPredicate(format: "identifier ENDSWITH '-body'")
        ).firstMatch
        XCTAssertTrue(
            sessionBody.waitForExistence(timeout: 12),
            "the session Block body is missing:\n\(app.debugDescription)"
        )
        let output = expectation(
            for: NSPredicate(
                format: "label CONTAINS %@ OR value CONTAINS %@",
                "seyal-1398-session-output",
                "seyal-1398-session-output"
            ),
            evaluatedWith: sessionBody,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [output], timeout: 12),
            .completed,
            "the completed session Block must contain the remote output:\n\(app.debugDescription)"
        )
        assertPaintedOutput(sessionBody, marker: "seyal-1398-session-output")
        let hostBody = host.descendants(matching: .any).matching(
            NSPredicate(format: "identifier ENDSWITH '-body'")
        ).firstMatch
        XCTAssertTrue(hostBody.waitForExistence(timeout: 8))
        XCTAssertFalse(
            (hostBody.label + " " + String(describing: hostBody.value ?? ""))
                .contains("seyal-1398-session-output"),
            "remote output belongs only to its session Block, not the SSH host Block:\n\(app.debugDescription)"
        )

        // A line-oriented interactive program keeps its input in the running
        // session Block. Composer Relay submits plaintext to that foreground
        // process without creating another Block or changing session scope.
        submitComposerCommand(app, "python3 -q")
        let pythonBlock = blockCommand(app, containing: "python3 -q")
        XCTAssertTrue(pythonBlock.waitForExistence(timeout: 12), "Python must own a running session Block")
        waitForComposerValue(composer, "relay", timeout: 20)
        XCTAssertTrue((pythonBlock.value as? String ?? "").contains("session"))
        let inlineComposer = app.descendants(matching: .any)["seyal-composer"].firstMatch
        XCTAssertTrue(
            inlineComposer.waitForExistence(timeout: 5),
            "the Pane must expose its single composer independently from Block history:\n\(app.debugDescription)"
        )
        XCTAssertTrue(inlineComposer.isHittable, "the Pane composer must paint above the terminal surface")
        XCTAssertGreaterThanOrEqual(
            inlineComposer.frame.minY,
            pythonBlock.frame.minY,
            "the Pane composer must align with the running Python Block"
        )
        XCTAssertLessThanOrEqual(
            inlineComposer.frame.maxY,
            pythonBlock.frame.maxY,
            "the Pane composer must align with the running Python Block"
        )
        let runningPythonBody = pythonBlock.descendants(matching: .any).matching(
            NSPredicate(format: "identifier ENDSWITH '-body'")
        ).firstMatch
        XCTAssertTrue(runningPythonBody.waitForExistence(timeout: 5))
        XCTAssertTrue(
            inlineComposer.frame.intersects(runningPythonBody.frame),
            "Relay input must share the live terminal prompt row, not sit in a second row below it"
        )
        let blocksScroll = app.scrollViews["seyal-blocks-scroll"].firstMatch
        XCTAssertTrue(blocksScroll.exists)
        let liveRelayLayout = XCTAttachment(screenshot: app.screenshot())
        liveRelayLayout.name = "1398-ssh-python-live-relay-layout"
        liveRelayLayout.lifetime = .keepAlways
        add(liveRelayLayout)
        XCTAssertLessThanOrEqual(
            blocksScroll.frame.maxY - pythonBlock.frame.maxY,
            96,
            "short Flow history and the active input must sit at the bottom of the transcript"
        )
        XCTAssertGreaterThanOrEqual(
            blocksScroll.frame.maxY,
            app.windows.firstMatch.frame.maxY - 160,
            "Relay must keep the Flow viewport connected to the bottom of the Pane"
        )
        XCTAssertLessThanOrEqual(
            blocksScroll.frame.maxY - inlineComposer.frame.maxY,
            128,
            "the active Python prompt must remain docked near the bottom edge with its Blocks"
        )
        let pythonMarker = "seyal-1398-python-line"
        submitComposerCommand(
            app,
            "print('\(pythonMarker)')",
            expectedComposerValue: "relay",
            beforeSubmit: { editor in
                XCTAssertTrue(
                    (editor.value as? String ?? "").contains(pythonMarker),
                    "Python's draft must remain in the editor embedded at its prompt"
                )
                XCTAssertTrue(
                    editor.isHittable,
                    "the Python prompt editor must be visible and hittable"
                )
                XCTAssertTrue(editor.frame.intersects(runningPythonBody.frame))
                let inlineDraftScreenshot = app.screenshot()
                let editorAttachment = XCTAttachment(screenshot: inlineDraftScreenshot)
                editorAttachment.name = "1398-ssh-python-inline-editor-before-submit"
                editorAttachment.lifetime = .keepAlways
                self.add(editorAttachment)
                do {
                    try self.assertVisibleText(in: inlineDraftScreenshot, fragments: [pythonMarker])
                } catch {
                    XCTFail("Could not recognize the rendered inline Python draft: \(error)")
                }
            }
        )
        waitForComposerValue(composer, "relay", timeout: 12)
        let livePython = XCTAttachment(screenshot: app.screenshot())
        livePython.name = "1398-ssh-python-running-block-editor"
        livePython.lifetime = .keepAlways
        add(livePython)
        let pythonBody = runningPythonBody
        XCTAssertEqual(
            pythonBody.frame.minX,
            sessionBody.frame.minX,
            accuracy: 1,
            "the running SSH Python Block must align with completed SSH command Blocks"
        )
        XCTAssertEqual(
            pythonBody.frame.maxX,
            sessionBody.frame.maxX,
            accuracy: 1,
            "the running SSH Python Block must use the normal Block width"
        )
        XCTAssertFalse(
            blockCommand(app, containing: pythonMarker).exists,
            "Relay input belongs to the running Python Block and must not create a child Block"
        )
        assertPaintedOutput(pythonBlock, marker: pythonMarker)
        submitComposerCommand(app, "exit()", expectedComposerValue: "relay")
        waitForComposerValue(composer, "available", timeout: 20)
        XCTAssertTrue(pythonBody.waitForExistence(timeout: 12))
        let completedPythonOutput = expectation(
            for: NSPredicate(format: "label CONTAINS %@ OR value CONTAINS %@", pythonMarker, pythonMarker),
            evaluatedWith: pythonBody,
            handler: nil
        )
        XCTAssertEqual(XCTWaiter.wait(for: [completedPythonOutput], timeout: 12), .completed)

        // Full-screen remote work takes over the execution's TUI surface and
        // restores the SSH prompt's Flow composer after leaving alternate screen.
        submitComposerCommand(app, "vim -Nu NONE -n")
        let vimBlock = blockCommand(app, containing: "vim -Nu NONE -n")
        let hidden = expectation(
            for: NSPredicate(format: "isHittable == false"),
            evaluatedWith: composer,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [hidden], timeout: 15),
            .completed,
            "alternate-screen vim must take over the Flow composer"
        )
        let tui = app.descendants(matching: .any)["terminal-input"].firstMatch
        XCTAssertTrue(tui.waitForExistence(timeout: 5), "vim must retain the live terminal surface")
        let activeTui = XCTAttachment(screenshot: app.screenshot())
        activeTui.name = "1398-ssh-session-vim-tui-active"
        activeTui.lifetime = .keepAlways
        add(activeTui)
        if tui.isHittable { tui.click() }
        app.typeKey(.escape, modifierFlags: [])
        RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        app.typeText(":q!")
        app.typeKey("\r", modifierFlags: [])
        waitForComposerValue(composer, "available", timeout: 20)
        XCTAssertTrue(
            vimBlock.waitForExistence(timeout: 8),
            "the completed vim command must return to the session Block timeline after TUI"
        )
        XCTAssertTrue((vimBlock.value as? String ?? "").contains("session"))
        let afterVimMarker = "seyal-1398-after-vim"
        submitComposerCommand(app, "echo \(afterVimMarker)")
        let afterVimBlock = blockCommand(app, containing: afterVimMarker)
        XCTAssertTrue(afterVimBlock.waitForExistence(timeout: 12))
        let afterVimBody = afterVimBlock.descendants(matching: .any).matching(
            NSPredicate(format: "identifier ENDSWITH '-body'")
        ).firstMatch
        XCTAssertTrue(afterVimBody.waitForExistence(timeout: 12))
        let afterVimOutput = expectation(
            for: NSPredicate(
                format: "label CONTAINS %@ OR value CONTAINS %@",
                afterVimMarker,
                afterVimMarker
            ),
            evaluatedWith: afterVimBody,
            handler: nil
        )
        XCTAssertEqual(XCTWaiter.wait(for: [afterVimOutput], timeout: 12), .completed)
        let restored = XCTAttachment(screenshot: app.screenshot())
        restored.name = "1398-ssh-session-vim-tui-restored-flow"
        restored.lifetime = .keepAlways
        add(restored)
        XCTAssertEqual(app.state, .runningForeground)
        let screenshot = XCTAttachment(screenshot: app.screenshot())
        screenshot.name = "1398-spawn-time-ssh-session-block"
        screenshot.lifetime = .keepAlways
        add(screenshot)
    }

    func testZshSSHNeedsExplicitConsentAndCreatesSessionBlocksWithoutRemoteFiles() throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("seyal-1398-zsh-\(UUID().uuidString)", isDirectory: true)
        let bin = root.appendingPathComponent("bin", isDirectory: true)
        let localHome = root.appendingPathComponent("local-home", isDirectory: true)
        let remoteHome = root.appendingPathComponent("remote-home", isDirectory: true)
        try FileManager.default.createDirectory(at: bin, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: localHome, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: remoteHome, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let fakeSSH = bin.appendingPathComponent("ssh")
        let invocation = root.appendingPathComponent("fake-ssh-invoked")
        let script = """
        #!/bin/sh
        : > '\(invocation.path)'
        remote=
        for arg do remote=$arg; done
        exec /usr/bin/env SHELL=/bin/zsh HOME='\(remoteHome.path)' ZDOTDIR='\(remoteHome.path)' HISTFILE=/dev/null /bin/zsh -c "$remote"
        """
        try script.write(to: fakeSSH, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes(
            [.posixPermissions: 0o755],
            ofItemAtPath: fakeSSH.path
        )

        let shell = bin.appendingPathComponent("zsh")
        let shellWrapper = """
        #!/bin/sh
        PATH="\(bin.path):/opt/homebrew/bin:/usr/bin:/bin"
        export PATH
        exec /bin/zsh "$@"
        """
        try shellWrapper.write(to: shell, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes(
            [.posixPermissions: 0o755],
            ofItemAtPath: shell.path
        )
        let config = root.appendingPathComponent("config.toml")
        try "[shell]\nprogram = \"\(shell.path)\"\n".write(
            to: config,
            atomically: true,
            encoding: .utf8
        )

        let app = XCUIApplication()
        app.launchIsolatedHost(
            environment: [
                IsolatedHostedRuntime.runtimeConfigEnvironmentKey: config.path,
                "HOME": localHome.path,
                "ZDOTDIR": localHome.path,
            ],
            helperCommand: shell.path
        )
        XCTAssertTrue(app.wait(for: .runningForeground, timeout: 10))
        app.activate()
        let terminal = app.descendants(matching: .any)["terminal-input"].firstMatch
        XCTAssertTrue(terminal.waitForExistence(timeout: 10))
        let usable = expectation(
            for: NSPredicate(format: "value CONTAINS 'connection=usable'"),
            evaluatedWith: terminal,
            handler: nil
        )
        XCTAssertEqual(XCTWaiter.wait(for: [usable], timeout: 20), .completed)
        app.ensureFlowBlocksPresentation()

        submitComposerCommand(app, "ssh -p 2222 me@test-host")
        let host = blockCommand(app, containing: "ssh -p 2222 me@test-host")
        XCTAssertTrue(host.waitForExistence(timeout: 10), "SSH needs its host Block")
        XCTAssertTrue(waitForFile(invocation.path, timeout: 8), "fake SSH was not invoked")

        let banner = app.descendants(matching: .any)["seyal-session-bootstrap"].firstMatch
        XCTAssertTrue(
            banner.waitForExistence(timeout: 10),
            "remote zsh must request consent because this path cannot bootstrap at spawn time"
        )
        let primary = app.descendants(matching: .any)["seyal-session-bootstrap-primary"].firstMatch
        XCTAssertTrue(primary.waitForExistence(timeout: 5))
        primary.click() // Explicitly request the consented typed path.

        let consentBody = app.descendants(matching: .any)["seyal-session-bootstrap-body"].firstMatch
        let setupTitle = app.descendants(matching: .any)["seyal-session-bootstrap-title"].firstMatch
        XCTAssertEqual(
            setupTitle.value as? String,
            "Enable Blocks for this session?",
            "session consent must not present Blocks as disabled by default"
        )
        let consentCopy = expectation(
            for: NSPredicate(format: "value CONTAINS[c] %@", "type one setup line into this session"),
            evaluatedWith: consentBody,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [consentCopy], timeout: 8),
            .completed,
            "typed zsh bootstrap must ask for explicit consent"
        )
        primary.click()

        let composer = app.descendants(matching: .any)["seyal-composer"].firstMatch
        waitForComposerValue(composer, "available", timeout: 25)
        let marker = "seyal-1398-zsh-session-output"
        submitComposerCommand(app, "echo \(marker)")
        let command = blockCommand(app, containing: "echo \(marker)")
        XCTAssertTrue(command.waitForExistence(timeout: 12), "zsh session command Block is missing")
        XCTAssertTrue(
            (command.value as? String ?? "").contains("session"),
            "Rust must project the remote session scope"
        )
        let body = command.descendants(matching: .any).matching(
            NSPredicate(format: "identifier ENDSWITH '-body'")
        ).firstMatch
        XCTAssertTrue(body.waitForExistence(timeout: 12))
        let output = expectation(
            for: NSPredicate(format: "label CONTAINS %@ OR value CONTAINS %@", marker, marker),
            evaluatedWith: body,
            handler: nil
        )
        XCTAssertEqual(XCTWaiter.wait(for: [output], timeout: 12), .completed)
        assertPaintedOutput(body, marker: marker)
        XCTAssertEqual(
            try FileManager.default.contentsOfDirectory(atPath: remoteHome.path),
            [],
            "the no-remote-files policy must hold throughout the consented path"
        )
    }

    private func bashWithReadN() -> String? {
        ["/opt/homebrew/bin/bash", "/usr/local/bin/bash"].first { path in
            guard FileManager.default.isExecutableFile(atPath: path) else { return false }
            let process = Process()
            process.executableURL = URL(fileURLWithPath: path)
            process.arguments = ["-c", "((BASH_VERSINFO[0]*100+BASH_VERSINFO[1]>=401))"]
            do {
                try process.run()
                process.waitUntilExit()
                return process.terminationStatus == 0
            } catch {
                return false
            }
        }
    }

    private func submitComposerCommand(
        _ app: XCUIApplication,
        _ command: String,
        expectedComposerValue: String = "available",
        beforeSubmit: ((XCUIElement) -> Void)? = nil
    ) {
        app.ensureFlowBlocksPresentation()
        let composer = app.descendants(matching: .any)["seyal-composer"].firstMatch
        waitForComposerValue(composer, expectedComposerValue, timeout: 20)
        let editor = app.descendants(matching: .any)["seyal-composer-editor"].firstMatch
        XCTAssertTrue(editor.waitForExistence(timeout: 5))
        // Relay keeps the same text view first responder as it moves into the
        // live Block. Clicking the parent card would select the Block; type
        // directly into the already-focused editor in that state.
        if expectedComposerValue == "available" {
            XCTAssertTrue(editor.isHittable, "the docked composer editor must be hittable")
            editor.click()
        }
        editor.typeText(command)
        beforeSubmit?(editor)
        editor.typeText("\n")
        let cleared = expectation(
            for: NSPredicate(format: "value == nil OR value == ''"),
            evaluatedWith: editor,
            handler: nil
        )
        XCTAssertEqual(XCTWaiter.wait(for: [cleared], timeout: 8), .completed)
    }

    private func waitForComposerValue(
        _ composer: XCUIElement,
        _ value: String,
        timeout: TimeInterval
    ) {
        XCTAssertTrue(composer.waitForExistence(timeout: min(timeout, 10)))
        let ready = expectation(
            for: NSPredicate(format: "value == %@", value),
            evaluatedWith: composer,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [ready], timeout: timeout),
            .completed,
            "composer expected \(value), actual=\(composer.value ?? "nil")"
        )
    }

    private func waitForFile(_ path: String, timeout: TimeInterval) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if FileManager.default.fileExists(atPath: path) { return true }
            RunLoop.current.run(until: Date().addingTimeInterval(0.05))
        }
        return FileManager.default.fileExists(atPath: path)
    }

    private func blockCommand(_ app: XCUIApplication, containing text: String) -> XCUIElement {
        app.descendants(matching: .any).matching(
            NSPredicate(
                format: "identifier BEGINSWITH 'seyal-block-' AND NOT identifier ENDSWITH '-body' AND label CONTAINS %@",
                text
            )
        ).firstMatch
    }

    private func assertPaintedOutput(_ body: XCUIElement, marker: String) {
        let shot = body.screenshot()
        let attachment = XCTAttachment(screenshot: shot)
        attachment.name = "1398-session-body-\(marker)"
        attachment.lifetime = .keepAlways
        add(attachment)
        let image = shot.image
        var proposed = CGRect(origin: .zero, size: image.size)
        guard let cgImage = image.cgImage(
            forProposedRect: &proposed,
            context: nil,
            hints: nil
        ), let data = cgImage.dataProvider?.data,
           let bytes = CFDataGetBytePtr(data)
        else {
            XCTFail("SSH session Block screenshot has no pixels")
            return
        }
        let bytesPerPixel = max(cgImage.bitsPerPixel / 8, 1)
        let step = max(min(cgImage.width, cgImage.height) / 16, 1)
        var ink = 0
        var y = 0
        while y < cgImage.height {
            var x = 0
            while x < cgImage.width {
                let offset = y * cgImage.bytesPerRow + x * bytesPerPixel
                let red = Int(bytes[offset])
                let green = Int(bytes[offset + min(1, bytesPerPixel - 1)])
                let blue = Int(bytes[offset + min(2, bytesPerPixel - 1)])
                if red + green + blue < 650 { ink += 1 }
                x += step
            }
            y += step
        }
        XCTAssertGreaterThan(
            ink,
            0,
            "SSH session Block (marker) has text in AX but no painted output pixels"
        )
    }

    private func assertVisibleText(
        in screenshot: XCUIScreenshot,
        fragments: [String],
        file: StaticString = #filePath,
        line: UInt = #line
    ) throws {
        let image = screenshot.image
        var proposed = CGRect(origin: .zero, size: image.size)
        guard let cgImage = image.cgImage(
            forProposedRect: &proposed,
            context: nil,
            hints: nil
        ) else {
            XCTFail("Block screenshot has no pixels", file: file, line: line)
            return
        }
        let request = VNRecognizeTextRequest()
        request.recognitionLevel = .accurate
        request.usesLanguageCorrection = false
        try VNImageRequestHandler(cgImage: cgImage).perform([request])
        let visibleText = request.results?
            .compactMap { $0.topCandidates(1).first?.string }
            .joined(separator: "\n") ?? ""
        for fragment in fragments {
            XCTAssertTrue(
                visibleText.localizedCaseInsensitiveContains(fragment),
                "Expected visible Block text \(fragment.debugDescription), OCR saw \(visibleText.debugDescription)",
                file: file,
                line: line
            )
        }
    }
}
