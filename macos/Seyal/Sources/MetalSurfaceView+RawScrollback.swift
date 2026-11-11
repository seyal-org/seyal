import AppKit

@MainActor
extension MetalSurfaceView {
    /// Whether Raw local scrollback (not PTY mouse) should own wheel/Page keys.
    func prefersRawLocalScrollback(eligibility: UInt16, alternateScreen: Bool) -> Bool {
        eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue) && !alternateScreen
    }

    /// Drop presentation-local Raw scroll state. Does not change eligibility
    /// presentation (Flow/TUI/Raw) — callers set that after clearing.
    func clearRawScrollbackFollowTail() {
        guard rawScrollOffset != 0 || rawScrollPaintedStartLineID != 0
            || rawScrollDidRefillLeadingBlanks
        else { return }
        rawScrollOffset = 0
        rawScrollPaintedStartLineID = 0
        rawScrollDidRefillLeadingBlanks = false
        var except = Set(historyRanges.keys.map(\.blockID))
        except.remove(RawScrollbackViewport.correlationBlockID)
        discardHistoryRequests(except: except)
        // Removes only the Raw correlation region; Flow Block ids stay intact.
        removeTranscriptRegions(except: except)
    }

    /// Follow-tail in Raw: clear scroll offset and restore live grid paint.
    func restoreRawFollowTailLiveGrid() {
        clearRawScrollbackFollowTail()
        applyRendererPresentation(
            RendererPresentationPlan.resolve(
                eligibility: UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue),
                variant: UInt16(SEYAL_APP_RENDERER_PLAN_VARIANT_DEFAULT)
            )
        )
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
        rawScrollDidRefillLeadingBlanks = false
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
                restoreRawFollowTailLiveGrid()
            }
            return
        }
        // Flow/TUI must never keep driving Raw scroll paint (#1442 regression).
        let eligibility =
            recoveryAppHandle != 0
            ? seyal_app_snapshot(recoveryAppHandle).eligibility : 0
        guard eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue) else {
            clearRawScrollbackFollowTail()
            return
        }
        guard let frame = terminalCurrentFrame(), frame.rows > 0 else { return }
        guard let startLineID = terminalViewportStartLineID(), startLineID > 0 else { return }
        rawScrollOffset = RawScrollbackViewport.clampOffset(
            startLineID: startLineID,
            offset: rawScrollOffset
        )
        guard rawScrollOffset > 0 else {
            restoreRawFollowTailLiveGrid()
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
            restoreRawFollowTailLiveGrid()
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
            restoreRawFollowTailLiveGrid()
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
        let eligibility =
            recoveryAppHandle != 0
            ? seyal_app_snapshot(recoveryAppHandle).eligibility : 0
        guard eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue) else {
            clearRawScrollbackFollowTail()
            return true
        }
        if range.rows.isEmpty {
            restoreRawFollowTailLiveGrid()
            return true
        }
        let columns = Int(terminalCurrentFrame()?.columns ?? 0)
        let targetRows = Int(terminalCurrentFrame()?.rows ?? 0)
        let leadingBlanks = range.rows.prefix { RawScrollbackViewport.isBlankRow($0) }.count
        // After trimming sealed empty rows at LineId 1, refill from the first
        // content LineId so the page is real output—not one title + blank pad.
        if leadingBlanks > 0, !rawScrollDidRefillLeadingBlanks, targetRows > 0 {
            let contentStart = range.startLine &+ UInt64(leadingBlanks)
            let contentEnd = contentStart &+ UInt64(targetRows) &- 1
            if contentStart > range.startLine {
                rawScrollDidRefillLeadingBlanks = true
                _ = requestHistoryRange(
                    startLine: contentStart,
                    endLine: contentEnd,
                    blockID: RawScrollbackViewport.correlationBlockID
                )
                return true
            }
        }
        // History covers the scrolled window; suppress live grid while offset > 0.
        // Flags come from Rust (`raw_scrollback_overlay`), not a Swift literal.
        applyRendererPresentation(
            RendererPresentationPlan.resolve(
                eligibility: UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue),
                variant: UInt16(SEYAL_APP_RENDERER_PLAN_VARIANT_RAW_SCROLLBACK)
            )
        )
        // Flow publishes order via setTranscriptFrame. Raw must set it here or
        // historyRegions stay undrawn (orderedBlockRegions empty → blank).
        renderer.setHistoryRegionOrder([RawScrollbackViewport.correlationBlockID])
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
