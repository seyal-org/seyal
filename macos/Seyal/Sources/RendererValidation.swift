import Foundation
import AppKit
import Darwin
import Metal
@preconcurrency import QuartzCore

/// CAMetalDisplayLink invokes its witness on the main run loop without a Swift
/// MainActor task. Keep this driver off MainActor isolation so the hop closure
/// can capture state without the compiler inserting `assumeIsolated` (which
/// Trace/BPTs under Xcode 16.4 Release `--renderer-benchmark`).
private final class DisplayLinkBenchmarkDriver: NSObject, CAMetalDisplayLinkDelegate, @unchecked Sendable {
    private let renderer: MetalTerminalRenderer
    private let link: CAMetalDisplayLink
    private var startedAt: UInt64?
    private(set) var samples = [UInt64]()

    @MainActor
    init(renderer: MetalTerminalRenderer, layer: CAMetalLayer) {
        self.renderer = renderer
        link = CAMetalDisplayLink(metalLayer: layer)
        super.init()
        link.delegate = self
        link.isPaused = true
        link.add(to: .main, forMode: .common)
    }

    @MainActor
    func submitOne() -> Bool {
        guard startedAt == nil else { return false }
        renderer.requestPresent()
        startedAt = DispatchTime.now().uptimeNanoseconds
        link.isPaused = false
        let sampleCount = samples.count
        let deadline = Date().addingTimeInterval(2)
        while samples.count == sampleCount, Date() < deadline {
            RunLoop.current.run(until: Date().addingTimeInterval(0.001))
        }
        let receivedSample = samples.count == sampleCount + 1
        if !receivedSample {
            // A headless macOS runner may have no WindowServer/display session,
            // so no CAMetalDisplayLink opportunity can arrive. Do not leave a
            // stale in-flight request blocking later benchmark iterations.
            startedAt = nil
        }
        return receivedSample
    }

    nonisolated func metalDisplayLink(
        _ link: CAMetalDisplayLink,
        needsUpdate update: CAMetalDisplayLink.Update
    ) {
        // Renderer present/drain are main-queue / non-MainActor. Call them
        // directly from the display-link run-loop callback — no MainActor hop.
        dispatchPrecondition(condition: .onQueue(.main))
        let driver = self
        let drawable = update.drawable
        link.isPaused = true
        driver.renderer.drainGPUCompletionsIfNeeded()
        guard let startedAt = driver.startedAt,
              driver.renderer.present(drawable: drawable)
        else {
            driver.startedAt = nil
            return
        }
        driver.samples.append(DispatchTime.now().uptimeNanoseconds - startedAt)
        driver.startedAt = nil
    }

    @MainActor
    func invalidate() {
        link.delegate = nil
        link.invalidate()
    }
}

@MainActor
enum RendererValidation {
    static func deterministicSelfTest() -> Bool {
        guard let device = MTLCreateSystemDefaultDevice() else { return false }
        do {
            let renderer = try MetalTerminalRenderer(device: device)
            var cells = [
                preparedCell(background: terminalRGB(red: 255, green: 0, blue: 0)),
                preparedCell(background: terminalRGB(red: 0, green: 255, blue: 0)),
                preparedCell(background: terminalRGB(red: 0, green: 0, blue: 255)),
                preparedCell(background: terminalRGB(red: 255, green: 255, blue: 0)),
            ]
            var damage = DamageMask()
            damage.markAll(rows: 2)
            let orientationPassed = try cells.withUnsafeBufferPointer { buffer -> Bool in
                let frame = NativePreparedFrame(
                    cells: buffer,
                    generation: 1,
                    rows: 2,
                    columns: 2,
                    damage: damage
                )
                guard try renderer.update(
                    frame: frame,
                    backingScale: 1,
                    forceFullRebuild: true
                ) == .updated else {
                    return false
                }
                let cellSize = renderer.cellPixelSize(backingScale: 1)
                guard let texture = renderer.renderOffscreenAndWait(
                    width: cellSize.width * 2,
                    height: cellSize.height * 2
                ) else {
                    return false
                }
                return pixelMatches(
                    texture,
                    x: cellSize.width / 2,
                    y: cellSize.height / 2,
                    red: 255,
                    green: 0,
                    blue: 0
                ) && pixelMatches(
                    texture,
                    x: cellSize.width + cellSize.width / 2,
                    y: cellSize.height / 2,
                    red: 0,
                    green: 255,
                    blue: 0
                ) && pixelMatches(
                    texture,
                    x: cellSize.width / 2,
                    y: cellSize.height + cellSize.height / 2,
                    red: 0,
                    green: 0,
                    blue: 255
                ) && pixelMatches(
                    texture,
                    x: cellSize.width + cellSize.width / 2,
                    y: cellSize.height + cellSize.height / 2,
                    red: 255,
                    green: 255,
                    blue: 0
                )
            }
            guard orientationPassed else { return false }

            cells = [
                preparedCell(
                    scalar: UInt32(ascii: "A"),
                    foreground: terminalRGB(red: 240, green: 240, blue: 240)
                ),
                preparedCell(
                    scalar: UInt32(ascii: "A"),
                    foreground: terminalRGB(red: 255, green: 0, blue: 0)
                ),
            ]
            let beforeGlyphs = renderer.glyphStats
            var oneRow = DamageMask()
            oneRow.mark(row: 0)
            let glyphPassed = try cells.withUnsafeBufferPointer { buffer -> Bool in
                let frame = NativePreparedFrame(
                    cells: buffer,
                    generation: 2,
                    rows: 1,
                    columns: 2,
                    damage: oneRow
                )
                guard try renderer.update(
                    frame: frame,
                    backingScale: 1,
                    forceFullRebuild: true
                ) == .updated else {
                    return false
                }
                let after = renderer.glyphStats
                return after.uploads > beforeGlyphs.uploads && after.hits > beforeGlyphs.hits
            }
            guard glyphPassed, renderer.hasDedicatedSurfaceResources else { return false }

            // Bold is a renderer/cache identity seam in M001. It must be able to
            // resolve different raster pixels without making terminal color part
            // of glyph identity.
            let beforeBold = renderer.glyphStats
            cells = [preparedCell(scalar: UInt32(ascii: "A"), flags: 1)]
            guard try cells.withUnsafeBufferPointer({ buffer in
                try renderer.update(
                    frame: NativePreparedFrame(
                        cells: buffer,
                        generation: 3,
                        rows: 1,
                        columns: 1,
                        damage: oneRow
                    ),
                    backingScale: 1,
                    forceFullRebuild: true
                ) == .updated
            }) else {
                return false
            }
            guard renderer.glyphStats.uploads > beforeBold.uploads else { return false }

            // Blank backgrounds, underline geometry and cursor inversion all use
            // the same cell rectangle and must render without relying on glyphs.
            cells = [
                preparedCell(
                    foreground: terminalRGB(red: 255, green: 0, blue: 0),
                    background: terminalRGB(red: 0, green: 0, blue: 255),
                    flags: 2
                ),
                preparedCell(
                    foreground: terminalRGB(red: 0, green: 255, blue: 0),
                    background: terminalRGB(red: 255, green: 0, blue: 0)
                ),
            ]
            let stylePassed = try cells.withUnsafeBufferPointer { buffer -> Bool in
                let frame = NativePreparedFrame(
                    cells: buffer,
                    generation: 4,
                    rows: 1,
                    columns: 2,
                    cursorRow: 0,
                    cursorColumn: 1,
                    cursorVisible: true,
                    damage: oneRow
                )
                guard try renderer.update(
                    frame: frame,
                    backingScale: 1,
                    forceFullRebuild: true
                ) == .updated else {
                    return false
                }
                let cellSize = renderer.cellPixelSize(backingScale: 1)
                guard let texture = renderer.renderOffscreenAndWait(
                    width: cellSize.width * 2,
                    height: cellSize.height
                ) else {
                    return false
                }
                let underlineY = max(0, cellSize.height - 1)
                return pixelMatches(
                    texture,
                    x: cellSize.width / 2,
                    y: cellSize.height / 2,
                    red: 0,
                    green: 0,
                    blue: 255
                ) && pixelMatches(
                    texture,
                    x: cellSize.width / 2,
                    y: underlineY,
                    red: 255,
                    green: 0,
                    blue: 0
                ) && pixelMatches(
                    texture,
                    x: cellSize.width + cellSize.width / 2,
                    y: cellSize.height / 2,
                    red: 0,
                    green: 255,
                    blue: 0
                )
            }
            guard stylePassed else { return false }

            let fullRebuildsBeforeScale = renderer.stats.fullRebuilds
            let atlasResetsBeforeScale = renderer.glyphStats.resets
            guard try cells.withUnsafeBufferPointer({ buffer in
                try renderer.update(
                    frame: NativePreparedFrame(
                        cells: buffer,
                        generation: 5,
                        rows: 1,
                        columns: 2,
                        cursorRow: 0,
                        cursorColumn: 1,
                        cursorVisible: true,
                        fullRebuild: false,
                        damage: DamageMask()
                    ),
                    backingScale: 2
                ) == .updated
            }) else {
                return false
            }
            guard renderer.stats.fullRebuilds > fullRebuildsBeforeScale,
                  renderer.glyphStats.resets > atlasResetsBeforeScale
            else {
                return false
            }

            // Hide must release dedicated resources. Showing again requests the
            // latest committed frame rather than replaying PTY bytes and forces a
            // reconstructable full redraw.
            renderer.setVisible(false)
            guard !renderer.hasDedicatedSurfaceResources else { return false }
            var currentFrameRequests = 0
            renderer.onNeedsCurrentFrame = { currentFrameRequests += 1 }
            renderer.setVisible(true)
            guard currentFrameRequests == 1 else { return false }
            let fullRebuildsBeforeShow = renderer.stats.fullRebuilds
            guard try cells.withUnsafeBufferPointer({ buffer in
                try renderer.update(
                    frame: NativePreparedFrame(
                        cells: buffer,
                        generation: 6,
                        rows: 1,
                        columns: 2,
                        cursorRow: 0,
                        cursorColumn: 1,
                        cursorVisible: true,
                        fullRebuild: false,
                        damage: DamageMask()
                    ),
                    backingScale: 1
                ) == .updated
            }) else {
                return false
            }
            guard renderer.stats.fullRebuilds > fullRebuildsBeforeShow else { return false }
            renderer.setVisible(false)
            guard !renderer.hasDedicatedSurfaceResources else { return false }

            // Offscreen tests above prove deterministic pixels. These separate
            // proofs cover finite atlas pressure/reclamation, repeated surface
            // lifecycle cleanup, and the production CAMetalLayer path itself.
            guard atlasPressureSelfTest(device: device),
                  try repeatedLifecycleSelfTest(device: device),
                  try productionLayerPresentSelfTest(device: device),
                  historyPrepareDefersWhileFrameInFlightSelfTest()
            else {
                return false
            }
            return GlyphAtlas.budgetBytes == 16 * 1024 * 1024
        } catch {
            return false
        }
    }

    /// #817 — a width-two lead grapheme must draw into its continuation cell.
    /// The assertion samples the real offscreen Metal output, rather than only
    /// checking that the production surface exists.
    static func wideGraphemeOffscreenSelfTest() -> Bool {
        guard let device = MTLCreateSystemDefaultDevice() else { return false }
        do {
            let renderer = try MetalTerminalRenderer(device: device)
            let leadReserved: UInt16 = 1 | (2 << 2) | (1 << 4)
            let continuationReserved: UInt16 = 2
            var lead = preparedCell(
                foreground: terminalRGB(red: 255, green: 255, blue: 255)
            )
            lead.reserved = leadReserved
            var continuation = preparedCell()
            continuation.reserved = continuationReserved
            let cells = [lead, continuation]
            let payload = Data("👩‍💻".utf8)
            var sidecar = Data([
                UInt8(payload.count & 0xff),
                UInt8((payload.count >> 8) & 0xff),
            ])
            sidecar.append(payload)
            var damage = DamageMask()
            damage.mark(row: 0)
            return try cells.withUnsafeBufferPointer { buffer in
                let frame = NativePreparedFrame(
                    cells: buffer,
                    generation: 1,
                    rows: 1,
                    columns: 2,
                    damage: damage,
                    graphemeUtf8: sidecar
                )
                guard try renderer.update(
                    frame: frame,
                    backingScale: 1,
                    forceFullRebuild: true
                ) == .updated else {
                    return false
                }
                let cellSize = renderer.cellPixelSize(backingScale: 1)
                guard let texture = renderer.renderOffscreenAndWait(
                    width: cellSize.width * 2,
                    height: cellSize.height
                ) else {
                    return false
                }
                return textureRegionContainsBrightGlyph(
                    texture,
                    xStart: cellSize.width,
                    xEnd: cellSize.width * 2
                )
            }
        } catch {
            return false
        }
    }

    /// #817 — a normal glyph in the live two-pass path must match the same
    /// glyph rendered by the history single-pass path. This catches sampling
    /// the atlas during the background pass, which would blend the glyph twice.
    static func normalGlyphMatchesSinglePassOffscreenSelfTest() -> Bool {
        guard let device = MTLCreateSystemDefaultDevice() else { return false }
        do {
            let renderer = try MetalTerminalRenderer(device: device)
            let cell = preparedCell(scalar: UInt32(ascii: "A"))
            let blank = preparedCell()
            var damage = DamageMask()
            damage.mark(row: 0)
            let cells = [cell, blank]
            guard try cells.withUnsafeBufferPointer({ buffer in
                try renderer.update(
                    frame: NativePreparedFrame(
                        cells: buffer,
                        generation: 1,
                        rows: 1,
                        columns: 2,
                        fullRebuild: true,
                        damage: damage
                    ),
                    backingScale: 1
                ) == .updated
            }) else {
                return false
            }
            let cellSize = renderer.cellPixelSize(backingScale: 1)
            let history = NativeHistoryRange(
                startLine: 1,
                endLine: 1,
                blockID: 817,
                requestID: 1,
                revision: 1,
                rows: [[
                    NativeHistoryRange.Cell(
                        scalar: UInt32(ascii: "A"),
                        foreground: 0xffe9_e1d8,
                        background: 0xff10_0d0b,
                        flags: 0
                    )
                ]]
            )
            let region = NativeTranscriptRegion(
                id: 817,
                origin: NSPoint(x: CGFloat(cellSize.width), y: 0),
                clip: NSRect(
                    x: CGFloat(cellSize.width),
                    y: 0,
                    width: CGFloat(cellSize.width),
                    height: CGFloat(cellSize.height)
                )
            )
            guard try renderer.update(
                historyRange: history,
                region: region,
                backingScale: 1
            ) == .updated else {
                return false
            }
            renderer.setHistoryRegionOrder([817])
            guard let texture = renderer.renderOffscreenAndWait(
                width: cellSize.width * 2,
                height: cellSize.height
            ) else {
                return false
            }
            return textureRegionsMatch(
                texture,
                firstX: 0,
                secondX: cellSize.width,
                width: cellSize.width,
                height: cellSize.height
            )
        } catch {
            return false
        }
    }

    /// #817 — a generation with no damaged rows must reuse the prepared
    /// grapheme projection without invoking the sidecar scan again.
    static func noDamageGraphemeReuseSelfTest() -> Bool {
        guard let device = MTLCreateSystemDefaultDevice() else { return false }
        do {
            let renderer = try MetalTerminalRenderer(device: device)
            var cell = preparedCell(scalar: UInt32(ascii: "e"))
            cell.reserved = 1 | (1 << 2) | (1 << 4)
            let cells = [cell]
            let payload = Data("e\u{301}".utf8)
            var sidecar = Data([
                UInt8(payload.count & 0xff),
                UInt8((payload.count >> 8) & 0xff),
            ])
            sidecar.append(payload)
            var fullDamage = DamageMask()
            fullDamage.mark(row: 0)
            let first = try cells.withUnsafeBufferPointer { buffer in
                try renderer.update(
                    frame: NativePreparedFrame(
                        cells: buffer,
                        generation: 1,
                        rows: 1,
                        columns: 1,
                        fullRebuild: true,
                        damage: fullDamage,
                        graphemeUtf8: sidecar
                    ),
                    backingScale: 1
                )
            }
            guard first == .updated else { return false }
            let glyphsBefore = renderer.glyphStats
            let rebuiltRowsBefore = renderer.stats.rebuiltRows
            let second = try cells.withUnsafeBufferPointer { buffer in
                try renderer.update(
                    frame: NativePreparedFrame(
                        cells: buffer,
                        generation: 2,
                        rows: 1,
                        columns: 1,
                        fullRebuild: false,
                        damage: DamageMask(),
                        graphemeUtf8: sidecar
                    ),
                    backingScale: 1
                )
            }
            return second == .updated
                && renderer.glyphStats == glyphsBefore
                && renderer.stats.rebuiltRows == rebuiltRowsBefore
        } catch {
            return false
        }
    }

    static func liveSelfTest(expectAlternateScreen: Bool) -> Bool {
        guard let device = MTLCreateSystemDefaultDevice() else { return false }
        let connect = seyal_bridge_connect_first()
        guard connect == 0 else { return false }
        defer { seyal_bridge_disconnect() }

        do {
            let renderer = try MetalTerminalRenderer(device: device)
            let expected = expectAlternateScreen ? "ALT-LIVE" : "SEYAL-LIVE"
            let deadline = Date().addingTimeInterval(3)
            while Date() < deadline {
                let bridgeFrame = seyal_bridge_frame()
                if let frame = NativePreparedFrame(bridgeFrame: bridgeFrame),
                   (!expectAlternateScreen || frame.alternateScreen),
                   frameContains(frame, text: expected)
                {
                    guard try renderer.update(
                        frame: frame,
                        backingScale: 1,
                        forceFullRebuild: true
                    ) == .updated else {
                        return false
                    }
                    let cellSize = renderer.cellPixelSize(backingScale: 1)
                    guard let texture = renderer.renderOffscreenAndWait(
                        width: cellSize.width * frame.columns,
                        height: cellSize.height * frame.rows
                    ), textureContainsBrightGlyphPixel(texture) else {
                        return false
                    }

                    // The same real Candidate-D-prepared state must also be
                    // accepted by the production CAMetalLayer presentation path.
                    let hosted = makePresentationLayer(
                        device: device,
                        width: cellSize.width * frame.columns,
                        height: cellSize.height * frame.rows
                    )
                    let layer = hosted.layer
                    let keepAlive = hosted.keepAlive
                    let completedBefore = renderer.stats.completedFrames
                    let submittedBefore = renderer.stats.submittedFrames
                    guard presentOnLayerForValidation(renderer: renderer, layer: layer),
                          renderer.stats.submittedFrames == submittedBefore + 1
                    else {
                        return false
                    }
                    let completed = waitForGPUCompletion(renderer, after: completedBefore)
                    _ = keepAlive
                    return completed
                }

                let poll = seyal_bridge_poll()
                if poll < 0 {
                    return false
                }
                Thread.sleep(forTimeInterval: 0.002)
            }
            return false
        } catch {
            return false
        }
    }

    private static func writeM002CohortFile(path: String, cohort: Int, samples: [Double]) throws {
        var body = "cohort = \(cohort)\nsamples = ["
        body += samples.map { String(format: "%.9f", $0) }.joined(separator: ", ")
        body += "]\n"
        try body.write(to: URL(fileURLWithPath: path), atomically: true, encoding: .utf8)
    }

    /// Five-cohort collector for `renderer_prepare_submission`.
    /// Measures production `NativePreparedFrame` update through Metal command
    /// submission (`renderOffscreenAndMeasureSubmission`). This is not
    /// scanout / key-to-photon.
    private static func runM002ContractCohort(gate: String) -> Bool {
        guard gate == "renderer_prepare_submission" else {
            fputs("unsupported M002 contract gate \(gate)\n", stderr)
            return false
        }
        guard let device = MTLCreateSystemDefaultDevice() else { return false }
        let env = ProcessInfo.processInfo.environment
        guard let out = env["SEYAL_M002_COHORT_OUT"], !out.isEmpty else {
            fputs("SEYAL_M002_COHORT_OUT is required\n", stderr)
            return false
        }
        let cohort = max(Int(env["SEYAL_M002_COHORT"] ?? "") ?? 1, 1)
        let warmups = max(Int(env["SEYAL_M002_WARMUPS"] ?? "") ?? 20, 1)
        let sampleCount = max(Int(env["SEYAL_M002_SAMPLES"] ?? "") ?? 100, 1)
        let rows = 40
        let columns = 120
        do {
            let renderer = try MetalTerminalRenderer(device: device)
            var cells = [SeyalPreparedCell](
                repeating: preparedCell(scalar: UInt32(ascii: "a")),
                count: rows * columns
            )
            var full = DamageMask()
            full.markAll(rows: rows)
            try cells.withUnsafeBufferPointer { buffer in
                _ = try renderer.update(
                    frame: NativePreparedFrame(
                        cells: buffer,
                        generation: 1,
                        rows: rows,
                        columns: columns,
                        damage: full
                    ),
                    backingScale: 1,
                    forceFullRebuild: true
                )
            }
            let cellSize = renderer.cellPixelSize(backingScale: 1)
            var retained: [Double] = []
            retained.reserveCapacity(sampleCount)
            for iteration in 0..<(warmups + sampleCount) {
                let row = iteration % rows
                let index = row * columns
                cells[index].scalar = iteration.isMultiple(of: 2)
                    ? UInt32(ascii: "a")
                    : UInt32(ascii: "b")
                var damage = DamageMask()
                damage.mark(row: row)
                let started = DispatchTime.now().uptimeNanoseconds
                try cells.withUnsafeBufferPointer { buffer in
                    _ = try renderer.update(
                        frame: NativePreparedFrame(
                            cells: buffer,
                            generation: UInt64(iteration + 2),
                            rows: rows,
                            columns: columns,
                            cursorRow: row,
                            cursorColumn: 0,
                            cursorVisible: true,
                            fullRebuild: false,
                            damage: damage
                        ),
                        backingScale: 1
                    )
                }
                guard let submission = renderer.renderOffscreenAndMeasureSubmission(
                    width: cellSize.width * columns,
                    height: cellSize.height * rows
                ) else {
                    fputs("Metal submission failed for M002 contract cohort\n", stderr)
                    return false
                }
                _ = submission
                let ms = Double(DispatchTime.now().uptimeNanoseconds - started) / 1_000_000.0
                if iteration >= warmups {
                    retained.append(ms)
                }
            }
            try writeM002CohortFile(path: out, cohort: cohort, samples: retained)
            print("pass6_native_renderer m002_contract gate=\(gate) cohort=\(cohort) warmups=\(warmups) samples=\(sampleCount) out=\(out) boundary=prepare_through_metal_submission performance_claim=false")
            return true
        } catch {
            fputs("M002 renderer contract failed: \(error)\n", stderr)
            return false
        }
    }

    static func runBenchmark() -> Bool {
        if let gate = ProcessInfo.processInfo.environment["SEYAL_M002_CONTRACT_GATE"],
           !gate.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        {
            return runM002ContractCohort(gate: gate)
        }
        guard let device = MTLCreateSystemDefaultDevice() else { return false }
        do {
            let renderer = try MetalTerminalRenderer(device: device)
            let rows = 40
            let columns = 120
            let repetitions = 120
            var cells = [SeyalPreparedCell](
                repeating: preparedCell(scalar: UInt32(ascii: "a")),
                count: rows * columns
            )
            var full = DamageMask()
            full.markAll(rows: rows)
            try cells.withUnsafeBufferPointer { buffer in
                _ = try renderer.update(
                    frame: NativePreparedFrame(
                        cells: buffer,
                        generation: 1,
                        rows: rows,
                        columns: columns,
                        damage: full
                    ),
                    backingScale: 1,
                    forceFullRebuild: true
                )
            }

            let cellSize = renderer.cellPixelSize(backingScale: 1)
            let hostedPresentation = makePresentationLayer(
                device: device,
                width: cellSize.width * columns,
                height: cellSize.height * rows,
                hosted: false
            )
            let presentationLayer = hostedPresentation.layer
            // CAMetalDisplayLink only produces frame opportunities for a layer
            // participating in an AppKit window hierarchy. Keep this small
            // benchmark window visible so the measurement exercises the same
            // scheduler/layer contract as production.
            let application = NSApplication.shared
            application.setActivationPolicy(.accessory)
            application.finishLaunching()
            application.activate(ignoringOtherApps: true)
            let benchmarkWindow = NSWindow(
                contentRect: NSRect(
                    x: 0,
                    y: 0,
                    width: cellSize.width * columns,
                    height: cellSize.height * rows
                ),
                styleMask: [.borderless],
                backing: .buffered,
                defer: false
            )
            let benchmarkHost = NSView(frame: benchmarkWindow.contentRect(forFrameRect: benchmarkWindow.frame))
            benchmarkHost.wantsLayer = true
            benchmarkHost.layer?.addSublayer(presentationLayer)
            benchmarkWindow.contentView = benchmarkHost
            benchmarkWindow.makeKeyAndOrderFront(nil)
            // Foundation CI sets SEYAL_REQUIRE_DISPLAY_LINK_BENCHMARK=0. Do not
            // arm CAMetalDisplayLink in that mode: Xcode 16.4 Trace/BPTs when
            // the witness (or any MainActor-isolated present path) runs from the
            // main run loop without a Swift MainActor task, and the CI honesty
            // contract already treats presentation-proxy as PLATFORM_LIMITED.
            let presentationProxyRequired = ProcessInfo.processInfo.environment[
                "SEYAL_REQUIRE_DISPLAY_LINK_BENCHMARK"
            ] == "1"
            let displayLinkDriver: DisplayLinkBenchmarkDriver?
            if presentationProxyRequired {
                displayLinkDriver = DisplayLinkBenchmarkDriver(
                    renderer: renderer,
                    layer: presentationLayer
                )
            } else {
                displayLinkDriver = nil
            }
            defer { displayLinkDriver?.invalidate() }
            var preparationSamples = [UInt64]()
            var preparedToCommitSamples = [UInt64]()
            var commitToCompletionSamples = [UInt64]()
            preparationSamples.reserveCapacity(repetitions)
            preparedToCommitSamples.reserveCapacity(repetitions)
            commitToCompletionSamples.reserveCapacity(repetitions)
            var presentationProxyAvailable = presentationProxyRequired

            for iteration in 0..<repetitions {
                let row = iteration % rows
                let index = row * columns
                cells[index].scalar = iteration.isMultiple(of: 2)
                    ? UInt32(ascii: "a")
                    : UInt32(ascii: "b")
                var damage = DamageMask()
                damage.mark(row: row)

                let prepareStarted = DispatchTime.now().uptimeNanoseconds
                try cells.withUnsafeBufferPointer { buffer in
                    _ = try renderer.update(
                        frame: NativePreparedFrame(
                            cells: buffer,
                            generation: UInt64(iteration + 2),
                            rows: rows,
                            columns: columns,
                            cursorRow: row,
                            cursorColumn: 0,
                            cursorVisible: true,
                            fullRebuild: false,
                            damage: damage
                        ),
                        backingScale: 1
                    )
                }
                preparationSamples.append(
                    DispatchTime.now().uptimeNanoseconds - prepareStarted
                )

                guard let submission = renderer.renderOffscreenAndMeasureSubmission(
                    width: cellSize.width * columns,
                    height: cellSize.height * rows
                ) else {
                    return false
                }
                preparedToCommitSamples.append(submission.preparedToCommitNanoseconds)
                commitToCompletionSamples.append(submission.commitToCompletionNanoseconds)
                if let displayLinkDriver, presentationProxyAvailable,
                   !displayLinkDriver.submitOne()
                {
                    guard presentationProxyRequired else {
                        presentationProxyAvailable = false
                        continue
                    }
                    return false
                }
            }

            let prep = percentileSummary(preparationSamples)
            let preparedToCommit = percentileSummary(preparedToCommitSamples)
            let commitToCompletion = percentileSummary(commitToCompletionSamples)
            let glyph = renderer.glyphStats
            print("pass6_native_renderer performance_claim=false boundaries=committed_generation_to_prepared_rows,prepared_batch_to_command_commit,command_commit_to_gpu_completion,committed_generation_to_presented_frame_proxy")
            print("device=\(device.name) registry_id=\(device.registryID) os=\(ProcessInfo.processInfo.operatingSystemVersionString) geometry=\(columns)x\(rows) repetitions=\(repetitions) backing_scale=1 percentile_method=nearest_rank")
            print("preparation p50_ns=\(prep.p50) p95_ns=\(prep.p95) p99_ns=\(prep.p99) max_ns=\(prep.max)")
            print("prepared_to_command_commit p50_ns=\(preparedToCommit.p50) p95_ns=\(preparedToCommit.p95) p99_ns=\(preparedToCommit.p99) max_ns=\(preparedToCommit.max) note=offscreen_target_allocation_excluded")
            print("command_commit_to_gpu_completion_proxy p50_ns=\(commitToCompletion.p50) p95_ns=\(commitToCompletion.p95) p99_ns=\(commitToCompletion.p99) max_ns=\(commitToCompletion.max)")
            if presentationProxyAvailable, let displayLinkDriver, !displayLinkDriver.samples.isEmpty {
                let presented = percentileSummary(displayLinkDriver.samples)
                print("committed_generation_to_presented_frame_proxy p50_ns=\(presented.p50) p95_ns=\(presented.p95) p99_ns=\(presented.p99) max_ns=\(presented.max) note=one_shot_CAMetalDisplayLink_to_command_commit")
            } else if presentationProxyRequired {
                print("committed_generation_to_presented_frame_proxy status=PLATFORM_LIMITED samples=0 reason=no_WindowServer_display_session")
            } else {
                print("committed_generation_to_presented_frame_proxy status=PLATFORM_LIMITED samples=0 reason=SEYAL_REQUIRE_DISPLAY_LINK_BENCHMARK_not_set")
            }
            print("renderer submitted_frames=\(renderer.stats.submittedFrames) display_link_samples=\(displayLinkDriver?.samples.count ?? 0) coalesced_frames=\(renderer.stats.coalescedFrames) rebuilt_rows=\(renderer.stats.rebuiltRows) rebuilt_cells=\(renderer.stats.rebuiltCells) instance_bytes=\(renderer.stats.instanceBytes) glyph_hits=\(glyph.hits) glyph_misses=\(glyph.misses) glyph_uploads=\(glyph.uploads) glyph_uploaded_bytes=\(glyph.uploadedBytes) atlas_budget_bytes=\(GlyphAtlas.budgetBytes) dedicated_gpu_bytes=\(renderer.estimatedDedicatedGPUBytes)")
            guard runUnicodeRendererBenchmark(device: device) else { return false }
            benchmarkWindow.orderOut(nil)
            return true
        } catch {
            return false
        }
    }

    /// M002 renderer diagnostics use the production NativePreparedFrame ->
    /// MetalTerminalRenderer -> CoreText/GlyphAtlas path. The accepted base
    /// predates complete grapheme presentation, so its Unicode result is
    /// explicitly noncomparable while the scalar benchmark above remains the
    /// common regression series.
    private static func runUnicodeRendererBenchmark(device: MTLDevice) -> Bool {
        let rows = 40
        let columns = 120
        let workload = unicodeRendererWorkload(rows: rows, columns: columns)
        guard workload.graphemeCount > 0 else { return false }

        do {
            let renderer = try MetalTerminalRenderer(device: device)
            var damage = DamageMask()
            damage.markAll(rows: rows)
            let resourcesBeforeCold = processResourceSnapshot()
            let statsBeforeCold = renderer.glyphStats
            let coldStarted = DispatchTime.now().uptimeNanoseconds
            let coldResult = try workload.cells.withUnsafeBufferPointer { buffer in
                try renderer.update(
                    frame: NativePreparedFrame(
                        cells: buffer,
                        generation: 1,
                        rows: rows,
                        columns: columns,
                        fullRebuild: true,
                        damage: damage,
                        graphemeUtf8: workload.sidecar
                    ),
                    backingScale: 1,
                    forceFullRebuild: true
                )
            }
            let coldPreparation = DispatchTime.now().uptimeNanoseconds - coldStarted
            let resourcesAfterCold = processResourceSnapshot()
            let statsAfterCold = renderer.glyphStats
            guard coldResult == .updated,
                  statsAfterCold.graphemeMisses > statsBeforeCold.graphemeMisses,
                  statsAfterCold.graphemeFallbackRuns
                    > statsBeforeCold.graphemeFallbackRuns
            else {
                return false
            }

            let resourcesBeforeWarm = resourcesAfterCold
            let statsBeforeWarm = statsAfterCold
            let warmStarted = DispatchTime.now().uptimeNanoseconds
            let warmResult = try workload.cells.withUnsafeBufferPointer { buffer in
                try renderer.update(
                    frame: NativePreparedFrame(
                        cells: buffer,
                        generation: 2,
                        rows: rows,
                        columns: columns,
                        fullRebuild: false,
                        damage: damage,
                        graphemeUtf8: workload.sidecar
                    ),
                    backingScale: 1
                )
            }
            let warmPreparation = DispatchTime.now().uptimeNanoseconds - warmStarted
            let resourcesAfterWarm = processResourceSnapshot()
            let statsAfterWarm = renderer.glyphStats
            guard warmResult == .updated,
                  statsAfterWarm.graphemeHits > statsBeforeWarm.graphemeHits,
                  statsAfterWarm.graphemeMisses == statsBeforeWarm.graphemeMisses
            else {
                return false
            }

            print("m002_unicode_renderer performance_claim=false baseline_commit=3359dc8 baseline_unicode_pipeline=UNSUPPORTED_NONCOMPARABLE common_scalar_series=pass6_native_renderer workload=complete_grapheme_fallback geometry=\(columns)x\(rows) grapheme_leads=\(workload.graphemeCount) backing_scale=1")
            print("m002_unicode_renderer cache_phase=cold_miss preparation_ns=\(coldPreparation) cache_hits=\(statsAfterCold.graphemeHits - statsBeforeCold.graphemeHits) cache_misses=\(statsAfterCold.graphemeMisses - statsBeforeCold.graphemeMisses) full_shaping_ns=\(statsAfterCold.graphemeShapingNanoseconds - statsBeforeCold.graphemeShapingNanoseconds) font_fallback_runs=\(statsAfterCold.graphemeFallbackRuns - statsBeforeCold.graphemeFallbackRuns) process_cpu_ns=\(resourcesAfterCold.cpuNanoseconds - resourcesBeforeCold.cpuNanoseconds) process_rss_bytes=\(resourcesAfterCold.peakResidentBytes) rss_semantics=process_peak")
            print("m002_unicode_renderer cache_phase=warm_hit preparation_ns=\(warmPreparation) cache_hits=\(statsAfterWarm.graphemeHits - statsBeforeWarm.graphemeHits) cache_misses=\(statsAfterWarm.graphemeMisses - statsBeforeWarm.graphemeMisses) full_shaping_ns=\(statsAfterWarm.graphemeShapingNanoseconds - statsBeforeWarm.graphemeShapingNanoseconds) font_fallback_runs=\(statsAfterWarm.graphemeFallbackRuns - statsBeforeWarm.graphemeFallbackRuns) process_cpu_ns=\(resourcesAfterWarm.cpuNanoseconds - resourcesBeforeWarm.cpuNanoseconds) process_rss_bytes=\(resourcesAfterWarm.peakResidentBytes) rss_semantics=process_peak")
            return true
        } catch {
            return false
        }
    }

    private static func unicodeRendererWorkload(
        rows: Int,
        columns: Int
    ) -> (cells: [SeyalPreparedCell], sidecar: Data, graphemeCount: Int) {
        let samples: [(text: String, width: UInt16)] = [
            ("e\u{301}", 1),
            ("\u{2764}\u{fe0f}", 2),
            ("\u{1f44d}\u{1f3fd}", 2),
            ("\u{1f469}\u{200d}\u{1f4bb}", 2),
            ("\u{1f1ee}\u{1f1f3}", 2),
            ("\u{0ba8}\u{0bbf}", 1),
            ("\u{0646}\u{0651}", 1),
        ]
        var cells = [SeyalPreparedCell]()
        var sidecar = Data()
        var sampleIndex = 0
        var graphemeCount = 0
        cells.reserveCapacity(rows * columns)

        for _ in 0..<rows {
            var column = 0
            while column < columns {
                var sample = samples[sampleIndex % samples.count]
                sampleIndex += 1
                if sample.width == 2, column + 1 == columns {
                    sample = samples[0]
                }
                let payload = Data(sample.text.utf8)
                guard payload.count <= Int(UInt16.max) else { continue }
                var lead = preparedCell(
                    scalar: sample.text.unicodeScalars.first?.value ?? 0xfffd
                )
                lead.reserved = 1 | (sample.width << 2) | (1 << 4)
                cells.append(lead)
                sidecar.append(UInt8(payload.count & 0xff))
                sidecar.append(UInt8((payload.count >> 8) & 0xff))
                sidecar.append(payload)
                graphemeCount += 1
                column += 1
                if sample.width == 2 {
                    var continuation = preparedCell()
                    continuation.reserved = 2
                    cells.append(continuation)
                    column += 1
                }
            }
        }
        return (cells, sidecar, graphemeCount)
    }

    private struct ProcessResourceSnapshot {
        let cpuNanoseconds: UInt64
        let peakResidentBytes: UInt64
    }

    private static func processResourceSnapshot() -> ProcessResourceSnapshot {
        var usage = rusage()
        guard getrusage(RUSAGE_SELF, &usage) == 0 else {
            return ProcessResourceSnapshot(cpuNanoseconds: 0, peakResidentBytes: 0)
        }
        let user = timevalNanoseconds(usage.ru_utime)
        let system = timevalNanoseconds(usage.ru_stime)
        return ProcessResourceSnapshot(
            cpuNanoseconds: user &+ system,
            peakResidentBytes: UInt64(max(0, usage.ru_maxrss))
        )
    }

    private static func timevalNanoseconds(_ value: timeval) -> UInt64 {
        UInt64(max(0, value.tv_sec)) &* 1_000_000_000
            &+ UInt64(max(0, value.tv_usec)) &* 1_000
    }

    private static func atlasPressureSelfTest(device: MTLDevice) -> Bool {
        let atlas = GlyphAtlas(device: device)
        let pressureScale: CGFloat = 48
        let pressureMetrics = atlas.metrics(backingScale: pressureScale)
        var reachedCapacity = false

        // At this controlled scale the finite 2048x2048x4 atlas can hold fewer
        // than the printable ASCII glyph set, so pressure is deterministic and
        // does not require thousands of platform-font rasterizations.
        for scalar in UInt32(0x21)...UInt32(0x7e) {
            do {
                _ = try atlas.lookup(
                    scalar: scalar,
                    bold: false,
                    backingScale: pressureScale,
                    cellMetrics: pressureMetrics
                )
            } catch GlyphAtlasError.capacityExceeded {
                reachedCapacity = true
                break
            } catch {
                return false
            }
        }
        guard reachedCapacity,
              atlas.estimatedResidentBytes == GlyphAtlas.budgetBytes,
              atlas.entryCount > 0
        else {
            return false
        }

        let resetsBefore = atlas.stats.resets
        atlas.resetWhenGPUIdle()
        guard atlas.stats.resets == resetsBefore + 1,
              atlas.entryCount == 0,
              atlas.estimatedResidentBytes == 0
        else {
            return false
        }

        do {
            let normalMetrics = atlas.metrics(backingScale: 1)
            _ = try atlas.lookup(
                scalar: UInt32(ascii: "A"),
                bold: false,
                backingScale: 1,
                cellMetrics: normalMetrics
            )
            return atlas.entryCount == 1
                && atlas.estimatedResidentBytes == GlyphAtlas.budgetBytes
        } catch {
            return false
        }
    }

    private static func repeatedLifecycleSelfTest(device: MTLDevice) throws -> Bool {
        var damage = DamageMask()
        damage.mark(row: 0)
        let cells = [preparedCell(scalar: UInt32(ascii: "A"))]

        for generation in 1...8 {
            weak var releasedRenderer: MetalTerminalRenderer?
            do {
                let renderer = try MetalTerminalRenderer(device: device)
                releasedRenderer = renderer
                renderer.setVisible(false)
                guard !renderer.hasDedicatedSurfaceResources,
                      renderer.estimatedDedicatedGPUBytes == 0
                else {
                    return false
                }

                renderer.setVisible(true)
                guard try cells.withUnsafeBufferPointer({ buffer in
                    try renderer.update(
                        frame: NativePreparedFrame(
                            cells: buffer,
                            generation: UInt64(generation),
                            rows: 1,
                            columns: 1,
                            damage: damage
                        ),
                        backingScale: 1,
                        forceFullRebuild: true
                    ) == .updated
                }), renderer.hasDedicatedSurfaceResources else {
                    return false
                }

                renderer.setVisible(false)
                guard !renderer.hasDedicatedSurfaceResources,
                      renderer.estimatedDedicatedGPUBytes == 0
                else {
                    return false
                }
            }
            guard releasedRenderer == nil else { return false }
        }
        return true
    }

    private static func productionLayerPresentSelfTest(device: MTLDevice) throws -> Bool {
        let renderer = try MetalTerminalRenderer(device: device)
        var damage = DamageMask()
        damage.mark(row: 0)
        let cells = [preparedCell(scalar: UInt32(ascii: "A"))]
        guard try cells.withUnsafeBufferPointer({ buffer in
            try renderer.update(
                frame: NativePreparedFrame(
                    cells: buffer,
                    generation: 1,
                    rows: 1,
                    columns: 1,
                    damage: damage
                ),
                backingScale: 1,
                forceFullRebuild: true
            ) == .updated
        }) else {
            return false
        }
        let cellSize = renderer.cellPixelSize(backingScale: 1)
        let hosted = makePresentationLayer(
            device: device,
            width: cellSize.width,
            height: cellSize.height
        )
        let layer = hosted.layer
        let keepAlive = hosted.keepAlive
        let completedBefore = renderer.stats.completedFrames
        guard presentOnLayerForValidation(renderer: renderer, layer: layer), renderer.stats.submittedFrames == 1 else {
            return false
        }

        // While a submitted frame is still in flight, a scale-invalidating
        // update must coalesce rather than reset/reclaim atlas resources.
        let resetsWhileInFlight = renderer.glyphStats.resets
        let deferred = try cells.withUnsafeBufferPointer { buffer in
            try renderer.update(
                frame: NativePreparedFrame(
                    cells: buffer,
                    generation: 2,
                    rows: 1,
                    columns: 1,
                    fullRebuild: true,
                    damage: damage
                ),
                backingScale: 2
            )
        }
        guard deferred == .deferred,
              renderer.glyphStats.resets == resetsWhileInFlight,
              waitForGPUCompletion(renderer, after: completedBefore)
        else {
            return false
        }

        let resetsAfterCompletion = renderer.glyphStats.resets
        let updated = try cells.withUnsafeBufferPointer { buffer in
            try renderer.update(
                frame: NativePreparedFrame(
                    cells: buffer,
                    generation: 3,
                    rows: 1,
                    columns: 1,
                    fullRebuild: true,
                    damage: damage
                ),
                backingScale: 2
            )
        }
        _ = keepAlive
        return updated == .updated && renderer.glyphStats.resets > resetsAfterCompletion
    }

    static func inFlightVisibilityRecoverySelfTest() -> Bool {
        guard let device = MTLCreateSystemDefaultDevice() else { return false }
        do {
            let renderer = try MetalTerminalRenderer(device: device)
            let cells = [preparedCell(scalar: UInt32(ascii: "A"))]
            var damage = DamageMask()
            damage.mark(row: 0)
            guard try cells.withUnsafeBufferPointer({ buffer in
                try renderer.update(
                    frame: NativePreparedFrame(
                        cells: buffer,
                        generation: 1,
                        rows: 1,
                        columns: 1,
                        damage: damage
                    ),
                    backingScale: 1,
                    forceFullRebuild: true
                ) == .updated
            }) else { return false }

            let cellSize = renderer.cellPixelSize(backingScale: 1)
            let hosted = makePresentationLayer(
                device: device,
                width: cellSize.width,
                height: cellSize.height
            )
            let layer = hosted.layer
            let keepAlive = hosted.keepAlive
            let completedBefore = renderer.stats.completedFrames
            guard presentOnLayerForValidation(renderer: renderer, layer: layer),
                  renderer.hasFrameInFlight
            else {
                return false
            }
            // Hide while in flight and do not issue another update/input —
            // completion wakeup alone must release dedicated resources.
            renderer.setVisible(false)
            let released = waitForGPUCompletion(renderer, after: completedBefore)
                && !renderer.hasFrameInFlight
                && !renderer.hasDedicatedSurfaceResources
            _ = keepAlive
            return released
        } catch {
            return false
        }
    }

    /// Deferred Candidate-D update while a frame is in flight must be woken by
    /// GPU completion delivery alone — no further input/update.
    static func deferredFrameCompletionWakeupSelfTest() -> Bool {
        guard let device = MTLCreateSystemDefaultDevice() else { return false }
        do {
            let renderer = try MetalTerminalRenderer(device: device)
            let cells = [preparedCell(scalar: UInt32(ascii: "A"))]
            var damage = DamageMask()
            damage.mark(row: 0)
            guard try cells.withUnsafeBufferPointer({ buffer in
                try renderer.update(
                    frame: NativePreparedFrame(
                        cells: buffer,
                        generation: 1,
                        rows: 1,
                        columns: 1,
                        damage: damage
                    ),
                    backingScale: 1,
                    forceFullRebuild: true
                ) == .updated
            }) else { return false }

            let cellSize = renderer.cellPixelSize(backingScale: 1)
            let hosted = makePresentationLayer(
                device: device,
                width: cellSize.width,
                height: cellSize.height
            )
            let layer = hosted.layer
            let keepAlive = hosted.keepAlive
            var currentFrameRequests = 0
            renderer.onNeedsCurrentFrame = { currentFrameRequests += 1 }
            let completedBefore = renderer.stats.completedFrames
            guard presentOnLayerForValidation(renderer: renderer, layer: layer),
                  renderer.hasFrameInFlight
            else {
                return false
            }
            let deferred = try cells.withUnsafeBufferPointer { buffer in
                try renderer.update(
                    frame: NativePreparedFrame(
                        cells: buffer,
                        generation: 2,
                        rows: 1,
                        columns: 1,
                        fullRebuild: true,
                        damage: damage
                    ),
                    backingScale: 2
                )
            }
            guard deferred == .deferred else { return false }
            // No further updates — only completion wakeup may request the frame.
            let woken = waitForGPUCompletion(renderer, after: completedBefore)
                && currentFrameRequests >= 1
                && !renderer.hasFrameInFlight
            _ = keepAlive
            return woken
        } catch {
            return false
        }
    }

    static func failedReplacementInvalidationSelfTest() -> Bool {
        guard let device = MTLCreateSystemDefaultDevice() else { return false }
        do {
            let renderer = try MetalTerminalRenderer(device: device)
            var cells = [
                preparedCell(scalar: UInt32(ascii: "A")),
                preparedCell(scalar: UInt32(ascii: "B"))
            ]
            var damage = DamageMask()
            damage.mark(row: 0)
            guard try cells.withUnsafeBufferPointer({ buffer in
                try renderer.update(
                    frame: NativePreparedFrame(
                        cells: buffer,
                        generation: 1,
                        rows: 1,
                        columns: 2,
                        damage: damage
                    ),
                    backingScale: 1,
                    forceFullRebuild: true
                ) == .updated
            }), renderer.hasPresentablePreparedState else { return false }

            cells[1].reserved = 1
            do {
                _ = try cells.withUnsafeBufferPointer { buffer in
                    try renderer.update(
                        frame: NativePreparedFrame(
                            cells: buffer,
                            generation: 2,
                            rows: 1,
                            columns: 2,
                            damage: damage
                        ),
                        backingScale: 1,
                        forceFullRebuild: true
                    )
                }
                return false
            } catch {
                guard !renderer.hasPresentablePreparedState else { return false }
            }

            cells[1].reserved = 0
            return try cells.withUnsafeBufferPointer { buffer in
                try renderer.update(
                    frame: NativePreparedFrame(
                        cells: buffer,
                        generation: 3,
                        rows: 1,
                        columns: 2,
                        damage: damage
                    ),
                    backingScale: 1,
                    forceFullRebuild: true
                ) == .updated && renderer.hasPresentablePreparedState
            }
        } catch {
            return false
        }
    }

    /// #786: history prepare must not CPU-write the shared glyph atlas while a
    /// command buffer is still sampling it.
    static func historyPrepareDefersWhileFrameInFlightSelfTest() -> Bool {
        guard let device = MTLCreateSystemDefaultDevice() else { return false }
        do {
            let renderer = try MetalTerminalRenderer(device: device)
            let liveCells = [preparedCell(scalar: UInt32(ascii: "A"))]
            var damage = DamageMask()
            damage.mark(row: 0)
            guard try liveCells.withUnsafeBufferPointer({ buffer in
                try renderer.update(
                    frame: NativePreparedFrame(
                        cells: buffer,
                        generation: 1,
                        rows: 1,
                        columns: 1,
                        fullRebuild: true,
                        damage: damage
                    ),
                    backingScale: 1
                ) == .updated
            }) else { return false }

            let cellSize = renderer.cellPixelSize(backingScale: 1)
            let hosted = makePresentationLayer(
                device: device,
                width: max(cellSize.width, 8),
                height: max(cellSize.height, 8)
            )
            let layer = hosted.layer
            let keepAlive = hosted.keepAlive
            let completedBefore = renderer.stats.completedFrames
            guard presentOnLayerForValidation(renderer: renderer, layer: layer),
                  renderer.hasFrameInFlight
            else {
                return false
            }

            let uploadsBefore = renderer.glyphStats.uploads
            let history = NativeHistoryRange(
                startLine: 1,
                endLine: 1,
                blockID: 42,
                requestID: 7,
                revision: 1,
                rows: [[
                    NativeHistoryRange.Cell(
                        scalar: UInt32(ascii: "Z"),
                        foreground: 0xffe9_e1d8,
                        background: 0xff10_0d0b,
                        flags: 0
                    )
                ]]
            )
            let region = NativeTranscriptRegion(
                id: 42,
                origin: .zero,
                clip: NSRect(
                    x: 0,
                    y: 0,
                    width: CGFloat(cellSize.width),
                    height: CGFloat(cellSize.height)
                )
            )
            let deferred = try renderer.update(
                historyRange: history,
                region: region,
                backingScale: 1
            )
            guard deferred == .deferred,
                  renderer.hasDeferredHistoryPrepare,
                  renderer.historyRegionCount == 0,
                  renderer.glyphStats.uploads == uploadsBefore
            else {
                return false
            }

            guard waitForGPUCompletion(renderer, after: completedBefore),
                  !renderer.hasFrameInFlight
            else {
                return false
            }
            let ok = !renderer.hasDeferredHistoryPrepare
                && renderer.historyRegionCount == 1
                && renderer.glyphStats.uploads > uploadsBefore
            _ = keepAlive
            return ok
        } catch {
            return false
        }
    }

    private static func makePresentationLayer(
        device: MTLDevice,
        width: Int,
        height: Int,
        hosted: Bool = true
    ) -> (layer: CAMetalLayer, keepAlive: NSWindow?) {
        let layer = CAMetalLayer()
        layer.device = device
        layer.pixelFormat = .bgra8Unorm
        layer.framebufferOnly = true
        layer.maximumDrawableCount = 2
        layer.presentsWithTransaction = false
        layer.contentsScale = 1
        layer.bounds = CGRect(x: 0, y: 0, width: width, height: height)
        layer.drawableSize = CGSize(width: width, height: height)
        // Headless Xcode 16.4 CI Trace/BPTs when presenting into an unattached
        // CAMetalLayer. Mirror the benchmark contract: the layer must live in
        // an AppKit window hierarchy for drawable present to complete safely.
        guard hosted else { return (layer, nil) }
        return (layer, hostPresentationLayer(layer, width: width, height: height))
    }

    private static func hostPresentationLayer(
        _ layer: CAMetalLayer,
        width: Int,
        height: Int
    ) -> NSWindow {
        let application = NSApplication.shared
        application.setActivationPolicy(.accessory)
        application.finishLaunching()
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: max(width, 1), height: max(height, 1)),
            styleMask: [.borderless],
            backing: .buffered,
            defer: false
        )
        let host = NSView(
            frame: NSRect(x: 0, y: 0, width: max(width, 1), height: max(height, 1))
        )
        host.wantsLayer = true
        layer.frame = host.bounds
        host.layer?.addSublayer(layer)
        window.contentView = host
        window.orderFront(nil)
        return window
    }

    private static func waitForGPUCompletion(
        _ renderer: MetalTerminalRenderer,
        after completedBefore: UInt64
    ) -> Bool {
        // GPU threads publish into the lock-free mailbox; a coalesced
        // `DispatchQueue.main.async` also drains for production wakeups. Poll
        // both the mailbox and the main run loop so deferred-frame / hide
        // recovery runs without fabricating completions.
        let deadline = Date().addingTimeInterval(2)
        while renderer.stats.completedFrames == completedBefore && Date() < deadline {
            renderer.drainGPUCompletionsIfNeeded()
            if renderer.stats.completedFrames > completedBefore {
                break
            }
            RunLoop.current.run(mode: .default, before: Date().addingTimeInterval(0.001))
        }
        renderer.drainGPUCompletionsIfNeeded()
        return renderer.stats.completedFrames > completedBefore
    }

    /// Production-path submit: real drawable + command buffer + completion
    /// mailbox. Uses `presentsToDisplay: false` so headless CI exercises the
    /// GPU boundary without compositor Trace/BPT. Returns false when no
    /// drawable is available (`ENVIRONMENT_UNSUPPORTED` for the caller).
    static func presentOnLayerForValidation(
        renderer: MetalTerminalRenderer,
        layer: CAMetalLayer
    ) -> Bool {
        guard let drawable = layer.nextDrawable() else {
            return false
        }
        return renderer.present(drawable: drawable, presentsToDisplay: false)
    }

    private static func frameContains(_ frame: NativePreparedFrame, text: String) -> Bool {
        let scalars = text.unicodeScalars.map(\.value)
        guard !scalars.isEmpty, scalars.count <= frame.cells.count else { return false }
        var matched = 0
        for cell in frame.cells {
            if cell.scalar == scalars[matched] {
                matched += 1
                if matched == scalars.count {
                    return true
                }
            } else {
                matched = cell.scalar == scalars[0] ? 1 : 0
            }
        }
        return false
    }

    private static func textureContainsBrightGlyphPixel(_ texture: MTLTexture) -> Bool {
        let bytesPerRow = texture.width * 4
        var bytes = [UInt8](repeating: 0, count: bytesPerRow * texture.height)
        texture.getBytes(
            &bytes,
            bytesPerRow: bytesPerRow,
            from: MTLRegionMake2D(0, 0, texture.width, texture.height),
            mipmapLevel: 0
        )
        var offset = 0
        while offset + 3 < bytes.count {
            let blue = bytes[offset]
            let green = bytes[offset + 1]
            let red = bytes[offset + 2]
            if max(red, max(green, blue)) > 128 {
                return true
            }
            offset += 4
        }
        return false
    }

    private static func textureRegionContainsBrightGlyph(
        _ texture: MTLTexture,
        xStart: Int,
        xEnd: Int
    ) -> Bool {
        let bytesPerRow = texture.width * 4
        var bytes = [UInt8](repeating: 0, count: bytesPerRow * texture.height)
        texture.getBytes(
            &bytes,
            bytesPerRow: bytesPerRow,
            from: MTLRegionMake2D(0, 0, texture.width, texture.height),
            mipmapLevel: 0
        )
        let clampedStart = max(0, xStart)
        let clampedEnd = min(texture.width, xEnd)
        guard clampedStart < clampedEnd else { return false }
        for y in 0..<texture.height {
            for x in clampedStart..<clampedEnd {
                let offset = y * bytesPerRow + x * 4
                let blue = bytes[offset]
                let green = bytes[offset + 1]
                let red = bytes[offset + 2]
                if max(red, max(green, blue)) > 128 {
                    return true
                }
            }
        }
        return false
    }

    private static func textureRegionsMatch(
        _ texture: MTLTexture,
        firstX: Int,
        secondX: Int,
        width: Int,
        height: Int
    ) -> Bool {
        guard width > 0, height > 0,
              firstX >= 0, secondX >= 0,
              firstX + width <= texture.width,
              secondX + width <= texture.width,
              height <= texture.height
        else {
            return false
        }
        let bytesPerRow = texture.width * 4
        var bytes = [UInt8](repeating: 0, count: bytesPerRow * texture.height)
        texture.getBytes(
            &bytes,
            bytesPerRow: bytesPerRow,
            from: MTLRegionMake2D(0, 0, texture.width, texture.height),
            mipmapLevel: 0
        )
        var maximumDifference = 0
        for y in 0..<height {
            for x in 0..<width {
                let first = (y * bytesPerRow) + ((firstX + x) * 4)
                let second = (y * bytesPerRow) + ((secondX + x) * 4)
                for channel in 0..<4 {
                    maximumDifference = max(
                        maximumDifference,
                        abs(Int(bytes[first + channel]) - Int(bytes[second + channel]))
                    )
                }
            }
        }
        return maximumDifference <= 1
    }

    private static func pixelMatches(
        _ texture: MTLTexture,
        x: Int,
        y: Int,
        red: UInt8,
        green: UInt8,
        blue: UInt8
    ) -> Bool {
        var pixel = [UInt8](repeating: 0, count: 4)
        texture.getBytes(
            &pixel,
            bytesPerRow: 4,
            from: MTLRegionMake2D(x, y, 1, 1),
            mipmapLevel: 0
        )
        return pixel[0] == blue
            && pixel[1] == green
            && pixel[2] == red
            && pixel[3] == 255
    }

    private static func preparedCell(
        scalar: UInt32 = 32,
        foreground: UInt32 = 0,
        background: UInt32 = 0,
        flags: UInt16 = 0
    ) -> SeyalPreparedCell {
        var cell = SeyalPreparedCell()
        cell.scalar = scalar
        cell.foreground = foreground
        cell.background = background
        cell.flags = flags
        cell.reserved = 0
        return cell
    }

    private static func terminalRGB(red: UInt8, green: UInt8, blue: UInt8) -> UInt32 {
        0x0200_0000
            | (UInt32(red) << 16)
            | (UInt32(green) << 8)
            | UInt32(blue)
    }

    private static func percentileSummary(_ input: [UInt64]) -> (p50: UInt64, p95: UInt64, p99: UInt64, max: UInt64) {
        let samples = input.sorted()
        return (
            percentile(samples, 50),
            percentile(samples, 95),
            percentile(samples, 99),
            samples.last ?? 0
        )
    }

    private static func percentile(_ samples: [UInt64], _ value: Int) -> UInt64 {
        guard !samples.isEmpty else { return 0 }
        let rank = max(1, (samples.count * value + 99) / 100)
        return samples[min(rank - 1, samples.count - 1)]
    }
}

private extension UInt32 {
    init(ascii character: Character) {
        self = character.unicodeScalars.first?.value ?? 0x20
    }
}
