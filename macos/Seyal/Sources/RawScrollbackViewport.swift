import Foundation

struct RawScrollbackRequest: Equatable {
    let displayHandle: UInt64
    let requestID: UInt64
    let startLine: UInt64
    let endLine: UInt64
}

/// Renderer-only row normalization for a Rust-authored Raw viewport plan.
enum RawScrollbackViewport {
    /// Matches `HISTORY_CELL_CONTINUATION_FLAG` in the Pass 7 wire contract.
    private static let continuationFlag: UInt16 = 1 << 3

    /// Correlation id for `HistoryRangeRequest` (Runtime echoes; fetch is by LineId).
    /// Not a BlockTimeline identity. Must match
    /// `LocalDisplayClient::PRESENTATION_HISTORY_CORRELATION_BLOCK_ID` (`u64::MAX`)
    /// so client BlockTimeline purge retains in-flight Raw scrollback.
    static let correlationBlockID: UInt64 = .max

    /// History requests can complete out of order while the reader scrolls.
    /// Only the exact latest request may replace the visible Raw page.
    static func isCurrentResponse(
        _ range: NativeHistoryRange,
        expected: RawScrollbackRequest?
    ) -> Bool {
        guard let expected else { return false }
        return range.blockID == correlationBlockID
            && range.sourceDisplayHandle == expected.displayHandle
            && range.requestID == expected.requestID
            && range.startLine == expected.startLine
            && range.endLine == expected.endLine
    }

    /// True when a history row has no visible glyphs (space-only / empty).
    static func isBlankRow(_ row: [NativeHistoryRange.Cell]) -> Bool {
        row.allSatisfy { cell in
            if !cell.graphemeUtf8.isEmpty { return false }
            let scalar = cell.scalar
            return scalar == 0 || scalar == 32
        }
    }

    /// Drop leading blank LineIds so absolute-top scroll top-aligns real content
    /// instead of painting sealed empty viewport rows as a white slab.
    static func trimLeadingBlankRows(
        _ rows: [[NativeHistoryRange.Cell]]
    ) -> [[NativeHistoryRange.Cell]] {
        var index = 0
        while index < rows.count, isBlankRow(rows[index]) {
            index += 1
        }
        if index == 0 { return rows }
        return Array(rows[index...])
    }

    /// Pad history rows to the live grid width/height so scrolled Raw fills
    /// like the live surface instead of sparse glyphs on cleared Metal.
    /// Leading blanks are trimmed first (see `trimLeadingBlankRows`).
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
        let trimmed = trimLeadingBlankRows(rows)
        var padded: [[NativeHistoryRange.Cell]] = trimmed.map { row in
            if row.count >= columns {
                var visible = Array(row.prefix(columns))
                // The next cell can be a wide grapheme continuation. If so,
                // clipping here would leave part of that grapheme at the pane
                // edge. Remove the whole glyph and let the normal pad below
                // clear the vacated cells.
                if row.count > columns, row[columns].flags & continuationFlag != 0 {
                    var glyphStart = columns
                    while glyphStart > 0,
                        row[glyphStart].flags & continuationFlag != 0
                    {
                        glyphStart -= 1
                    }
                    visible.removeSubrange(glyphStart..<visible.count)
                }
                return visible
            }
            var next = row
            next.reserveCapacity(columns)
            while next.count < columns {
                next.append(blank)
            }
            return next
        }
        if padded.count > targetRowCount {
            padded = Array(padded.prefix(targetRowCount))
        }
        while padded.count < targetRowCount {
            padded.append(Array(repeating: blank, count: columns))
        }
        return padded
    }
}
