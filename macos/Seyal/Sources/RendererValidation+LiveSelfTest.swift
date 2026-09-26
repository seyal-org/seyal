import Foundation
import AppKit
import Darwin
import Metal
@preconcurrency import QuartzCore

@MainActor
extension RendererValidation {
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

    static func writeM002CohortFile(path: String, cohort: Int, samples: [Double]) throws {
        var body = "cohort = \(cohort)\nsamples = ["
        body += samples.map { String(format: "%.9f", $0) }.joined(separator: ", ")
        body += "]\n"
        try body.write(to: URL(fileURLWithPath: path), atomically: true, encoding: .utf8)
    }

    /// Five-cohort collector for `renderer_prepare_submission`.
    /// Measures production `NativePreparedFrame` update through Metal command
    /// submission (`renderOffscreenAndMeasureSubmission`). This is not
    /// scanout / key-to-photon.
    static func runM002ContractCohort(gate: String) -> Bool {
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

}
