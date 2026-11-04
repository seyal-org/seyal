import Foundation

/// Presentation-local Raw scrollback window over SPEC-010 primary history.
///
/// Offset is rows above the live viewport origin (`ViewportLineIds[0]`).
/// `offset == 0` means follow-tail / live grid. Never mutates PTY or VT.
enum RawScrollbackViewport {
    /// Correlation id for `HistoryRangeRequest` (Runtime echoes; fetch is by LineId).
    /// Not a BlockTimeline identity. Must match
    /// `LocalDisplayClient::PRESENTATION_HISTORY_CORRELATION_BLOCK_ID` (`u64::MAX`)
    /// so client BlockTimeline purge retains in-flight Raw scrollback.
    static let correlationBlockID: UInt64 = .max

    /// Visible LineId window for a non-zero offset, or `nil` when follow-tail.
    static func lineWindow(
        startLineID: UInt64,
        rows: UInt16,
        offset: UInt64
    ) -> (start: UInt64, end: UInt64)? {
        guard startLineID > 0, rows > 0 else { return nil }
        let maxOffset = startLineID &- 1
        let clamped = min(offset, maxOffset)
        guard clamped > 0 else { return nil }
        let rowCount = UInt64(rows)
        let start = startLineID > clamped ? startLineID &- clamped : 1
        let end = start &+ rowCount &- 1
        return (start, end)
    }

    static func clampOffset(startLineID: UInt64, offset: UInt64) -> UInt64 {
        guard startLineID > 1 else { return 0 }
        return min(offset, startLineID &- 1)
    }

    static func applyDelta(
        startLineID: UInt64,
        offset: UInt64,
        deltaRows: Int
    ) -> UInt64 {
        guard deltaRows != 0 else { return clampOffset(startLineID: startLineID, offset: offset) }
        if deltaRows > 0 {
            let add = UInt64(deltaRows)
            let next = offset > UInt64.max &- add ? UInt64.max : offset &+ add
            return clampOffset(startLineID: startLineID, offset: next)
        }
        let sub = UInt64(-deltaRows)
        let next = offset > sub ? offset &- sub : 0
        return clampOffset(startLineID: startLineID, offset: next)
    }

    /// Pad history rows to the live grid width/height so scrolled Raw fills
    /// like the live surface instead of sparse glyphs on cleared Metal.
    static func padRowsForViewport(
        rows: [[NativeHistoryRange.Cell]],
        columns: Int,
        targetRowCount: Int
    ) -> [[NativeHistoryRange.Cell]] {
        guard columns > 0, targetRowCount > 0 else { return rows }
        let blank = NativeHistoryRange.Cell(
            scalar: 32,
            foreground: 0,
            background: 0,
            flags: 0
        )
        var padded: [[NativeHistoryRange.Cell]] = rows.map { row in
            if row.count >= columns {
                return Array(row.prefix(columns))
            }
            var next = row
            next.reserveCapacity(columns)
            while next.count < columns {
                next.append(blank)
            }
            return next
        }
        while padded.count < targetRowCount {
            padded.append(Array(repeating: blank, count: columns))
        }
        if padded.count > targetRowCount {
            padded = Array(padded.prefix(targetRowCount))
        }
        return padded
    }
}
