import Metal
import XCTest

@testable import Seyal

/// #1433 — `GlyphAtlas.lookup`'s bitmap cache hid a repeated, uncached
/// CoreText font/glyph resolution underneath it on every call (hit or miss).
/// Under a stuck/recovering Pane's retry churn this turned every render pass
/// into a full CoreText storm, pegging the main thread. `resolve` must now
/// memoize by its own inputs.
final class TerminalFontResolverCacheTests: XCTestCase {
    func testRepeatedResolveForSameInputsIsACacheHit() {
        let resolver = TerminalFontResolver()

        XCTAssertTrue(
            resolver.warmResolveCacheForTests(scalar: 65, bold: false, backingScale: 2.0),
            "ASCII 'A' must resolve on a fresh resolver"
        )
        XCTAssertEqual(resolver.resolveCacheMisses, 1)
        XCTAssertEqual(resolver.resolveCacheHits, 0)

        XCTAssertTrue(
            resolver.warmResolveCacheForTests(scalar: 65, bold: false, backingScale: 2.0),
            "identical inputs must resolve again"
        )
        XCTAssertEqual(
            resolver.resolveCacheMisses, 1,
            "a repeated call with identical inputs must not re-run CoreText resolution"
        )
        XCTAssertEqual(resolver.resolveCacheHits, 1)
    }

    func testDistinctInputsEachMissOnce() {
        let resolver = TerminalFontResolver()

        resolver.warmResolveCacheForTests(scalar: 65, bold: false, backingScale: 1.0)
        resolver.warmResolveCacheForTests(scalar: 65, bold: true, backingScale: 1.0)
        resolver.warmResolveCacheForTests(scalar: 65, bold: false, backingScale: 2.0)
        resolver.warmResolveCacheForTests(scalar: 66, bold: false, backingScale: 1.0)

        XCTAssertEqual(
            resolver.resolveCacheMisses, 4,
            "bold/backingScale/scalar are all part of the cache key"
        )
        XCTAssertEqual(resolver.resolveCacheHits, 0)

        resolver.warmResolveCacheForTests(scalar: 65, bold: false, backingScale: 1.0)
        XCTAssertEqual(resolver.resolveCacheHits, 1)
        XCTAssertEqual(resolver.resolveCacheMisses, 4)
    }

    func testResolveCacheHardCapDropsOverflow() {
        let resolver = TerminalFontResolver(resolveCacheLimit: 2)
        XCTAssertTrue(resolver.warmResolveCacheForTests(scalar: 65, bold: false, backingScale: 1))
        XCTAssertTrue(resolver.warmResolveCacheForTests(scalar: 66, bold: false, backingScale: 1))
        XCTAssertEqual(resolver.resolveCacheCount, 2)
        XCTAssertTrue(resolver.warmResolveCacheForTests(scalar: 67, bold: false, backingScale: 1))
        XCTAssertEqual(
            resolver.resolveCacheCount, 1,
            "a full cache drops before the new entry so the map never exceeds the cap"
        )
        XCTAssertTrue(resolver.warmResolveCacheForTests(scalar: 67, bold: false, backingScale: 1))
        XCTAssertEqual(resolver.resolveCacheHits, 1)
    }

    func testClearResolveCacheForcesTheNextResolveToMiss() {
        let resolver = TerminalFontResolver()
        XCTAssertTrue(resolver.warmResolveCacheForTests(scalar: 65))
        XCTAssertEqual(resolver.resolveCacheCount, 1)
        resolver.clearResolveCache()
        XCTAssertEqual(resolver.resolveCacheCount, 0)
        XCTAssertTrue(resolver.warmResolveCacheForTests(scalar: 65))
        XCTAssertEqual(resolver.resolveCacheMisses, 2)
        XCTAssertEqual(resolver.resolveCacheHits, 0)
    }

    func testAtlasResetWhenGPUIdleClearsResolveCache() {
        guard let device = MTLCreateSystemDefaultDevice() else { return }
        let resolver = TerminalFontResolver()
        XCTAssertTrue(resolver.warmResolveCacheForTests(scalar: 65, backingScale: 2))
        let atlas = GlyphAtlas(device: device, fontResolver: resolver)
        atlas.resetWhenGPUIdle()
        XCTAssertEqual(resolver.resolveCacheCount, 0)
    }
}
