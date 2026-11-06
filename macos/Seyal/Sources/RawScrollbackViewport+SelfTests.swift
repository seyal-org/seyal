import Foundation

enum RawScrollbackViewportSelfTests {
    static func runAll() -> Bool {
        lineWindowFollowTail()
            && lineWindowScrolled()
            && lineWindowClamp()
            && applyDelta()
            && padRowsForViewport()
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
        // maxOffset = 4 → start = 1; over-fetch second page for leading-blank trim
        return window.start == 1 && window.end == 80
            && RawScrollbackViewport.clampOffset(startLineID: 5, offset: 99) == 4
    }

    private static func applyDelta() -> Bool {
        RawScrollbackViewport.applyDelta(startLineID: 100, offset: 0, deltaRows: 3) == 3
            && RawScrollbackViewport.applyDelta(startLineID: 100, offset: 3, deltaRows: -5) == 0
            && RawScrollbackViewport.applyDelta(startLineID: 10, offset: 8, deltaRows: 50) == 9
    }

    private static func padRowsForViewport() -> Bool {
        let space = NativeHistoryRange.Cell(
            scalar: 32, foreground: 0, background: 0, flags: 0)
        let digit = NativeHistoryRange.Cell(
            scalar: 49, foreground: 0, background: 0, flags: 0)
        let short: [[NativeHistoryRange.Cell]] = [[digit]]
        let padded = RawScrollbackViewport.padRowsForViewport(
            rows: short, columns: 4, targetRowCount: 3)
        let withLeadingBlanks = RawScrollbackViewport.padRowsForViewport(
            rows: [[space], [space], [digit]],
            columns: 4,
            targetRowCount: 3
        )
        return padded.count == 3
            && padded.allSatisfy { $0.count == 4 }
            && padded[0][0].scalar == 49
            && padded[0][1].scalar == 32
            && padded[2][0].scalar == 32
            && withLeadingBlanks.count == 3
            && withLeadingBlanks[0][0].scalar == 49
            && RawScrollbackViewport.trimLeadingBlankRows([[space], [digit]]).count == 1
    }
}
