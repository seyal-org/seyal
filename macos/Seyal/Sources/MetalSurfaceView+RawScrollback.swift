import AppKit

@MainActor
extension MetalSurfaceView {
    func rawScrollDisplayHandle() -> UInt64 {
        if transcriptDisplayHandle != 0 { return transcriptDisplayHandle }
        guard let bridge else { return 0 }
        return bridge.windowFocusedDisplayHandle != 0
            ? bridge.windowFocusedDisplayHandle : bridge.clientHandle
    }

    var rawScrollOffset: UInt64 {
        currentRawViewportPlan()?.offset_rows ?? 0
    }

    func currentRawViewportPlan() -> SeyalRawViewportPlan? {
        guard let bridge, bridge.isConnected,
            let frame = terminalCurrentFrame(),
            frame.rows > 0,
            let startLineID = terminalViewportStartLineID()
        else { return nil }
        let handle = rawScrollDisplayHandle()
        guard handle != 0 else { return nil }
        let plan = seyal_bridge_raw_viewport_plan_for(handle, startLineID, frame.rows)
        return plan.active == 0 ? nil : plan
    }

    /// Whether Raw local scrollback (not PTY mouse) should own wheel/Page keys.
    func prefersRawLocalScrollback(eligibility: UInt16, alternateScreen: Bool) -> Bool {
        eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue) && !alternateScreen
    }

    /// Drop presentation-local Raw scroll state. Does not change eligibility
    /// presentation (Flow/TUI/Raw) — callers set that after clearing.
    func clearRawScrollbackFollowTail() {
        let handle = rawScrollDisplayHandle()
        if handle != 0 { _ = seyal_bridge_raw_viewport_clear_for(handle) }
        rawScrollPaintedStartLineID = 0
        rawScrollRequestedHistory = nil
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
        guard let startLineID = terminalViewportStartLineID(), startLineID > 0,
            let frame = terminalCurrentFrame()
        else {
            return false
        }
        let handle = rawScrollDisplayHandle()
        guard handle != 0 else { return false }
        let plan = seyal_bridge_raw_viewport_apply_for(
            handle, startLineID, frame.rows, Int32(clamping: deltaRows))
        if plan.active == 0 {
            restoreRawFollowTailLiveGrid()
            return true
        }
        rawScrollPaintedStartLineID = plan.start_line
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
        guard let plan = currentRawViewportPlan() else {
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
        if !force, plan.start_line == rawScrollPaintedStartLineID { return }
        rawScrollPaintedStartLineID = plan.start_line
        // Keep live grid until history arrives. Suppressing early left a blank
        // surface when BlockTimeline purge dropped the in-flight MAX request.
        var requestID: UInt64 = 0
        let handle = rawScrollDisplayHandle()
        let result = requestHistoryRange(
            startLine: plan.start_line,
            endLine: plan.end_line,
            blockID: RawScrollbackViewport.correlationBlockID,
            rawViewportColumns: frame.columns,
            onRequestID: { requestID = $0 }
        )
        if result != 0 {
            rawScrollRequestedHistory = nil
            restoreRawFollowTailLiveGrid()
        } else {
            rawScrollRequestedHistory = RawScrollbackRequest(
                displayHandle: handle,
                requestID: requestID,
                startLine: plan.start_line,
                endLine: plan.end_line
            )
        }
    }

    func terminalViewportStartLineID() -> UInt64? {
        guard let bridge, bridge.isConnected else { return nil }
        let handle = rawScrollDisplayHandle()
        let value =
            handle != 0
            ? seyal_bridge_viewport_start_line_id_for(handle)
            : seyal_bridge_viewport_start_line_id()
        return value == 0 ? nil : value
    }

    func historyExceptPreservingRawScrollback(_ blockIDs: Set<UInt64>) -> Set<UInt64> {
        var kept = blockIDs
        if currentRawViewportPlan() != nil {
            kept.insert(RawScrollbackViewport.correlationBlockID)
        }
        return kept
    }

    @discardableResult
    func renderRawScrollbackHistoryIfNeeded(_ range: NativeHistoryRange) -> Bool {
        guard range.blockID == RawScrollbackViewport.correlationBlockID else { return false }
        // Consume every Raw-correlated result here, including stale pages.
        // Sending a late page through Flow's renderer resurrects removed
        // transcript state and can paint the previous scroll position in place.
        guard RawScrollbackViewport.isCurrentResponse(
            range, expected: rawScrollRequestedHistory
        ), currentRawViewportPlan() != nil else { return true }
        rawScrollRequestedHistory = nil
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
                var requestID: UInt64 = 0
                let handle = rawScrollDisplayHandle()
                let requestResult = requestHistoryRange(
                    startLine: contentStart,
                    endLine: contentEnd,
                    blockID: RawScrollbackViewport.correlationBlockID,
                    rawViewportColumns: UInt16(clamping: columns),
                    onRequestID: { requestID = $0 }
                )
                if requestResult == 0 {
                    rawScrollRequestedHistory = RawScrollbackRequest(
                        displayHandle: handle,
                        requestID: requestID,
                        startLine: contentStart,
                        endLine: contentEnd
                    )
                } else {
                    restoreRawFollowTailLiveGrid()
                }
                return true
            }
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
