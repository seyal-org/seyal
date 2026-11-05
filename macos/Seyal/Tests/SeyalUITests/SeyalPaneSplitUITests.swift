import AppKit
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

    /// #1424: Split Right keeps Block transcript + composer on both Flow panes.
    func testSplitProjectsBlocksAndComposerOnFocusedAndSecondaryPanes() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        XCTAssertTrue(element(app, "seyal-composer").waitForExistence(timeout: 8))
        XCTAssertTrue(element(app, "seyal-blocks").waitForExistence(timeout: 5))
        XCTAssertTrue(element(app, "seyal-blocks-scroll").waitForExistence(timeout: 5))

        click(app, "seyal-split-right")
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12))
        XCTAssertTrue(element(app, "seyal-secondary-live-pane").waitForExistence(timeout: 12))
        XCTAssertTrue(
            element(app, "seyal-pane-presentation-host").waitForExistence(timeout: 8),
            "focused Flow leaf keeps a durable presentation host"
        )
        XCTAssertTrue(element(app, "seyal-composer").exists, "focused Pane keeps seyal-composer")
        XCTAssertTrue(element(app, "seyal-blocks").exists, "focused Pane keeps Blocks")
        XCTAssertTrue(
            element(app, "seyal-secondary-blocks").waitForExistence(timeout: 8),
            "unfocused Flow pane must present Blocks, not Metal-only chrome"
        )
        XCTAssertTrue(element(app, "seyal-secondary-blocks-scroll").exists)
        let secondaryComposers = app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier BEGINSWITH %@", "seyal-composer-")
        )
        let appeared = expectation(
            for: NSPredicate(format: "count >= 1"),
            evaluatedWith: secondaryComposers,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [appeared], timeout: 8),
            .completed,
            "inactive Pane projects a subdued pane-scoped composer"
        )

        let other = (try focusedRegionIndex(app, count: 2)) == 0 ? 1 : 0
        element(app, "seyal-pane-region-\(other)").click()
        waitForFocus(app, region: other)
        XCTAssertTrue(element(app, "seyal-composer").waitForExistence(timeout: 5))
        XCTAssertTrue(element(app, "seyal-blocks").exists)
        XCTAssertTrue(element(app, "seyal-pane-presentation-host").waitForExistence(timeout: 5))
        XCTAssertTrue(element(app, "seyal-secondary-live-pane").waitForExistence(timeout: 5))
        XCTAssertTrue(element(app, "seyal-secondary-blocks").exists)

        let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        shot.name = "1424-dual-flow-blocks-composers"
        shot.lifetime = .keepAlways
        add(shot)
        XCTAssertEqual(app.state, .runningForeground)
    }

    /// A live Flow command must remain a Block when its pane becomes a
    /// secondary pane during a split. Repeated focus changes must keep the
    /// command and streamed output in that pane's transcript.
    func testRunningFlowBlockStaysInSecondaryPaneDuringFocusSwitches() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        if !element(app, "seyal-composer").waitForExistence(timeout: 2) {
            app.menuBars.menuBarItems["View"].click()
            app.menuItems["Toggle Raw"].click()
        }
        XCTAssertTrue(element(app, "seyal-composer").waitForExistence(timeout: 8))

        // Keep the twelve-line body under the bounded AX summary so every
        // streamed line remains observable to this assertion.
        let marker = "stream1426"
        submitComposerCommand(
            "for i in $(seq 1 12); do printf '\(marker) %02d\\n' \"$i\"; sleep 1; done",
            in: app,
            expectBlock: true,
            blockMarker: marker
        )
        let runningBlock = anyPaneBlock(in: app, containing: marker)
        XCTAssertTrue(runningBlock.waitForExistence(timeout: 5))

        click(app, "seyal-split-right")
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12))
        let secondaryBlock = app.descendants(matching: .any).matching(
            NSPredicate(
                format: "identifier BEGINSWITH %@ AND label CONTAINS %@",
                "seyal-secondary-block-",
                marker
            )
        ).firstMatch
        XCTAssertTrue(
            secondaryBlock.waitForExistence(timeout: 8),
            "the running source pane must keep its Block when split focus moves to the sibling"
        )

        for _ in 0..<3 {
            element(app, "seyal-pane-region-0").click()
            waitForFocus(app, region: 0)
            element(app, "seyal-pane-region-1").click()
            waitForFocus(app, region: 1)
            XCTAssertTrue(
                secondaryBlock.exists,
                "the inactive source pane must retain its Block after focus switches"
            )
        }
        let secondaryBody = app.descendants(matching: .any)[
            "\(secondaryBlock.identifier)-body"
        ].firstMatch
        XCTAssertTrue(
            secondaryBody.waitForExistence(timeout: 5),
            "the secondary Block must retain its pane-local output body"
        )
        for line in 1...12 {
            let expected = String(format: "%@ %02d", marker, line)
            XCTAssertTrue(
                secondaryBody.label.contains(expected),
                "the secondary Block retains streamed line \(line) after focus changes"
            )
        }
        XCTAssertEqual(app.state, .runningForeground)
    }

    /// #1426: splitting a Raw Pane creates a Blocks Pane when its own shell
    /// integration is trusted, and that Pane retains a numbered live stream
    /// when focus returns to its Raw sibling.
    func testRawSplitCreatesBlocksPaneAndKeepsUnfocusedStream() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        if element(app, "seyal-composer").waitForExistence(timeout: 2) {
            app.menuBars.menuBarItems["View"].click()
            app.menuItems["Toggle Raw"].click()
        }

        click(app, "seyal-split-right")
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12))
        waitForFocus(app, region: 1)
        waitForUsablePty(in: app)
        XCTAssertTrue(
            element(app, "seyal-composer").waitForExistence(timeout: 8),
            "a trusted split from Raw starts in Blocks"
        )
        XCTAssertTrue(element(app, "seyal-blocks").exists)

        let marker = "switchflow1426-\(String(UUID().uuidString.prefix(6)))"
        submitComposerCommand(
            "for i in $(seq 1 16); do printf '\(marker)-%02d\\n' \"$i\"; sleep 0.5; done",
            in: app,
            expectBlock: true,
            blockMarker: marker
        )
        let running = anyPaneBlock(in: app, containing: marker)
        XCTAssertTrue(running.waitForExistence(timeout: 6), "Flow command becomes a Block before exit")

        element(app, "seyal-pane-region-0").click()
        waitForFocus(app, region: 0)
        let inactiveBlock = app.descendants(matching: .any).matching(
            NSPredicate(
                format: "identifier BEGINSWITH %@ AND label CONTAINS %@",
                "seyal-secondary-block-",
                marker
            )
        ).firstMatch
        XCTAssertTrue(
            inactiveBlock.waitForExistence(timeout: 8),
            "an existing Pane's Flow selection must not become Raw when focus moves"
        )
        let body = blockBody(for: inactiveBlock, in: app)
        waitForBodyMarker(body, marker: "\(marker)-01")
        waitForBodyMarker(body, marker: "\(marker)-16")

        let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        shot.name = "1426-raw-split-blocks-remains-while-unfocused"
        shot.lifetime = .keepAlways
        add(shot)
        XCTAssertEqual(app.state, .runningForeground)
    }

    /// #1426: a sibling split from Flow stays Flow when focus moves away from
    /// it. Run a live numbered stream in that sibling, switch to the source
    /// Pane, and verify the secondary transcript retains every output line.
    func testFlowSplitSiblingStaysBlocksWhileUnfocusedDuringStream() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        if !element(app, "seyal-composer").waitForExistence(timeout: 2) {
            app.menuBars.menuBarItems["View"].click()
            app.menuItems["Toggle Raw"].click()
        }
        XCTAssertTrue(element(app, "seyal-composer").waitForExistence(timeout: 8))

        click(app, "seyal-split-right")
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12))
        waitForFocus(app, region: 1)
        XCTAssertTrue(
            element(app, "seyal-pane-presentation-host").waitForExistence(timeout: 8),
            "a pane split from Flow must inherit the Blocks presentation"
        )
        XCTAssertTrue(element(app, "seyal-composer").exists)
        XCTAssertTrue(element(app, "seyal-blocks-scroll").exists)

        let marker = "splitstream1426"
        submitComposerCommand(
            "for i in $(seq 1 24); do printf '\(marker) %02d\\n' \"$i\"; sleep 1; done; printf '\(marker) COMPLETE\\n'",
            in: app,
            expectBlock: true,
            blockMarker: marker
        )
        let secondaryPane = element(app, "seyal-pane-region-0")
        secondaryPane.click()
        waitForFocus(app, region: 0)

        let secondaryBlocks = element(app, "seyal-secondary-blocks")
        XCTAssertTrue(
            secondaryBlocks.waitForExistence(timeout: 8),
            "the inactive Flow sibling must remain a Block transcript, not Raw"
        )
        let secondaryBlock = app.descendants(matching: .any).matching(
            NSPredicate(
                format: "identifier BEGINSWITH %@ AND label CONTAINS %@",
                "seyal-secondary-block-",
                marker
            )
        ).firstMatch
        XCTAssertTrue(secondaryBlock.waitForExistence(timeout: 8))
        let secondaryBody = app.descendants(matching: .any)[
            "\(secondaryBlock.identifier)-body"
        ].firstMatch
        XCTAssertTrue(secondaryBody.waitForExistence(timeout: 8))
        let runningComposer = app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier BEGINSWITH %@ AND value == %@", "seyal-composer-", "busy")
        ).firstMatch
        XCTAssertTrue(
            runningComposer.waitForExistence(timeout: 5),
            "the inactive pane remains in Blocks while its command is still running"
        )
        let inactiveStreamScreenshot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        inactiveStreamScreenshot.name = "1426-inactive-flow-block-during-numbered-stream"
        inactiveStreamScreenshot.lifetime = .keepAlways
        add(inactiveStreamScreenshot)
        let finalLine = "\(marker) COMPLETE"
        let streamFinished = expectation(
            for: NSPredicate(format: "label CONTAINS %@ OR value CONTAINS %@", finalLine, finalLine),
            evaluatedWith: secondaryBody,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [streamFinished], timeout: 35),
            .completed,
            "the unfocused Flow Block receives the end of the stream"
        )
        for line in 1...24 {
            let expected = String(format: "%@ %02d", marker, line)
            let containsLine = expectation(
                for: NSPredicate(format: "label CONTAINS %@ OR value CONTAINS %@", expected, expected),
                evaluatedWith: secondaryBody,
                handler: nil
            )
            XCTAssertEqual(
                XCTWaiter.wait(for: [containsLine], timeout: 8),
                .completed,
                "the unfocused Flow Block retains streamed line \(line)"
            )
        }
        let completedInactiveScreenshot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        completedInactiveScreenshot.name = "1426-complete-flow-block-while-inactive"
        completedInactiveScreenshot.lifetime = .keepAlways
        add(completedInactiveScreenshot)

        element(app, "seyal-pane-region-1").click()
        waitForFocus(app, region: 1)
        let completedBlock = anyPaneBlock(in: app, containing: marker)
        XCTAssertTrue(completedBlock.waitForExistence(timeout: 8))
        let completedStreamScreenshot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        completedStreamScreenshot.name = "1426-flow-block-after-stream-and-focus-return"
        completedStreamScreenshot.lifetime = .keepAlways
        add(completedStreamScreenshot)
        XCTAssertEqual(app.state, .runningForeground)
    }

    /// A Pane's terminal tabs own separate Runtime timelines. Their local
    /// Block ids may match, so switching executions must not reuse a sibling
    /// tab's command card or history body.
    func testTerminalTabBlocksStayBoundToTheirOwnExecution() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        if !element(app, "seyal-composer").waitForExistence(timeout: 2) {
            app.menuBars.menuBarItems["View"].click()
            app.menuItems["Toggle Raw"].click()
        }
        XCTAssertTrue(element(app, "seyal-composer").waitForExistence(timeout: 8))

        let tokenA = "pane-a-\(String(UUID().uuidString.prefix(6)))"
        submitComposerCommand(
            "printf '\(tokenA)\\n'",
            in: app,
            expectBlock: true,
            blockMarker: tokenA
        )
        let blockA = anyPaneBlock(in: app, containing: tokenA)
        XCTAssertTrue(blockA.waitForExistence(timeout: 8))
        waitForBodyMarker(blockBody(for: blockA, in: app), marker: tokenA)

        click(app, "seyal-split-right")
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12))
        let paneBRegion = try focusedRegionIndex(app, count: 2)
        waitForUsablePty(in: app)

        let tokenB1 = "pane-b1-\(String(UUID().uuidString.prefix(6)))"
        submitComposerCommand(
            "printf '\(tokenB1)\\n'",
            in: app,
            expectBlock: true,
            blockMarker: tokenB1
        )
        let blockB1 = anyPaneBlock(in: app, containing: tokenB1)
        XCTAssertTrue(blockB1.waitForExistence(timeout: 8))
        waitForBodyMarker(blockBody(for: blockB1, in: app), marker: tokenB1)

        let paneRegion = element(app, "seyal-pane-region-\(paneBRegion)")
        let addButtons = app.buttons.matching(
            NSPredicate(format: "identifier CONTAINS %@", "-new-terminal")
        ).allElementsBoundByIndex
        guard let addTerminal = addButtons.first(where: { paneRegion.frame.contains($0.frame) }) else {
            XCTFail("Pane B must expose its pane-local New Terminal control")
            return
        }
        addTerminal.click()
        let tabTwo = app.buttons.matching(
            NSPredicate(format: "label == %@", "Pane \(paneBRegion + 1) · 2")
        ).firstMatch
        XCTAssertTrue(tabTwo.waitForExistence(timeout: 10), "Pane B's second terminal tab must exist")
        waitForUsablePty(in: app)

        let tokenB2 = "pane-b2-\(String(UUID().uuidString.prefix(6)))"
        submitComposerCommand(
            "printf '\(tokenB2)\\n'",
            in: app,
            expectBlock: true,
            blockMarker: tokenB2
        )
        let blockB2 = anyPaneBlock(in: app, containing: tokenB2)
        XCTAssertTrue(blockB2.waitForExistence(timeout: 8))
        waitForBodyMarker(blockBody(for: blockB2, in: app), marker: tokenB2)

        let tabOne = app.buttons.matching(
            NSPredicate(format: "label == %@", "Pane \(paneBRegion + 1) · 1")
        ).firstMatch
        XCTAssertTrue(tabOne.waitForExistence(timeout: 8))
        tabOne.click()

        let selectedB1 = element(app, "seyal-block-0")
        XCTAssertTrue(
            selectedB1.waitForExistence(timeout: 8),
            "selected terminal 1 must keep a Block card"
        )
        let titleUpdated = expectation(
            for: NSPredicate(format: "label CONTAINS %@", tokenB1),
            evaluatedWith: selectedB1,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [titleUpdated], timeout: 6),
            .completed,
            "selected terminal 1 must show its own command title; actual=\(selectedB1.label)"
        )
        XCTAssertFalse(selectedB1.label.contains(tokenB2), "terminal 1 command title must not bleed from terminal 2")
        let selectedBodyB1 = blockBody(for: selectedB1, in: app)
        waitForBodyMarker(selectedBodyB1, marker: tokenB1)
        XCTAssertFalse(
            "\(selectedBodyB1.label) \(selectedBodyB1.value ?? "")".contains(tokenB2),
            "terminal 1 history body must not contain terminal 2 output"
        )

        element(app, "seyal-pane-region-0").click()
        waitForFocus(app, region: 0)
        let inactiveA = anyPaneBlock(in: app, containing: tokenA)
        XCTAssertTrue(inactiveA.exists, "Pane A history stays visible when Pane B changes terminal tabs")
        waitForBodyMarker(blockBody(for: inactiveA, in: app), marker: tokenA)

        let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        shot.name = "1426-terminal-tab-block-isolation"
        shot.lifetime = .keepAlways
        add(shot)
        XCTAssertEqual(app.state, .runningForeground)
    }

    /// #1424 scenario 2: focus churn keeps host/Block content and scroll mid-transcript.
    func testFocusChurnRetainsScrolledTranscriptAndPaneHosts() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        // Split first so region projection is established before tall transcripts.
        click(app, "seyal-split-right")
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12))
        waitForUsablePty(in: app)
        XCTAssertTrue(element(app, "seyal-secondary-blocks-scroll").waitForExistence(timeout: 8))
        XCTAssertTrue(element(app, "seyal-pane-presentation-host").waitForExistence(timeout: 8))

        let focusedIdx = try focusedRegionIndex(app, count: 2)
        submitComposerCommand(
            "printf '%s\\n' $(seq 1 24) 'seyal-1424-scroll-a'",
            in: app
        )
        let other = focusedIdx == 0 ? 1 : 0
        // Seed the newly focused sibling with its own tall transcript.
        element(app, "seyal-pane-region-\(other)").click()
        waitForFocus(app, region: other)
        waitForUsablePty(in: app)
        submitComposerCommand(
            "printf '%s\\n' $(seq 1 24) 'seyal-1424-scroll-b'",
            in: app
        )

        let primaryScroll = element(app, "seyal-blocks-scroll")
        let secondaryScroll = element(app, "seyal-secondary-blocks-scroll")
        XCTAssertTrue(primaryScroll.exists)
        XCTAssertTrue(secondaryScroll.exists)
        // Scroll the focused transcript away from live end.
        primaryScroll.scroll(byDeltaX: 0, deltaY: -420)
        let primaryOriginBefore = primaryScroll.coordinate(
            withNormalizedOffset: CGVector(dx: 0.5, dy: 0.2)
        ).screenPoint
        let secondaryFrameBefore = secondaryScroll.frame
        let blockA = anyPaneBlock(in: app, containing: "seyal-1424-scroll-a")
        let blockB = anyPaneBlock(in: app, containing: "seyal-1424-scroll-b")
        XCTAssertTrue(blockA.exists || blockB.exists)

        for cycle in 0..<4 {
            let target = cycle % 2
            element(app, "seyal-pane-region-\(target)").click()
            waitForFocus(app, region: target)
            XCTAssertTrue(element(app, "seyal-pane-presentation-host").exists)
            XCTAssertTrue(element(app, "seyal-secondary-live-pane").exists)
            XCTAssertTrue(element(app, "seyal-composer").exists)
            XCTAssertTrue(element(app, "seyal-blocks").exists)
            XCTAssertTrue(element(app, "seyal-secondary-blocks").exists)
            XCTAssertTrue(element(app, "seyal-secondary-blocks-scroll").exists)
            XCTAssertTrue(
                anyPaneBlock(in: app, containing: "seyal-1424-scroll-a").exists
                    || anyPaneBlock(in: app, containing: "seyal-1424-scroll-b").exists,
                "focus cycle \(cycle) must keep pane Block content"
            )
        }

        // Mid-transcript scroll must not jump back to live end after focus churn.
        let primaryOriginAfter = primaryScroll.coordinate(
            withNormalizedOffset: CGVector(dx: 0.5, dy: 0.2)
        ).screenPoint
        XCTAssertEqual(
            primaryOriginAfter.y,
            primaryOriginBefore.y,
            accuracy: 48,
            "focused transcript scroll position must survive focus ownership churn"
        )
        XCTAssertEqual(
            secondaryScroll.frame.minY,
            secondaryFrameBefore.minY,
            accuracy: 8,
            "secondary transcript host frame must stay stable"
        )
        let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        shot.name = "1424-focus-churn-scroll-retained"
        shot.lifetime = .keepAlways
        add(shot)
        XCTAssertEqual(app.state, .runningForeground)
    }

    /// #1424 scenario 3: unfocused pane keeps updating the same Block while the sibling is used.
    func testUnfocusedPaneStreamKeepsUpdatingSameBlock() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        if !element(app, "seyal-composer").waitForExistence(timeout: 2) {
            app.menuBars.menuBarItems["View"].click()
            app.menuItems["Toggle Raw"].click()
        }
        XCTAssertTrue(element(app, "seyal-composer").waitForExistence(timeout: 8))
        let token = "stream-marker-\(String(UUID().uuidString.prefix(6)))"
        click(app, "seyal-split-right")
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12))
        waitForUsablePty(in: app)
        // Start the stream on the focused pane, then move focus to the sibling.
        // Long stream so late lines arrive after we move focus to the sibling.
        submitComposerCommand(
            "for i in $(seq 1 40); do printf '%s-%s\\n' '\(token)' \"$i\"; sleep 0.15; done",
            in: app,
            expectBlock: true,
            blockMarker: token
        )
        let streamBlock = anyPaneBlock(in: app, containing: token)
        XCTAssertTrue(streamBlock.waitForExistence(timeout: 12), "stream Block must appear")
        let streamBody = blockBody(for: streamBlock, in: app)
        XCTAssertTrue(streamBody.waitForExistence(timeout: 8))
        let heightBefore = streamBody.frame.height
        let lateMarker = "\(token)-35"

        let focused = try focusedRegionIndex(app, count: 2)
        // Prefer focusing the non-streaming pane so the stream stays visible but unfocused.
        let sibling = focused == 0 ? 1 : 0
        element(app, "seyal-pane-region-\(sibling)").click()
        waitForFocus(app, region: sibling)
        waitForUsablePty(in: app)
        submitComposerCommand("printf 'seyal-1424-sibling\\n'", in: app)
        XCTAssertTrue(
            anyPaneBlock(in: app, containing: "seyal-1424-sibling").waitForExistence(timeout: 10)
        )

        let unfocusedStream = anyPaneBlock(in: app, containing: token)
        XCTAssertTrue(unfocusedStream.exists, "unfocused stream Block must remain after focus move")
        let unfocusedBody = blockBody(for: unfocusedStream, in: app)
        let progressed = expectation(
            for: NSPredicate { _, _ in
                guard unfocusedBody.exists else { return false }
                if unfocusedBody.frame.height > heightBefore + 8 { return true }
                let label = (unfocusedBody.label as String?) ?? ""
                let value = (unfocusedBody.value as? String) ?? ""
                return label.contains(lateMarker) || value.contains(lateMarker)
            },
            evaluatedWith: nil,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [progressed], timeout: 12),
            .completed,
            "unfocused stream must keep updating the same Block; before=\(heightBefore) after=\(unfocusedBody.frame.height) label=\(unfocusedBody.label) value=\(unfocusedBody.value ?? "nil")"
        )
        XCTAssertTrue(
            unfocusedStream.identifier.hasPrefix("seyal-secondary-block-")
                || unfocusedStream.identifier.hasPrefix("seyal-block-"),
            "stream must stay on a Block surface, id=\(unfocusedStream.identifier)"
        )
        let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        shot.name = "1424-unfocused-stream-same-block"
        shot.lifetime = .keepAlways
        add(shot)
        XCTAssertEqual(app.state, .runningForeground)
    }

    /// #1426: completed Block history stays on the producing pane after focus moves.
    /// Handle-addressed history FFI must not blank or swap bodies via the focused display.
    func testCompletedBlockHistoryStaysOnProducingUnfocusedPane() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        // New workspaces now start in Raw. This acceptance covers per-Pane
        // completed Blocks, so select Flow through the product View menu.
        let viewMenu = app.menuBars.menuBarItems["View"]
        XCTAssertTrue(viewMenu.waitForExistence(timeout: 5))
        viewMenu.click()
        let toggleRaw = app.menuItems["Toggle Raw"]
        XCTAssertTrue(toggleRaw.waitForExistence(timeout: 5), app.debugDescription)
        toggleRaw.click()
        XCTAssertTrue(element(app, "seyal-composer").waitForExistence(timeout: 12))
        let markerA = "seyal-1426-hist-a-\(String(UUID().uuidString.prefix(6)))"
        let markerB = "seyal-1426-hist-b-\(String(UUID().uuidString.prefix(6)))"
        click(app, "seyal-split-right")
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12))
        waitForUsablePty(in: app)
        let paneARegion = try focusedRegionIndex(app, count: 2)

        submitComposerCommand(
            "printf '\(markerA)\\n'",
            in: app,
            expectBlock: true,
            blockMarker: markerA
        )
        XCTAssertTrue(
            anyPaneBlock(in: app, containing: markerA).waitForExistence(timeout: 12),
            "pane A completed Block must appear"
        )
        let bodyA = blockBody(for: anyPaneBlock(in: app, containing: markerA), in: app)
        XCTAssertTrue(bodyA.waitForExistence(timeout: 8))
        let heightA = bodyA.frame.height

        let sibling = paneARegion == 0 ? 1 : 0
        element(app, "seyal-pane-region-\(sibling)").click()
        waitForFocus(app, region: sibling)
        waitForUsablePty(in: app)
        submitComposerCommand(
            "printf '\(markerB)\\n'",
            in: app,
            expectBlock: true,
            blockMarker: markerB
        )
        XCTAssertTrue(
            anyPaneBlock(in: app, containing: markerB).waitForExistence(timeout: 12),
            "pane B completed Block must appear"
        )

        let bodyB = blockBody(for: anyPaneBlock(in: app, containing: markerB), in: app)
        XCTAssertTrue(bodyB.waitForExistence(timeout: 8), "pane B completed body must appear")
        waitForBodyMarker(bodyA, marker: markerA)
        waitForBodyMarker(bodyB, marker: markerB)

        let unfocusedA = app.descendants(matching: .any).matching(
            NSPredicate(
                format: "identifier BEGINSWITH %@ AND label CONTAINS %@",
                "seyal-secondary-block-",
                markerA
            )
        ).firstMatch
        XCTAssertTrue(
            unfocusedA.waitForExistence(timeout: 8),
            "pane A's inactive transcript must remain a visible Flow Block host"
        )
        let unfocusedBodyA = blockBody(for: unfocusedA, in: app)
        XCTAssertTrue(unfocusedBodyA.exists, "pane A Block body must stay bound")
        XCTAssertGreaterThan(
            unfocusedBodyA.frame.height,
            0,
            "pane A history body must not blank via focused-handle routing"
        )
        XCTAssertEqual(
            unfocusedBodyA.frame.height,
            heightA,
            accuracy: 12,
            "pane A completed body height must stay stable after sibling history bind"
        )
        XCTAssertTrue(
            anyPaneBlock(in: app, containing: markerB).exists,
            "pane B Block must not steal or erase pane A history"
        )

        // Exercise the actual focus boundary repeatedly. The focused leaf is
        // also hosted by SecondaryLivePaneHost in multipane Flow; both
        // completed bodies must stay in their producing pane as each leaf
        // alternates between focused and unfocused ownership.
        for cycle in 0..<10 {
            let target = cycle % 2 == 0 ? paneARegion : sibling
            element(app, "seyal-pane-region-\(target)").click()
            waitForFocus(app, region: target)
            let paneAPrefix = target == paneARegion ? "seyal-block-" : "seyal-secondary-block-"
            let paneBPrefix = target == sibling ? "seyal-block-" : "seyal-secondary-block-"
            let blockA = app.descendants(matching: .any).matching(
                NSPredicate(format: "identifier BEGINSWITH %@ AND label CONTAINS %@", paneAPrefix, markerA)
            ).firstMatch
            let blockB = app.descendants(matching: .any).matching(
                NSPredicate(format: "identifier BEGINSWITH %@ AND label CONTAINS %@", paneBPrefix, markerB)
            ).firstMatch
            XCTAssertTrue(
                blockA.waitForExistence(timeout: 8),
                "cycle \(cycle): pane A command stays on its own Flow transcript"
            )
            XCTAssertTrue(
                blockB.waitForExistence(timeout: 8),
                "cycle \(cycle): pane B command stays on its own Flow transcript"
            )
            let currentBodyA = blockBody(for: blockA, in: app)
            let currentBodyB = blockBody(for: blockB, in: app)
            waitForBodyMarker(currentBodyA, marker: markerA)
            waitForBodyMarker(currentBodyB, marker: markerB)
            let textA = "\(currentBodyA.label) \(currentBodyA.value ?? "")"
            let textB = "\(currentBodyB.label) \(currentBodyB.value ?? "")"
            XCTAssertFalse(textA.contains(markerB), "cycle \(cycle): pane A never shows pane B output")
            XCTAssertFalse(textB.contains(markerA), "cycle \(cycle): pane B never shows pane A output")
        }

        let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        shot.name = "1426-completed-history-on-producing-pane"
        shot.lifetime = .keepAlways
        add(shot)
        XCTAssertEqual(app.state, .runningForeground)
    }

    /// #1424 scenario 4: divider drag keeps dual Flow Blocks+composers.
    func testDividerDragKeepsDualFlowBlocksAndComposers() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        click(app, "seyal-split-right")
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12))
        waitForUsablePty(in: app)
        let focused = try focusedRegionIndex(app, count: 2)
        submitComposerCommand("printf 'seyal-1424-div-a\\n'", in: app)
        let other = focused == 0 ? 1 : 0
        element(app, "seyal-pane-region-\(other)").click()
        waitForFocus(app, region: other)
        waitForUsablePty(in: app)
        submitComposerCommand("printf 'seyal-1424-div-b\\n'", in: app)

        let divider = element(app, "seyal-pane-divider-0")
        XCTAssertTrue(divider.waitForExistence(timeout: 8))
        let leading = element(app, "seyal-pane-region-0")
        let trailing = element(app, "seyal-pane-region-1")
        let area = leading.frame.union(trailing.frame)
        let start = divider.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
        let end = start.withOffset(CGVector(dx: area.minX + area.width * 0.35 - divider.frame.midX, dy: 0))
        start.press(forDuration: 0.2, thenDragTo: end, withVelocity: 300, thenHoldForDuration: 0.3)

        XCTAssertTrue(element(app, "seyal-composer").waitForExistence(timeout: 5))
        XCTAssertTrue(element(app, "seyal-blocks").exists)
        XCTAssertTrue(element(app, "seyal-secondary-blocks").exists)
        XCTAssertTrue(element(app, "seyal-secondary-blocks-scroll").exists)
        XCTAssertFalse(
            app.descendants(matching: .any).matching(
                NSPredicate(format: "identifier BEGINSWITH %@", "seyal-composer-")
            ).allElementsBoundByIndex.isEmpty
        )
        XCTAssertTrue(anyPaneBlock(in: app, containing: "seyal-1424-div-a").exists)
        XCTAssertTrue(anyPaneBlock(in: app, containing: "seyal-1424-div-b").exists)
        let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        shot.name = "1424-divider-dual-flow-retained"
        shot.lifetime = .keepAlways
        add(shot)
        XCTAssertEqual(app.state, .runningForeground)
    }

    /// #1424 scenario 5: disposable vim TUI enter/exit restores Blocks+composer.
    func testVimTuiEnterExitRestoresFlowBlocksAndComposer() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        XCTAssertTrue(element(app, "seyal-composer").waitForExistence(timeout: 8))
        // TUI entry can fence Flow Blocks immediately; do not require a Block card.
        submitComposerCommand("vim -Nu NONE -n", in: app, expectBlock: false)

        let composer = element(app, "seyal-composer")
        let terminal = app.descendants(matching: .any)["terminal-input"].firstMatch
        // TUI should take the live surface; composer becomes non-hittable / Hidden.
        let tuiArrived = expectation(
            for: NSPredicate(format: "isHittable == false"),
            evaluatedWith: composer,
            handler: nil
        )
        let tuiResult = XCTWaiter.wait(for: [tuiArrived], timeout: 12)
        if tuiResult != .completed {
            // Some shells surface vim under Raw with a still-visible composer;
            // require a usable live terminal either way.
            XCTAssertTrue(terminal.waitForExistence(timeout: 5), "vim must attach a live surface")
        }
        let tuiShot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        tuiShot.name = "1424-vim-tui-active"
        tuiShot.lifetime = .keepAlways
        add(tuiShot)

        if terminal.exists, terminal.isHittable {
            terminal.click()
        }
        // Quit vim: Escape then :q!
        app.typeKey(.escape, modifierFlags: [])
        RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        app.typeText(":q!")
        app.typeKey("\r", modifierFlags: [])

        let restored = expectation(
            for: NSPredicate(format: "isHittable == true"),
            evaluatedWith: composer,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [restored], timeout: 15),
            .completed,
            "leaving vim must restore hittable Flow composer"
        )
        XCTAssertTrue(element(app, "seyal-blocks").waitForExistence(timeout: 8))
        XCTAssertTrue(element(app, "seyal-blocks-scroll").exists)
        let editor = element(app, "seyal-composer-editor")
        XCTAssertTrue(editor.waitForExistence(timeout: 5))
        editor.click()
        let token = "after-vim-marker-1424"
        editor.typeText(token)
        XCTAssertTrue(
            NSPredicate(format: "value CONTAINS %@", token).evaluate(with: editor),
            "post-vim keys must enter the composer"
        )
        let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        shot.name = "1424-vim-tui-restored-flow"
        shot.lifetime = .keepAlways
        add(shot)
        XCTAssertEqual(app.state, .runningForeground)
    }

    /// #1424 scenario 6: resize/reflow keeps dual Flow chrome without blank-canvas collapse.
    func testResizeReflowKeepsDualFlowHostsWithoutBlankCanvas() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        click(app, "seyal-split-right")
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12))
        waitForUsablePty(in: app)
        XCTAssertTrue(element(app, "seyal-secondary-blocks").waitForExistence(timeout: 8))
        submitComposerCommand("printf '%s\\n' $(seq 1 12) 'seyal-1424-resize'", in: app)

        let window = app.windows.firstMatch
        XCTAssertTrue(window.waitForExistence(timeout: 5))
        let original = window.frame
        let origin = window.coordinate(withNormalizedOffset: CGVector(dx: 1, dy: 1))
        origin.click()
        origin.press(
            forDuration: 0.05,
            thenDragTo: origin.withOffset(CGVector(dx: -160, dy: -90))
        )
        RunLoop.current.run(until: Date().addingTimeInterval(0.4))
        origin.press(
            forDuration: 0.05,
            thenDragTo: origin.withOffset(CGVector(dx: 120, dy: 60))
        )
        RunLoop.current.run(until: Date().addingTimeInterval(0.4))

        let focused = try focusedRegionIndex(app, count: 2)
        element(app, "seyal-pane-region-\(focused)").click()
        waitForUsablePty(in: app)
        submitComposerCommand("printf 'seyal-1424-reflow\\n'", in: app)

        XCTAssertTrue(element(app, "seyal-composer").exists)
        XCTAssertTrue(element(app, "seyal-blocks").exists)
        XCTAssertTrue(element(app, "seyal-secondary-blocks").exists)
        XCTAssertTrue(element(app, "seyal-secondary-blocks-scroll").exists)
        XCTAssertTrue(element(app, "seyal-pane-presentation-host").exists)
        XCTAssertTrue(element(app, "seyal-secondary-live-pane").exists)
        XCTAssertTrue(anyPaneBlock(in: app, containing: "seyal-1424-resize").exists)
        XCTAssertTrue(anyPaneBlock(in: app, containing: "seyal-1424-reflow").exists)
        let secondaryScroll = element(app, "seyal-secondary-blocks-scroll")
        XCTAssertLessThan(
            secondaryScroll.frame.height,
            max(original.height, window.frame.height),
            "secondary scroll clip stays inside the window"
        )
        let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        shot.name = "1424-resize-reflow-dual-flow"
        shot.lifetime = .keepAlways
        add(shot)
        XCTAssertEqual(app.state, .runningForeground)
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
        created.click()
        XCTAssertEqual(created.value as? String, "selected")
        let createdExecution = try XCTUnwrap(executionIdentity(terminalSurface.value as? String))
        XCTAssertNotEqual(createdExecution, originalExecution, "the selected terminal surface follows the selected tab")
        original.click()
        XCTAssertEqual(
            try XCTUnwrap(executionIdentity(terminalSurface.value as? String)),
            originalExecution,
            "switching back restores the original execution without replacing it"
        )

        // Split while the AX tree is still light (before Block markers). Seeding
        // Blocks first stalled multipane region queries on CI (#1424).
        click(app, "seyal-split-right")
        XCTAssertTrue(
            element(app, "seyal-pane-region-1").waitForExistence(timeout: 12),
            "Split Right projects a second Pane after pane-local tabs"
        )
        let secondPaneStrips = app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier CONTAINS %@ AND identifier CONTAINS %@", "-terminal-tabs", "seyal-pane-")
        )
        let stripsReady = expectation(
            for: NSPredicate(format: "count == 2"),
            evaluatedWith: secondPaneStrips,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [stripsReady], timeout: 12),
            .completed,
            "each Pane owns its own terminal-tab strip"
        )
        waitForFocus(app, region: 1)
        waitForUsablePty(in: app)
        let focusedHost = element(app, "seyal-pane-presentation-host")
        XCTAssertTrue(focusedHost.waitForExistence(timeout: 8))
        let secondPaneExecution = try XCTUnwrap(executionIdentity(focusedHost.value as? String))
        XCTAssertNotEqual(secondPaneExecution, originalExecution, "Split creates an independent terminal execution")
        XCTAssertNotEqual(secondPaneExecution, createdExecution, "split Pane does not reuse either terminal tab execution")
        XCTAssertEqual(terminalTabs.count, 3, "two terminal tabs stay in Pane 1 and Pane 2 starts with one")
        submitComposerCommand("echo SEYAL_PANE_TWO_MARKER", in: app)

        element(app, "seyal-pane-region-0").click()
        waitForFocus(app, region: 0)
        waitForUsablePty(in: app)
        let pane0Host = element(app, "seyal-pane-presentation-host")
        XCTAssertTrue(pane0Host.waitForExistence(timeout: 8))
        // Pane 1 still has the selected created tab from before the split.
        XCTAssertEqual(
            try XCTUnwrap(executionIdentity(pane0Host.value as? String)),
            createdExecution,
            "returning to Pane 1 preserves its selected terminal"
        )
        original.click()
        waitForUsablePty(in: app)
        submitComposerCommand("echo SEYAL_TAB_ONE_MARKER", in: app)
        created.click()
        waitForUsablePty(in: app)
        submitComposerCommand("echo SEYAL_TAB_TWO_MARKER", in: app)
        original.click()
        XCTAssertTrue(blockCommand(in: app, containing: "SEYAL_TAB_ONE_MARKER").exists)
        XCTAssertFalse(blockCommand(in: app, containing: "SEYAL_TAB_TWO_MARKER").exists)
        created.click()
        XCTAssertTrue(blockCommand(in: app, containing: "SEYAL_TAB_TWO_MARKER").exists)
        XCTAssertFalse(blockCommand(in: app, containing: "SEYAL_TAB_ONE_MARKER").exists)
        XCTAssertFalse(
            anyPaneBlock(in: app, containing: "SEYAL_PANE_TWO_MARKER").exists,
            "Pane 2 command history stays out of Pane 1"
        )

        element(app, "seyal-pane-region-1").click()
        waitForFocus(app, region: 1)
        let pane1Host = element(app, "seyal-pane-presentation-host")
        XCTAssertTrue(pane1Host.waitForExistence(timeout: 8))
        XCTAssertEqual(
            try XCTUnwrap(executionIdentity(pane1Host.value as? String)),
            secondPaneExecution,
            "returning to Pane 2 restores its independent execution"
        )
        XCTAssertTrue(anyPaneBlock(in: app, containing: "SEYAL_PANE_TWO_MARKER").exists)

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

    /// Focused Flow presentation sits inside the focused region. Multipane
    /// Flow (#1424) owns chrome through durable pane hosts; Raw/TUI may still
    /// expose a single `terminal-input` inside liveContent.
    private func assertSingleLiveSurface(in app: XCUIApplication, focusedRegion index: Int) {
        let region = element(app, "seyal-pane-region-\(index)").frame
        let host = element(app, "seyal-pane-presentation-host")
        if host.waitForExistence(timeout: 4) {
            XCTAssertTrue(
                region.insetBy(dx: -1, dy: -1).contains(host.frame),
                "presentation host \(host.frame) must sit inside focused region \(region)"
            )
            return
        }
        let terminals = app.descendants(matching: .any).matching(identifier: "terminal-input")
        let live = expectation(for: NSPredicate(format: "count == 1"), evaluatedWith: terminals, handler: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [live], timeout: 12), .completed, "exactly one live terminal surface")
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
        // Multipane Flow hides liveContent/`terminal-input`; readiness is the
        // focused presentation host + composer (#1424).
        let presentation = element(app, "seyal-pane-presentation-host")
        if presentation.waitForExistence(timeout: 2) {
            XCTAssertTrue(
                element(app, "seyal-composer").waitForExistence(timeout: timeout),
                "focused Flow presentation host must expose a composer"
            )
            XCTAssertEqual(app.state, .runningForeground, "Seyal.app crashed while waiting for multipane Flow")
            return
        }
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

    private func submitComposerCommand(
        _ command: String,
        in app: XCUIApplication,
        expectBlock: Bool = true,
        blockMarker: String? = nil
    ) {
        // Every pane's editor keeps the stable child ID for host reuse. Scope
        // lookup through the focused composer so a duplicate inactive editor
        // cannot steal synthetic input after a Pane focus change.
        let focusedComposer = element(app, "seyal-composer")
        XCTAssertTrue(focusedComposer.waitForExistence(timeout: 8), "focused composer missing")
        let editor = focusedComposer.descendants(matching: .any)
            .matching(identifier: "seyal-composer-editor")
            .firstMatch
        XCTAssertTrue(editor.waitForExistence(timeout: 8), "composer editor missing")
        editor.click()
        // Prefer paste: XCUI typeText mangles quotes/`$()`/paths and only the
        // first character (`/`) may land in the draft under multipane Flow.
        pasteIntoComposerEditor(editor, text: command)
        // Paste can clear keyboard focus; re-click so Return reaches the editor.
        editor.click()
        let draftMatched = expectation(
            for: NSPredicate(format: "value CONTAINS %@", String(command.prefix(12))),
            evaluatedWith: editor,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [draftMatched], timeout: 5),
            .completed,
            "composer draft missing after paste; value=\(editor.value ?? "nil")"
        )
        let submit = focusedComposer.descendants(matching: .any)
            .matching(identifier: "seyal-composer-execute")
            .firstMatch
        let enabled = expectation(for: NSPredicate(format: "isEnabled == true"), evaluatedWith: submit, handler: nil)
        if XCTWaiter.wait(for: [enabled], timeout: 4) == .completed {
            submit.click()
        } else {
            // submit() pushDraft()s first; Return works even when AX still shows
            // execute disabled after a synthetic paste.
            editor.typeKey("\r", modifierFlags: [])
        }
        guard expectBlock else { return }
        let marker = blockMarker ?? blockMarkerToken(in: command) ?? command
        let block = anyPaneBlock(in: app, containing: marker)
        XCTAssertTrue(
            block.waitForExistence(timeout: 10),
            "the selected execution records its own submitted command; marker=\(marker)"
        )
    }

    /// Paste avoids XCUI key synthesis gaps for shell metacharacters.
    private func pasteIntoComposerEditor(_ editor: XCUIElement, text: String) {
        let previous = NSPasteboard.general.string(forType: .string)
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
        // Ensure first-responder before synthetic Cmd-A/V; paste can clear focus.
        editor.click()
        editor.typeKey("a", modifierFlags: [.command])
        editor.typeKey("v", modifierFlags: [.command])
        editor.click()
        let needle = String(text.prefix(12))
        let pasted = NSPredicate(format: "value CONTAINS %@", needle).evaluate(with: editor)
        if !pasted {
            // Retry once — XCUI paste can miss under multipane focus churn.
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(text, forType: .string)
            editor.click()
            editor.typeKey("a", modifierFlags: [.command])
            editor.typeKey("v", modifierFlags: [.command])
            editor.click()
        }
        if let previous {
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(previous, forType: .string)
        }
    }

    /// Prefer a stable marker embedded in the command over the full shell line.
    private func blockMarkerToken(in command: String) -> String? {
        let patterns = [
            #"seyal-1424-[A-Za-z0-9_-]+"#,
            #"SEYAL_[A-Z0-9_]+"#,
            #"stream-marker-[A-Za-z0-9_-]+"#,
            #"after-vim-marker-[A-Za-z0-9_-]+"#,
        ]
        for pattern in patterns {
            if let match = command.range(of: pattern, options: .regularExpression) {
                return String(command[match])
            }
        }
        return nil
    }

    private func blockCommand(in app: XCUIApplication, containing command: String) -> XCUIElement {
        app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier BEGINSWITH %@ AND label CONTAINS %@", "seyal-block-", command)
        ).firstMatch
    }

    private func anyPaneBlock(in app: XCUIApplication, containing command: String) -> XCUIElement {
        app.descendants(matching: .any).matching(
            NSPredicate(
                format: "(identifier BEGINSWITH %@ OR identifier BEGINSWITH %@) AND label CONTAINS %@",
                "seyal-block-",
                "seyal-secondary-block-",
                command
            )
        ).firstMatch
    }

    private func blockBody(for block: XCUIElement, in app: XCUIApplication) -> XCUIElement {
        let bodyId = "\(block.identifier)-body"
        let body = app.descendants(matching: .any)[bodyId].firstMatch
        if body.exists { return body }
        // Fallback: any body descendant under the same pane Block stack.
        return block.descendants(matching: .any).matching(
            NSPredicate(format: "identifier ENDSWITH %@", "-body")
        ).firstMatch
    }

    private func waitForBodyMarker(_ body: XCUIElement, marker: String) {
        let rendered = expectation(
            for: NSPredicate(format: "label CONTAINS %@ OR value CONTAINS %@", marker, marker),
            evaluatedWith: body,
            handler: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [rendered], timeout: 8),
            .completed,
            "completed Block body visibly contains its output marker: \(marker)"
        )
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
