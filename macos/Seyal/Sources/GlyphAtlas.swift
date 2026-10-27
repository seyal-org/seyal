import CoreGraphics
import CoreText
import Foundation
import Metal

struct TerminalFontMetrics: Equatable {
    let cellWidth: Int
    let cellHeight: Int
    let ascent: CGFloat
    let descent: CGFloat
}

struct GlyphAtlasEntry {
    let uvRect: SIMD4<Float>
    let slice: UInt32
}

struct GlyphAtlasStats: Equatable {
    var hits: UInt64 = 0
    var misses: UInt64 = 0
    var uploads: UInt64 = 0
    var uploadedBytes: UInt64 = 0
    var resets: UInt64 = 0
    var graphemeHits: UInt64 = 0
    var graphemeMisses: UInt64 = 0
    var graphemeShapingNanoseconds: UInt64 = 0
    var graphemeFallbackRuns: UInt64 = 0
}

enum GlyphAtlasError: Error {
    case unavailableFont
    case unavailableTexture
    case capacityExceeded
    case rasterizationFailed
}

private struct GlyphKey: Hashable {
    let fontName: String
    let glyph: UInt16
    let pixelSizeBits: UInt64
    let bold: Bool
}

private struct GraphemeKey: Hashable {
    let utf8: Data
    let fontName: String
    let pixelSizeBits: UInt64
    let bold: Bool
    let fallbackGeneration: UInt64
}

private struct ResolvedGlyph {
    let font: CTFont
    let glyph: CGGlyph
    let fontName: String
}

private struct AtlasCursor {
    var x = 1
    var y = 1
    var rowHeight = 0
}

final class TerminalFontResolver {
    private let pointSize: CGFloat
    private let regular: CTFont
    private let bold: CTFont
    let resolvedFamily: String

    var regularFontForShaping: CTFont { regular }
    var boldFontForShaping: CTFont { bold }

    init(spec: SeyalResolvedFontSpec = .canonicalTerminal) {
        self.pointSize = spec.pointSize
        let resolved = Self.makeFonts(spec: spec)
        regular = resolved.regular
        bold = resolved.bold
        resolvedFamily = resolved.family
    }

    convenience init(pointSize: CGFloat) {
        self.init(
            spec: SeyalResolvedFontSpec(
                family: SeyalResolvedFontSpec.canonicalTerminal.family,
                fallbacks: SeyalResolvedFontSpec.canonicalTerminal.fallbacks,
                pointSize: pointSize
            )
        )
    }

    private static func makeFonts(spec: SeyalResolvedFontSpec) -> (
        regular: CTFont,
        bold: CTFont,
        family: String
    ) {
        let candidates = ([spec.family] + spec.fallbacks)
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }
        for (index, family) in candidates.enumerated() {
            if let regular = font(named: family, size: spec.pointSize, bold: false) {
                let bold = font(named: family, size: spec.pointSize, bold: true) ?? regular
                // Remaining candidates form the CoreText cascade so Nerd Font /
                // PUA glyphs resolve without changing cell metrics authority
                // (still measured from the primary monospace family).
                let cascade = Array(candidates[(index + 1)...])
                return (
                    withCascade(regular, cascadeFamilies: cascade, size: spec.pointSize),
                    withCascade(bold, cascadeFamilies: cascade, size: spec.pointSize),
                    family
                )
            }
        }
        let fallback = CTFontCreateWithName("Menlo" as CFString, spec.pointSize, nil)
        let bold = CTFontCreateWithName("Menlo-Bold" as CFString, spec.pointSize, nil)
        let cascade = Array(candidates.dropFirst())
        return (
            withCascade(fallback, cascadeFamilies: cascade, size: spec.pointSize),
            withCascade(bold, cascadeFamilies: cascade, size: spec.pointSize),
            "Menlo"
        )
    }

    private static func withCascade(
        _ font: CTFont,
        cascadeFamilies: [String],
        size: CGFloat
    ) -> CTFont {
        let descriptors: [CTFontDescriptor] = cascadeFamilies.compactMap { family in
            guard self.font(named: family, size: size, bold: false) != nil else { return nil }
            return CTFontDescriptorCreateWithAttributes(
                [kCTFontFamilyNameAttribute: family] as CFDictionary
            )
        }
        guard !descriptors.isEmpty else { return font }
        let base = CTFontCopyFontDescriptor(font)
        let cascaded = CTFontDescriptorCreateCopyWithAttributes(
            base,
            [kCTFontCascadeListAttribute: descriptors] as CFDictionary
        )
        return CTFontCreateWithFontDescriptor(cascaded, size, nil)
    }

    private static func font(named family: String, size: CGFloat, bold: Bool) -> CTFont? {
        let descriptor = CTFontDescriptorCreateWithAttributes(
            [
                kCTFontFamilyNameAttribute: family,
                kCTFontSizeAttribute: size,
                kCTFontTraitsAttribute: [
                    kCTFontSymbolicTrait: bold
                        ? CTFontSymbolicTraits.boldTrait.rawValue
                        : 0,
                ],
            ] as CFDictionary
        )
        let font = CTFontCreateWithFontDescriptor(descriptor, size, nil)
        let resolvedFamily = CTFontCopyFamilyName(font) as String
        guard resolvedFamily.compare(family, options: [.caseInsensitive, .diacriticInsensitive])
            == .orderedSame
        else {
            return nil
        }
        return font
    }

    func metrics(backingScale: CGFloat) -> TerminalFontMetrics {
        let scale = max(backingScale, 1)
        let pixelFont = CTFontCreateCopyWithAttributes(regular, pointSize * scale, nil, nil)
        var glyph = glyphForScalar(77, font: pixelFont) ?? 0
        var advance = CGSize.zero
        _ = CTFontGetAdvancesForGlyphs(pixelFont, .horizontal, &glyph, &advance, 1)
        let width = max(1, Int(ceil(advance.width)))
        let ascent = CTFontGetAscent(pixelFont)
        let descent = CTFontGetDescent(pixelFont)
        let leading = CTFontGetLeading(pixelFont)
        let height = max(1, Int(ceil(ascent + descent + leading)))
        return TerminalFontMetrics(
            cellWidth: width,
            cellHeight: height,
            ascent: ascent,
            descent: descent
        )
    }

    fileprivate func resolve(
        scalar: UInt32,
        bold requestedBold: Bool,
        backingScale: CGFloat
    ) -> ResolvedGlyph? {
        if let resolved = resolveDirect(
            scalar: scalar,
            bold: requestedBold,
            backingScale: backingScale
        ) {
            return resolved
        }
        return resolveReplacement(bold: requestedBold, backingScale: backingScale)
    }

    /// Validation seam for M001 scalar correctness. This proves that a valid
    /// scalar can be resolved directly rather than through U+FFFD while keeping
    /// grapheme/emoji width semantics deferred to their owning milestone.
    func canResolveScalarDirectly(
        _ scalar: UInt32,
        bold requestedBold: Bool = false,
        backingScale: CGFloat = 1
    ) -> Bool {
        resolveDirect(
            scalar: scalar,
            bold: requestedBold,
            backingScale: backingScale
        ) != nil
    }

    private func resolveDirect(
        scalar: UInt32,
        bold requestedBold: Bool,
        backingScale: CGFloat
    ) -> ResolvedGlyph? {
        guard let unicodeScalar = UnicodeScalar(scalar) else { return nil }
        let text = String(unicodeScalar)
        let base = requestedBold ? bold : regular
        let utf16Length = (text as NSString).length
        let fallback = CTFontCreateForString(
            base,
            text as CFString,
            CFRange(location: 0, length: utf16Length)
        )
        let scale = max(backingScale, 1)
        let pixelFont = CTFontCreateCopyWithAttributes(
            fallback,
            CTFontGetSize(fallback) * scale,
            nil,
            nil
        )

        guard let glyph = glyphForScalar(scalar, font: pixelFont), glyph != 0 else {
            return nil
        }
        return ResolvedGlyph(
            font: pixelFont,
            glyph: glyph,
            fontName: CTFontCopyPostScriptName(pixelFont) as String
        )
    }

    private func resolveReplacement(
        bold requestedBold: Bool,
        backingScale: CGFloat
    ) -> ResolvedGlyph? {
        let replacement: UInt32 = 0xfffd
        let base = requestedBold ? bold : regular
        let text = "\u{fffd}"
        let fallback = CTFontCreateForString(
            base,
            text as CFString,
            CFRange(location: 0, length: 1)
        )
        let scale = max(backingScale, 1)
        let pixelFont = CTFontCreateCopyWithAttributes(
            fallback,
            CTFontGetSize(fallback) * scale,
            nil,
            nil
        )
        guard let glyph = glyphForScalar(replacement, font: pixelFont), glyph != 0 else {
            return nil
        }
        return ResolvedGlyph(
            font: pixelFont,
            glyph: glyph,
            fontName: CTFontCopyPostScriptName(pixelFont) as String
        )
    }

    /// Resolve one Unicode scalar without making the overwhelmingly common BMP
    /// terminal path pay for a shaping line allocation. BMP scalars map through
    /// CoreText's direct character-to-glyph API. Supplementary-plane scalars are
    /// represented by a UTF-16 surrogate pair and therefore use the shaping path
    /// so the pair is treated as one Unicode scalar/glyph when the font supports
    /// it. Seyal still applies the M001 one-cell placement model here; grapheme,
    /// emoji-width and bidi behavior remain deferred.
    private func glyphForScalar(_ scalar: UInt32, font: CTFont) -> CGGlyph? {
        guard let unicodeScalar = UnicodeScalar(scalar) else { return nil }

        if scalar <= 0xffff {
            var character = UniChar(scalar)
            var glyph: CGGlyph = 0
            guard CTFontGetGlyphsForCharacters(font, &character, &glyph, 1), glyph != 0 else {
                return nil
            }
            return glyph
        }

        let text = String(unicodeScalar)
        let attributed = NSAttributedString(
            string: text,
            attributes: [NSAttributedString.Key(kCTFontAttributeName as String): font]
        )
        let line = CTLineCreateWithAttributedString(attributed as CFAttributedString)
        let runs = CTLineGetGlyphRuns(line) as! [CTRun]
        guard runs.count == 1, CTRunGetGlyphCount(runs[0]) == 1 else { return nil }
        var glyph: CGGlyph = 0
        CTRunGetGlyphs(runs[0], CFRange(location: 0, length: 1), &glyph)
        return glyph == 0 ? nil : glyph
    }
}

final class GlyphAtlas {
    static let width = 2048
    static let height = 2048
    static let sliceCount = 4
    static let bytesPerPixel = 1
    static let budgetBytes = width * height * sliceCount * bytesPerPixel

    private let device: MTLDevice
    private let fontResolver: TerminalFontResolver
    private var textureStorage: MTLTexture?
    private var entries: [GlyphKey: GlyphAtlasEntry] = [:]
    private var graphemeEntries: [GraphemeKey: GlyphAtlasEntry] = [:]
    private var fallbackGeneration: UInt64 = 0
    private let maxGraphemeEntries = 2048
    private var cursors = Array(repeating: AtlasCursor(), count: sliceCount)
    private(set) var stats = GlyphAtlasStats()

    init(device: MTLDevice, fontResolver: TerminalFontResolver = TerminalFontResolver()) {
        self.device = device
        self.fontResolver = fontResolver
    }

    var texture: MTLTexture? {
        textureStorage
    }

    var entryCount: Int {
        entries.count
    }

    var estimatedResidentBytes: Int {
        textureStorage == nil ? 0 : Self.budgetBytes
    }

    func metrics(backingScale: CGFloat) -> TerminalFontMetrics {
        fontResolver.metrics(backingScale: backingScale)
    }

    @discardableResult
    func ensureTextureForRendering() throws -> MTLTexture {
        try ensureTexture()
    }

    func lookup(
        scalar: UInt32,
        bold: Bool,
        backingScale: CGFloat,
        cellMetrics: TerminalFontMetrics
    ) throws -> GlyphAtlasEntry {
        guard let resolved = fontResolver.resolve(
            scalar: scalar,
            bold: bold,
            backingScale: backingScale
        ) else {
            throw GlyphAtlasError.unavailableFont
        }
        let key = GlyphKey(
            fontName: resolved.fontName,
            glyph: resolved.glyph,
            pixelSizeBits: Double(CTFontGetSize(resolved.font)).bitPattern,
            bold: bold
        )
        if let entry = entries[key] {
            stats.hits &+= 1
            return entry
        }

        stats.misses &+= 1
        let bitmap = try rasterize(
            resolved: resolved,
            width: cellMetrics.cellWidth,
            height: cellMetrics.cellHeight
        )
        guard let allocation = allocate(width: bitmap.width, height: bitmap.height) else {
            throw GlyphAtlasError.capacityExceeded
        }
        let texture = try ensureTexture()
        bitmap.bytes.withUnsafeBytes { rawBuffer in
            guard let baseAddress = rawBuffer.baseAddress else { return }
            texture.replace(
                region: MTLRegionMake2D(allocation.x, allocation.y, bitmap.width, bitmap.height),
                mipmapLevel: 0,
                slice: allocation.slice,
                withBytes: baseAddress,
                bytesPerRow: bitmap.width,
                bytesPerImage: bitmap.width * bitmap.height
            )
        }

        let atlasWidth = Float(Self.width)
        let atlasHeight = Float(Self.height)
        let entry = GlyphAtlasEntry(
            uvRect: SIMD4<Float>(
                Float(allocation.x) / atlasWidth,
                Float(allocation.y) / atlasHeight,
                Float(allocation.x + bitmap.width) / atlasWidth,
                Float(allocation.y + bitmap.height) / atlasHeight
            ),
            slice: UInt32(allocation.slice)
        )
        entries[key] = entry
        stats.uploads &+= 1
        stats.uploadedBytes &+= UInt64(bitmap.width * bitmap.height)
        return entry
    }

    /// Shape a multi-scalar grapheme into one atlas tile for the lead cell.
    /// Typographic advances never change terminal width/cursor authority.
    func lookupGrapheme(
        text: String,
        bold: Bool,
        backingScale: CGFloat,
        cellMetrics: TerminalFontMetrics
    ) throws -> GlyphAtlasEntry {
        let scale = max(backingScale, 1)
        let base = bold ? fontResolver.boldFontForShaping : fontResolver.regularFontForShaping
        let pixelFont = CTFontCreateCopyWithAttributes(base, CTFontGetSize(base) * scale, nil, nil)
        let fontName = CTFontCopyPostScriptName(pixelFont) as String
        let utf8 = Data(text.utf8)
        let key = GraphemeKey(
            utf8: utf8,
            fontName: fontName,
            pixelSizeBits: Double(CTFontGetSize(pixelFont)).bitPattern,
            bold: bold,
            fallbackGeneration: fallbackGeneration
        )
        if let entry = graphemeEntries[key] {
            stats.hits &+= 1
            stats.graphemeHits &+= 1
            return entry
        }
        if graphemeEntries.count >= maxGraphemeEntries {
            graphemeEntries.removeAll(keepingCapacity: true)
            fallbackGeneration &+= 1
        }

        stats.misses &+= 1
        stats.graphemeMisses &+= 1
        let shapingStarted = DispatchTime.now().uptimeNanoseconds
        let attributed = NSAttributedString(
            string: text,
            attributes: [NSAttributedString.Key(kCTFontAttributeName as String): pixelFont]
        )
        let line = CTLineCreateWithAttributedString(attributed as CFAttributedString)
        let bounds = CTLineGetImageBounds(line, nil)
        for run in CTLineGetGlyphRuns(line) as! [CTRun] {
            let attributes = CTRunGetAttributes(run) as NSDictionary
            guard let value = attributes[kCTFontAttributeName] else { continue }
            let runFont = value as! CTFont
            if CTFontCopyPostScriptName(runFont) as String != fontName {
                stats.graphemeFallbackRuns &+= 1
            }
        }
        stats.graphemeShapingNanoseconds &+= DispatchTime.now().uptimeNanoseconds
            - shapingStarted
        let width = max(cellMetrics.cellWidth, Int(ceil(bounds.width)) + 2)
        let height = max(cellMetrics.cellHeight, Int(ceil(bounds.height)) + 2)
        var bytes = [UInt8](repeating: 0, count: width * height)
        let colorSpace = CGColorSpaceCreateDeviceGray()
        guard let context = CGContext(
            data: &bytes,
            width: width,
            height: height,
            bitsPerComponent: 8,
            bytesPerRow: width,
            space: colorSpace,
            bitmapInfo: CGImageAlphaInfo.none.rawValue
        ) else {
            throw GlyphAtlasError.rasterizationFailed
        }
        context.translateBy(x: 0, y: CGFloat(height))
        context.scaleBy(x: 1, y: -1)
        context.setFillColor(gray: 1, alpha: 1)
        let ascent = CTFontGetAscent(pixelFont)
        context.textPosition = CGPoint(x: 1, y: CGFloat(height) - ascent - 1)
        CTLineDraw(line, context)

        guard let allocation = allocate(width: width, height: height) else {
            throw GlyphAtlasError.capacityExceeded
        }
        let texture = try ensureTexture()
        bytes.withUnsafeBytes { rawBuffer in
            guard let baseAddress = rawBuffer.baseAddress else { return }
            texture.replace(
                region: MTLRegionMake2D(allocation.x, allocation.y, width, height),
                mipmapLevel: 0,
                slice: allocation.slice,
                withBytes: baseAddress,
                bytesPerRow: width,
                bytesPerImage: width * height
            )
        }
        let atlasWidth = Float(Self.width)
        let atlasHeight = Float(Self.height)
        let entry = GlyphAtlasEntry(
            uvRect: SIMD4<Float>(
                Float(allocation.x) / atlasWidth,
                Float(allocation.y) / atlasHeight,
                Float(allocation.x + width) / atlasWidth,
                Float(allocation.y + height) / atlasHeight
            ),
            slice: UInt32(allocation.slice)
        )
        graphemeEntries[key] = entry
        stats.uploads &+= 1
        stats.uploadedBytes &+= UInt64(width * height)
        return entry
    }

    /// Reclaim the finite atlas. Callers must prove no submitted command buffer
    /// can still reference the old texture before invoking this method.
    func resetWhenGPUIdle() {
        textureStorage = nil
        entries.removeAll(keepingCapacity: true)
        graphemeEntries.removeAll(keepingCapacity: true)
        fallbackGeneration &+= 1
        cursors = Array(repeating: AtlasCursor(), count: Self.sliceCount)
        stats.resets &+= 1
    }

    func releaseResourcesWhenGPUIdle() {
        resetWhenGPUIdle()
        entries.removeAll(keepingCapacity: false)
    }

    private func ensureTexture() throws -> MTLTexture {
        if let textureStorage {
            return textureStorage
        }
        let descriptor = MTLTextureDescriptor()
        descriptor.textureType = .type2DArray
        descriptor.pixelFormat = .r8Unorm
        descriptor.width = Self.width
        descriptor.height = Self.height
        descriptor.arrayLength = Self.sliceCount
        descriptor.mipmapLevelCount = 1
        descriptor.usage = [.shaderRead]
        descriptor.storageMode = .shared
        guard let texture = device.makeTexture(descriptor: descriptor) else {
            throw GlyphAtlasError.unavailableTexture
        }
        texture.label = "Seyal Glyph Atlas"
        textureStorage = texture
        return texture
    }

    private func allocate(width: Int, height: Int) -> (slice: Int, x: Int, y: Int)? {
        guard width > 0, height > 0,
              width + 2 <= Self.width,
              height + 2 <= Self.height
        else {
            return nil
        }

        for slice in 0..<Self.sliceCount {
            var cursor = cursors[slice]
            if cursor.x + width + 1 > Self.width {
                cursor.x = 1
                cursor.y += cursor.rowHeight + 1
                cursor.rowHeight = 0
            }
            guard cursor.y + height + 1 <= Self.height else {
                continue
            }
            let allocation = (slice: slice, x: cursor.x, y: cursor.y)
            cursor.x += width + 1
            cursor.rowHeight = max(cursor.rowHeight, height)
            cursors[slice] = cursor
            return allocation
        }
        return nil
    }

    private func rasterize(
        resolved: ResolvedGlyph,
        width: Int,
        height: Int
    ) throws -> (bytes: [UInt8], width: Int, height: Int) {
        guard width > 0, height > 0 else {
            throw GlyphAtlasError.rasterizationFailed
        }
        var bytes = [UInt8](repeating: 0, count: width * height)
        let colorSpace = CGColorSpaceCreateDeviceGray()
        let madeContext = bytes.withUnsafeMutableBytes { rawBuffer -> Bool in
            guard let baseAddress = rawBuffer.baseAddress,
                  let context = CGContext(
                      data: baseAddress,
                      width: width,
                      height: height,
                      bitsPerComponent: 8,
                      bytesPerRow: width,
                      space: colorSpace,
                      bitmapInfo: CGImageAlphaInfo.none.rawValue
                  )
            else {
                return false
            }
            context.setFillColor(gray: 1, alpha: 1)
            context.setShouldAntialias(true)
            context.setAllowsAntialiasing(true)
            context.textMatrix = .identity

            var glyph = resolved.glyph
            let bounds = CTFontGetBoundingRectsForGlyphs(resolved.font, .default, &glyph, nil, 1)
            var position = CGPoint(
                x: max(0, -bounds.minX),
                y: CTFontGetDescent(resolved.font)
            )
            CTFontDrawGlyphs(resolved.font, &glyph, &position, 1, context)
            return true
        }
        guard madeContext else {
            throw GlyphAtlasError.rasterizationFailed
        }
        return (bytes, width, height)
    }
}
