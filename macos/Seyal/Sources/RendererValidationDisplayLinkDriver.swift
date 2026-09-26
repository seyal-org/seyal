import Foundation
import AppKit
import Darwin
import Metal
@preconcurrency import QuartzCore

/// CAMetalDisplayLink invokes its witness on the main run loop without a Swift
/// MainActor task. Keep this driver off MainActor isolation so the hop closure
/// can capture state without the compiler inserting `assumeIsolated` (which
/// Trace/BPTs under Xcode 16.4 Release `--renderer-benchmark`).
final class DisplayLinkBenchmarkDriver: NSObject, CAMetalDisplayLinkDelegate, @unchecked Sendable {
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

