import Foundation
import AppKit
import Darwin
import Metal
@preconcurrency import QuartzCore

@MainActor
extension RendererValidation {
    static func makePresentationLayer(
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

    static func hostPresentationLayer(
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

    static func waitForGPUCompletion(
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

    static func frameContains(_ frame: NativePreparedFrame, text: String) -> Bool {
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

    static func textureContainsBrightGlyphPixel(_ texture: MTLTexture) -> Bool {
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

    static func textureRegionContainsBrightGlyph(
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

    static func textureRegionsMatch(
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

    static func pixelMatches(
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

    static func preparedCell(
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

    static func terminalRGB(red: UInt8, green: UInt8, blue: UInt8) -> UInt32 {
        0x0200_0000
            | (UInt32(red) << 16)
            | (UInt32(green) << 8)
            | UInt32(blue)
    }

    static func percentileSummary(_ input: [UInt64]) -> (p50: UInt64, p95: UInt64, p99: UInt64, max: UInt64) {
        let samples = input.sorted()
        return (
            percentile(samples, 50),
            percentile(samples, 95),
            percentile(samples, 99),
            samples.last ?? 0
        )
    }

    static func percentile(_ samples: [UInt64], _ value: Int) -> UInt64 {
        guard !samples.isEmpty else { return 0 }
        let rank = max(1, (samples.count * value + 99) / 100)
        return samples[min(rank - 1, samples.count - 1)]
    }
}

extension UInt32 {
    init(ascii character: Character) {
        self = character.unicodeScalars.first?.value ?? 0x20
    }
}
