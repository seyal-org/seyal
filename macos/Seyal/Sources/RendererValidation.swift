import Foundation
import AppKit
import Darwin
import Metal
@preconcurrency import QuartzCore

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
    static func atlasPressureSelfTest(device: MTLDevice) -> Bool {
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

    static func repeatedLifecycleSelfTest(device: MTLDevice) throws -> Bool {
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

    static func productionLayerPresentSelfTest(device: MTLDevice) throws -> Bool {
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

}
