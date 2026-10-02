import Foundation
import AppKit
import Darwin
import Metal
@preconcurrency import QuartzCore

@MainActor
extension RendererValidation {
    static func retainedDefaultColorsFollowThemeOffscreenSelfTest() -> Bool {
        guard let device = MTLCreateSystemDefaultDevice() else { return false }
        do {
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
                        fullRebuild: true,
                        damage: damage
                    ),
                    backingScale: 1
                ) == .updated
            }) else { return false }
            let size = renderer.cellPixelSize(backingScale: 1)
            renderer.setDefaultTerminalColors(
                foreground: packRGBA(red: 30, green: 35, blue: 40),
                background: packRGBA(red: 252, green: 252, blue: 250)
            )
            guard let light = renderer.renderOffscreenAndWait(width: size.width, height: size.height) else {
                return false
            }
            let lightBackgroundMatches = pixelMatches(
                light, x: size.width - 1, y: size.height - 1, red: 252, green: 252, blue: 250
            )
            let darkGlyphMatches = textureRegionContainsDarkGlyph(light, xStart: 0, xEnd: size.width)
            guard lightBackgroundMatches && darkGlyphMatches else { return false }

            renderer.setDefaultTerminalColors(
                foreground: packRGBA(red: 230, green: 225, blue: 216),
                background: packRGBA(red: 16, green: 13, blue: 11)
            )
            guard let dark = renderer.renderOffscreenAndWait(width: size.width, height: size.height) else {
                return false
            }
            let backgroundMatches = pixelMatches(dark, x: size.width - 1, y: size.height - 1, red: 16, green: 13, blue: 11)
            let brightGlyphMatches = textureRegionContainsBrightGlyph(dark, xStart: 0, xEnd: size.width)
            return backgroundMatches && brightGlyphMatches
        } catch {
            return false
        }
    }

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

    /// #865: Flow running live-tail clips only the mapped primary-frame row
    /// slice into the Block region without enabling Pane-wide live grid drawing.
    static func liveTailPrimaryClipSelfTest() -> Bool {
        guard let device = MTLCreateSystemDefaultDevice() else { return false }
        do {
            let renderer = try MetalTerminalRenderer(device: device)
            renderer.setPresentationPlan(.flow())
            let liveCells = ["A", "B", "H", "i", "!", "!"].map {
                preparedCell(scalar: UInt32(Character($0).asciiValue!))
            }
            var damage = DamageMask()
            damage.markAll(rows: 3)
            guard try liveCells.withUnsafeBufferPointer({ buffer in
                try renderer.update(
                    frame: NativePreparedFrame(
                        cells: buffer,
                        generation: 865,
                        rows: 3,
                        columns: 2,
                        fullRebuild: true,
                        damage: damage
                    ),
                    backingScale: 1
                ) == .updated
            }) else { return false }

            let cellSize = renderer.cellPixelSize(backingScale: 1)
            let region = NativeTranscriptRegion(
                id: 865,
                origin: .zero,
                clip: NSRect(
                    x: 0,
                    y: 0,
                    width: CGFloat(cellSize.width * 2),
                    height: CGFloat(cellSize.height * 2)
                )
            )
            renderer.setTranscriptRegions([region])
            renderer.setLiveTailBlocks([
                865: LiveTailClip(startLine: 30, firstRow: 1, rowCount: 2)
            ])
            guard renderer.liveTailRegionCount == 1,
                  renderer.liveTailInstanceCount(for: 865) == 4
            else { return false }

            let inspection = renderer.inspectPresentation()
            guard inspection.mode == .flow,
                  !inspection.drawsLiveGrid,
                  !inspection.drawsFullGridBackground,
                  !inspection.drawsCursorOutsideBlockRegions,
                  inspection.blockRegionIDs == [865]
            else {
                return false
            }

            let paint = renderer.inspectFlowPaint()
            return paint.isClean
                && paint.historyInstanceCount == 4
                && paint.instancesOutsideClips == 0
        } catch {
            return false
        }
    }
}
