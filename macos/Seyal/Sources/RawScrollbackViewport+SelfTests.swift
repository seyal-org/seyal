import Foundation

enum RawScrollbackViewportSelfTests {
    static func runAll() -> Bool {
        lineWindowFollowTail()
            && lineWindowScrolled()
            && lineWindowClamp()
            && applyDelta()
    }

    private static func lineWindowFollowTail() -> Bool {
        RawScrollbackViewport.lineWindow(startLineID: 100, rows: 40, offset: 0) == nil
    }

    private static func lineWindowScrolled() -> Bool {
        guard let window = RawScrollbackViewport.lineWindow(startLineID: 100, rows: 40, offset: 10)
        else { return false }
        return window.start == 90 && window.end == 129
    }

    private static func lineWindowClamp() -> Bool {
        guard let window = RawScrollbackViewport.lineWindow(startLineID: 5, rows: 40, offset: 100)
        else { return false }
        // maxOffset = 4 → start = 1, end = 40
        return window.start == 1 && window.end == 40
            && RawScrollbackViewport.clampOffset(startLineID: 5, offset: 99) == 4
    }

    private static func applyDelta() -> Bool {
        RawScrollbackViewport.applyDelta(startLineID: 100, offset: 0, deltaRows: 3) == 3
            && RawScrollbackViewport.applyDelta(startLineID: 100, offset: 3, deltaRows: -5) == 0
            && RawScrollbackViewport.applyDelta(startLineID: 10, offset: 8, deltaRows: 50) == 9
    }
}
