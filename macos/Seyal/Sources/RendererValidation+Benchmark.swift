import Foundation
import AppKit
import Darwin
import Metal
@preconcurrency import QuartzCore

@MainActor
extension RendererValidation {
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
    static func runUnicodeRendererBenchmark(device: MTLDevice) -> Bool {
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

    static func unicodeRendererWorkload(
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

    struct ProcessResourceSnapshot {
        let cpuNanoseconds: UInt64
        let peakResidentBytes: UInt64
    }

    static func processResourceSnapshot() -> ProcessResourceSnapshot {
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

    static func timevalNanoseconds(_ value: timeval) -> UInt64 {
        UInt64(max(0, value.tv_sec)) &* 1_000_000_000
            &+ UInt64(max(0, value.tv_usec)) &* 1_000
    }

}
