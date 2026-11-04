import AppKit

@MainActor
extension MetalSurfaceView {
    /// Whether Raw local scrollback (not PTY mouse) should own wheel/Page keys.
    func prefersRawLocalScrollback(eligibility: UInt16, alternateScreen: Bool) -> Bool {
        eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue) && !alternateScreen
    }

    func clearRawScrollbackFollowTail() {
        guard rawScrollOffset != 0 || rawScrollPaintedStartLineID != 0 else { return }
        rawScrollOffset = 0
        rawScrollPaintedStartLineID = 0
        var except = Set(historyRanges.keys.map(\.blockID))
        except.remove(RawScrollbackViewport.correlationBlockID)
        discardHistoryRequests(except: except)
        removeTranscriptRegions(except: except)
        renderer.setHistoryRegionOrder([])
        applyRendererPresentation(.fullPane(.raw))
        publishCurrentTerminalFrame()
    }

    /// Adjust offset by signed row delta and refresh paint. Returns true when handled.
    @discardableResult
    func applyRawScrollDelta(rows deltaRows: Int, eligibility: UInt16, alternateScreen: Bool)
        -> Bool
    {
        guard prefersRawLocalScrollback(eligibility: eligibility, alternateScreen: alternateScreen)
        else {
            return false
        }
        guard let startLineID = terminalViewportStartLineID(), startLineID > 0 else {
            return false
        }
        let next = RawScrollbackViewport.applyDelta(
            startLineID: startLineID,
            offset: rawScrollOffset,
            deltaRows: deltaRows
        )
        if next == rawScrollOffset, next == 0 {
            return true
        }
        rawScrollOffset = next
        refreshRawScrollbackPaint(force: true)
        return true
    }

    func preserveRawScrollbackAfterPresentationChange(eligibility: UInt16) {
        guard eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue), rawScrollOffset > 0 else {
            return
        }
        refreshRawScrollbackPaint(force: true)
    }

    func refreshRawScrollbackPaint(force: Bool = false) {
        guard rawScrollOffset > 0 else {
            if rawScrollPaintedStartLineID != 0 {
                clearRawScrollbackFollowTail()
            }
            return
        }
        guard let frame = terminalCurrentFrame(), frame.rows > 0 else { return }
        guard let startLineID = terminalViewportStartLineID(), startLineID > 0 else { return }
        rawScrollOffset = RawScrollbackViewport.clampOffset(
            startLineID: startLineID,
            offset: rawScrollOffset
        )
        guard rawScrollOffset > 0 else {
            clearRawScrollbackFollowTail()
            return
        }
        if !force, startLineID == rawScrollPaintedStartLineID { return }
        guard
            let window = RawScrollbackViewport.lineWindow(
                startLineID: startLineID,
                rows: frame.rows,
                offset: rawScrollOffset
            )
        else {
            clearRawScrollbackFollowTail()
            return
        }
        rawScrollPaintedStartLineID = startLineID
        // Keep live grid until history arrives. Suppressing early left a blank
        // surface when BlockTimeline purge dropped the in-flight MAX request.
        let result = requestHistoryRange(
            startLine: window.start,
            endLine: window.end,
            blockID: RawScrollbackViewport.correlationBlockID
        )
        if result != 0 {
            clearRawScrollbackFollowTail()
        }
    }

    func terminalViewportStartLineID() -> UInt64? {
        guard let bridge, bridge.isConnected else { return nil }
        let handle =
            bridge.windowFocusedDisplayHandle != 0
            ? bridge.windowFocusedDisplayHandle : bridge.clientHandle
        let value =
            handle != 0
            ? seyal_bridge_viewport_start_line_id_for(handle)
            : seyal_bridge_viewport_start_line_id()
        return value == 0 ? nil : value
    }

    func historyExceptPreservingRawScrollback(_ blockIDs: Set<UInt64>) -> Set<UInt64> {
        var kept = blockIDs
        if rawScrollOffset > 0 {
            kept.insert(RawScrollbackViewport.correlationBlockID)
        }
        return kept
    }

    @discardableResult
    func renderRawScrollbackHistoryIfNeeded(_ range: NativeHistoryRange) -> Bool {
        guard range.blockID == RawScrollbackViewport.correlationBlockID else { return false }
        guard rawScrollOffset > 0 else { return false }
        if range.rows.isEmpty {
            clearRawScrollbackFollowTail()
            return true
        }
        // History covers the scrolled window; suppress live grid while offset > 0.
        applyRendererPresentation(
            RendererPresentationPlan(
                mode: .raw,
                drawsFullGridBackground: true,
                drawsLiveGrid: false,
                drawsCursorOutsideBlockRegions: false
            )
        )
        // Flow publishes order via setTranscriptFrame. Raw must set it here or
        // historyRegions stay undrawn (orderedBlockRegions empty → blank).
        renderer.setHistoryRegionOrder([RawScrollbackViewport.correlationBlockID])
        let columns = Int(terminalCurrentFrame()?.columns ?? 0)
        let targetRows = Int(terminalCurrentFrame()?.rows ?? 0)
        let paddedRows = RawScrollbackViewport.padRowsForViewport(
            rows: range.rows,
            columns: columns,
            targetRowCount: targetRows
        )
        let painted = NativeHistoryRange(
            startLine: range.startLine,
            endLine: range.endLine,
            blockID: range.blockID,
            requestID: range.requestID,
            revision: range.revision,
            rows: paddedRows,
            status: range.status
        )
        renderHistoryRange(painted)
        return true
    }
}
