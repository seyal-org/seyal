import Foundation

enum RawScrollbackViewportSelfTests {
    static func runAll() -> Bool {
        padRowsForViewport()
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
