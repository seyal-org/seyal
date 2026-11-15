import AppKit

@MainActor
extension SecondaryLivePaneHost {
    func rebuildBlocks() {
        let eligibility = seyal_app_eligibility_for_pane(appHandle, paneLo, paneHi)
        if eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue)
            || eligibility == UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue)
        {
            blocks.arrangedSubviews.forEach { $0.removeFromSuperview() }
            blockCards.removeAll()
            syncTranscriptDocumentSize()
            lastBlockCount = 0
            return
        }
        // Rust retains the scroll anchor across projection and focus changes.
        let previousCards = blockCards
        blockCards.removeAll(keepingCapacity: true)
        let composerSnap = seyal_app_composer_for_pane(appHandle, paneLo, paneHi)
        let count = Int(composerSnap.block_count)
        let sessionSurface = seyal_bridge_session_surface_for_handle(displayHandle)
        let composerTarget = seyal_bridge_composer_target_for_handle(displayHandle)
        let requestableSessionHost = sessionSurface.kind == 0
            ? sessionSurface.host_block_id
            : 0
        let sessionDisplayHandle = displayHandle
        let floorLo = composerSnap.canvas_floor_block_id_lo
        let floorHi = composerSnap.canvas_floor_block_id_hi
        let cellHeight = surface.terminalPresentationCellSize().height
        var retained = Set<UInt64>()
        var visibleIndex = 0
        for index in 0..<count {
            let row = seyal_app_block_row_for_pane(appHandle, paneLo, paneHi, UInt32(index))
            let blockID = row.id_lo
            if blockID != 0 {
                retained.insert(blockID)
            }
            if floorLo != 0 || floorHi != 0 {
                let atOrBelowFloor = row.id_hi < floorHi
                    || (row.id_hi == floorHi && row.id_lo <= floorLo)
                if atOrBelowFloor {
                    let state = row.flags & UInt16(SEYAL_APP_BLOCK_STATE_MASK)
                    let isSession = row.flags & UInt16(SEYAL_APP_BLOCK_SESSION) != 0
                    let keepLocalRunningHost = !isSession
                        && state == UInt16(SEYAL_APP_BLOCK_STATE_RUNNING)
                    if !keepLocalRunningHost {
                        continue
                    }
                }
            }
            let projection = seyal_app_block_projection_for_pane(
                appHandle,
                paneLo,
                paneHi,
                UInt32(index)
            )
            let title = productChromeCopyUTF8(row.title, row.title_len) ?? "command"
            let detail = productChromeCopyUTF8(row.detail, row.detail_len) ?? ""
            let promptRow = seyal_app_copy(appHandle, UInt16(SEYAL_APP_COPY_BLOCK_PROMPT))
            let prompt = productChromeCopyUTF8(promptRow.title, promptRow.title_len) ?? "$"
            let card = CommandBlockStackReconciler.reuseOrCreate(
                blockID: blockID,
                previousCards: previousCards,
                stack: blocks
            ) {
                CommandBlockView(
                    prompt: prompt,
                    title: title,
                    detail: detail,
                    state: row.flags & UInt16(SEYAL_APP_BLOCK_STATE_MASK),
                    cellHeight: cellHeight,
                    lines: outputLineCount(projection: projection),
                    session: row.flags & UInt16(SEYAL_APP_BLOCK_SESSION) != 0
                )
            }
            card.updatePresentation(
                prompt: prompt,
                title: title,
                detail: detail,
                state: row.flags & UInt16(SEYAL_APP_BLOCK_STATE_MASK),
                session: row.flags & UInt16(SEYAL_APP_BLOCK_SESSION) != 0
            )
            // Rust RegionGrid clips are the only visible Block presentation.
            card.isHidden = true
            card.setHeaderRows(
                Int(seyal_bridge_block_viewport_header_rows_for(displayHandle, blockID)),
                cellHeight: cellHeight
            )
            card.canRequestSessionBootstrap = blockID != 0 && blockID == requestableSessionHost
            card.onRequestSessionBootstrap = card.canRequestSessionBootstrap
                ? { [weak self, sessionDisplayHandle, blockID, sessionRevision = sessionSurface.revision] in
                    self?.onFocus?()
                    _ = seyal_bridge_session_request_for_handle(
                        sessionDisplayHandle,
                        blockID,
                        sessionRevision
                    )
                }
                : nil
            // DirectInput (mode 3) owns ordinary REPL keys; Interact is Raw fallback only.
            let runningDirectTarget = inputOwned
                && blockID != 0
                && blockID == composerTarget.block_id
                && composerTarget.target_class == 2
                && (row.flags & UInt16(SEYAL_APP_BLOCK_STATE_MASK))
                    == UInt16(SEYAL_APP_BLOCK_STATE_RUNNING)
            card.canInteractWithRunningProgram = runningDirectTarget
                && composerTarget.mode == 0
            card.onInteractWithRunningProgram = card.canInteractWithRunningProgram
                ? { [weak self, blockID, generation = composerTarget.target_generation,
                     revision = composerTarget.revision] in
                    guard let self else { return }
                    let interactionFence = seyal_app_snapshot(self.appHandle)
                    var action = SeyalAppAction()
                    action.version = UInt16(SEYAL_APP_ABI_VERSION)
                    action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
                    action.kind = UInt16(SEYAL_APP_ACTION_INTERACT_WITH_RUNNING_BLOCK.rawValue)
                    action.applySnapshotFence(interactionFence)
                    action.target_execution_lo = blockID
                    action.target_execution_hi = revision
                    action.target_generation = generation
                    guard seyal_app_apply(self.appHandle, &action) == 0 else { return }
                    self.applyTranscriptPresentation()
                    self.window?.makeFirstResponder(self.surface)
                }
                : nil
            card.onRequestDirectInputFocus = runningDirectTarget
                ? { [weak self, blockID] in
                    guard let self else { return }
                    let target = seyal_bridge_composer_target_for_handle(self.displayHandle)
                    guard seyalComposerTargetIsBlockScopedDirectInput(target),
                        blockID == target.block_id
                    else { return }
                    self.onFocus?()
                    if self.surface.bridge == nil, let historyBridge = self.historyBridge {
                        self.surface.bridge = historyBridge
                    }
                    self.surface.syncInputRoutePresentation()
                    self.window?.makeFirstResponder(self.surface)
                }
                : nil
            // Focused hosts use the primary seyal-block-* ids so existing XCUI
            // and host assertions keep working; unfocused stay pane-secondary.
            let prefix = inputOwned ? "seyal-block-" : "seyal-secondary-block-"
            card.setAccessibilityIdentifier("\(prefix)\(visibleIndex)")
            card.setBodyAccessibilityIdentifier("\(prefix)\(visibleIndex)-body")
            visibleIndex += 1
            card.isSelected = row.flags & UInt16(SEYAL_APP_BLOCK_SELECTED) != 0
            card.onSelect = { [weak self] _ in
                self?.onFocus?()
            }
            let lines = outputLineCount(projection: projection)
            if blockID != 0,
               let retainedRange = surface.retainedHistoryRange(
                   blockID: blockID,
                   displayHandle: displayHandle
               ) {
                card.setBodyTranscript(retainedRange.accessibilityTranscript())
            }
            // Reserve chrome body height for plan→card placement (#1484 manual).
            card.setOutputLines(lines, cellHeight: cellHeight)
            if blockID != 0 {
                blockCards[blockID] = card
                applyBlockOutputProjection(blockID: blockID, projection: projection)
            }
        }
        CommandBlockStackReconciler.removeStale(
            previousCards: previousCards,
            retainedBlockIDs: Set(blockCards.keys),
            from: blocks
        )
        // Bridgeless surfaces: discard through the borrowed window bridge,
        // scoped to this pane's own handle (#1426 residual).
        historyBridge?.discardHistoryRequests(except: retained, forDisplayHandle: displayHandle)
        let retainedTranscriptIDs = retained.union(
            surface.currentTranscriptRegions().map(\.id)
        )
        surface.removeTranscriptRegions(except: retainedTranscriptIDs)
        syncTranscriptDocumentSize()
        publishLiveTailBlocks()
        publishBlockOutputFrame()
        lastBlockCount = visibleIndex
    }

    /// Update selection chrome without recreating Block views.
    func syncBlockSelectionFromProjection() {
        let composerSnap = seyal_app_composer_for_pane(appHandle, paneLo, paneHi)
        for index in 0..<Int(composerSnap.block_count) {
            let row = seyal_app_block_row_for_pane(appHandle, paneLo, paneHi, UInt32(index))
            guard row.id_lo != 0, let card = blockCards[row.id_lo] else { continue }
            card.isSelected = row.flags & UInt16(SEYAL_APP_BLOCK_SELECTED) != 0
        }
    }

    /// Re-request history ranges for existing cards after a display-handle bind.
    /// Also refreshes body height / retained transcript text so a late bind
    /// does not leave 1-line empty stubs (#1424).
    func republishExistingBlockProjections() {
        let composerSnap = seyal_app_composer_for_pane(appHandle, paneLo, paneHi)
        let cellHeight = surface.terminalPresentationCellSize().height
        for index in 0..<Int(composerSnap.block_count) {
            let row = seyal_app_block_row_for_pane(appHandle, paneLo, paneHi, UInt32(index))
            guard row.id_lo != 0, let card = blockCards[row.id_lo] else { continue }
            let projection = seyal_app_block_projection_for_pane(
                appHandle,
                paneLo,
                paneHi,
                UInt32(index)
            )
            if let retainedRange = surface.retainedHistoryRange(
                blockID: row.id_lo,
                displayHandle: displayHandle
            ) {
                card.setBodyTranscript(retainedRange.accessibilityTranscript())
            }
            card.setOutputLines(outputLineCount(projection: projection), cellHeight: cellHeight)
            // Sealed NATIVE only — Running PRIMARY_CLIP paints via LiveTail (#1484).
            if projection.kind == UInt16(SEYAL_APP_BLOCK_PROJECTION_NATIVE_REGIONS)
                || projection.kind == UInt16(SEYAL_APP_BLOCK_PROJECTION_HISTORY)
            {
                applyBlockOutputProjection(blockID: row.id_lo, projection: projection)
            }
        }
        syncTranscriptDocumentSize()
    }

    func outputLineCount(projection: SeyalAppBlockProjection) -> Int {
        switch projection.kind {
        case UInt16(SEYAL_APP_BLOCK_PROJECTION_HISTORY):
            guard projection.start_line > 0, projection.end_line >= projection.start_line else {
                return 1
            }
            return Int(min(projection.end_line - projection.start_line + 1, 512))
        case UInt16(SEYAL_APP_BLOCK_PROJECTION_PRIMARY_CLIP):
            return max(Int(projection.reserved1), 1)
        case UInt16(SEYAL_APP_BLOCK_PROJECTION_NATIVE_REGIONS):
            // Cap chrome height — sealed extent is not the AppKit body floor.
            return max(min(Int(projection.reserved1), 128), 1)
        default:
            return 1
        }
    }

    /// Flow Block output fetches are owned by `applyBlockViewportPlan` (C/D/E).
    func applyBlockOutputProjection(blockID: UInt64, projection: SeyalAppBlockProjection) {
        guard displayHandle != 0 else { return }
        _ = blockID
        _ = projection
    }

    /// Deliver a completed history range into this leaf's Metal surface + card.
    func applyHistoryRange(_ range: NativeHistoryRange) {
        guard range.sourceDisplayHandle == 0 || range.sourceDisplayHandle == displayHandle else {
            return
        }
        // The Runtime batch is already bounded to current RegionGrid clips.
        guard !range.rows.isEmpty else { return }
        surface.retainHistoryRange(range)
        if let planRegion = surface.currentTranscriptRegion(blockID: range.blockID) {
            surface.renderHistoryRange(range, region: planRegion)
        } else {
            surface.renderHistoryRange(range)
        }
    }

    func selectBlockAtViewportRow(_ viewportRow: UInt32) {
        guard displayHandle != 0 else { return }
        var hit = SeyalBlockViewportHit()
        guard seyal_bridge_block_viewport_hit_test_row_for(displayHandle, viewportRow, &hit) != 0 else {
            return
        }
        let composer = seyal_app_composer_for_pane(appHandle, paneLo, paneHi)
        for index in 0..<Int(composer.block_count) {
            let row = seyal_app_block_row_for_pane(appHandle, paneLo, paneHi, UInt32(index))
            guard row.id_lo == hit.block_id else { continue }
            let snapshot = seyal_app_snapshot(appHandle)
            var action = SeyalAppAction()
            action.version = UInt16(SEYAL_APP_ABI_VERSION)
            action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
            action.kind = UInt16(SEYAL_APP_ACTION_SELECT_BLOCK.rawValue)
            action.applySnapshotFence(snapshot)
            action.target_execution_lo = row.id_lo
            action.target_execution_hi = row.id_hi
            guard seyal_app_apply(appHandle, &action) == 0 else { return }
            rebuildBlocks()
            return
        }
    }

    func hasRunningPrimaryClip() -> Bool {
        let composerSnap = seyal_app_composer_for_pane(appHandle, paneLo, paneHi)
        for index in 0..<Int(composerSnap.block_count) {
            let projection = seyal_app_block_projection_for_pane(
                appHandle, paneLo, paneHi, UInt32(index)
            )
            if projection.kind == UInt16(SEYAL_APP_BLOCK_PROJECTION_PRIMARY_CLIP) {
                return true
            }
        }
        return false
    }

    func publishLiveTailBlocks() {
        // Membership and geometry are published from Rust BlockViewportPlan.
        publishBlockOutputFrame()
    }

    /// Forward native Flow scrolling into this Pane's Rust viewport.
    func handleBlockScrollWheel(_ event: NSEvent) -> Bool {
        let eligibility = seyal_app_eligibility_for_pane(appHandle, paneLo, paneHi)
        guard eligibility != UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue),
              eligibility != UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue),
              displayHandle != 0
        else { return false }
        let cellHeight = max(surface.terminalPresentationCellSize().height, 1)
        let deltaRows = event.hasPreciseScrollingDeltas
            ? -event.scrollingDeltaY / cellHeight
            : -event.scrollingDeltaY
        flowScrollRowRemainder += deltaRows
        let wholeRows = Int32(flowScrollRowRemainder.rounded(.towardZero))
        guard wholeRows != 0 else { return true }
        flowScrollRowRemainder -= CGFloat(wholeRows)
        let viewportHeight = max(transcript.contentView.bounds.height, cellHeight)
        let windowRows = UInt32(max(1, (viewportHeight / cellHeight).rounded(.up)))
        guard seyal_bridge_block_viewport_scroll_by_for(
            displayHandle, wholeRows, windowRows
        ) != 0 else { return true }
        syncTranscriptDocumentSize()
        publishBlockOutputFrame()
        return true
    }

    /// Report native viewport height. Rust keeps this display handle's anchor.
    func syncBlockViewportScrollAnchor() {
        let cellHeight = max(surface.terminalPresentationCellSize().height, 1)
        let viewportH = max(transcript.contentView.bounds.height, 1)
        let windowRows = UInt32(max(1, (viewportH / cellHeight).rounded(.up)))
        _ = seyal_bridge_block_viewport_set_window_rows_for(displayHandle, windowRows)
    }

    func publishBlockOutputFrame() {
        let eligibility = seyal_app_eligibility_for_pane(appHandle, paneLo, paneHi)
        let direct = eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue)
            || eligibility == UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue)
        guard !direct, displayHandle != 0 else { return }
        // Rust plan owns Block output geometry and clipping.
        syncBlockViewportScrollAnchor()
        if surface.applyBlockViewportPlan(
            displayHandle: displayHandle,
            historyBridge: historyBridge
        ) {
            blockOutputZeroClipRetries = 0
            lastPublishedTranscriptRegions = surface.currentTranscriptRegions()
            surface.refreshRecoveryAccessibilityValue()
            if ProcessInfo.processInfo.environment["SEYAL_UI_TEST_FORWARD_RUNTIME_COMMAND"] == "1",
               lastVisibleHistoryPixelCount == 0,
               Date().timeIntervalSince(lastHistoryPixelSampleAt) >= 0.5
            {
                lastHistoryPixelSampleAt = Date()
                lastVisibleHistoryPixelCount = surface.inspectFlowPaint(sampleGPU: true).opaquePixelsInsideClips
            }
            return
        }
        // LiveTail owns running paint; empty sealed plan mid-stream is normal.
        if !surface.liveTailBlockIDs().isEmpty || blockCards.isEmpty {
            blockOutputZeroClipRetries = 0
            return
        }
        // Plan absent during sealed revision churn — bounded retry only.
        if blockOutputZeroClipRetries < Self.blockOutputZeroClipRetryLimit {
            blockOutputZeroClipRetries += 1
            DispatchQueue.main.async { [weak self] in
                self?.publishBlockOutputFrame()
            }
        }
    }

    /// Realize Rust's document extent and scroll row for the native scroller.
    /// Hidden cards supply accessibility and actions only.
    func syncTranscriptDocumentSize() {
        guard displayHandle != 0 else { return }
        let cellHeight = max(surface.terminalPresentationCellSize().height, 1)
        let viewportHeight = max(transcript.contentView.bounds.height, 1)
        let windowRows = UInt32(max(1, (viewportHeight / cellHeight).rounded(.up)))
        guard seyal_bridge_block_viewport_set_window_rows_for(displayHandle, windowRows) != 0 else {
            return
        }
        var plan = SeyalBlockViewportPlanHeader()
        guard seyal_bridge_block_viewport_plan_header_for(displayHandle, &plan) != 0 else { return }
        let width = max(transcript.contentView.bounds.width, 1)
        let height = max(CGFloat(plan.total_document_extent) * cellHeight, 1)
        let position = CGFloat(plan.scroll_y) * cellHeight
        isProgrammaticTranscriptScroll = true
        defer { isProgrammaticTranscriptScroll = false }
        if abs(blocks.frame.width - width) > 0.5 || abs(blocks.frame.height - height) > 0.5 {
            blocks.setFrameSize(NSSize(width: width, height: height))
            transcript.reflectScrolledClipView(transcript.contentView)
        }
        if abs(transcript.contentView.bounds.origin.y - position) > 0.5 {
            transcript.contentView.scroll(to: NSPoint(x: 0, y: position))
            transcript.reflectScrolledClipView(transcript.contentView)
        }
        lastObservedTranscriptOriginY = transcript.contentView.bounds.origin.y
    }

    func transcriptDocumentScrollHeight() -> CGFloat {
        var plan = SeyalBlockViewportPlanHeader()
        guard displayHandle != 0,
              seyal_bridge_block_viewport_plan_header_for(displayHandle, &plan) != 0
        else { return 1 }
        return max(CGFloat(plan.total_document_extent)
            * max(surface.terminalPresentationCellSize().height, 1), 1)
    }

    func isNearLiveEnd(tolerance: CGFloat = 24) -> Bool {
        var plan = SeyalBlockViewportPlanHeader()
        guard displayHandle != 0,
              seyal_bridge_block_viewport_plan_header_for(displayHandle, &plan) != 0
        else { return true }
        let cellHeight = max(surface.terminalPresentationCellSize().height, 1)
        let rows = UInt64(max(1, (transcript.contentView.bounds.height / cellHeight).rounded(.up)))
        let liveEnd = plan.total_document_extent > rows ? plan.total_document_extent - rows : 0
        return CGFloat(liveEnd > plan.scroll_y ? liveEnd - plan.scroll_y : 0) * cellHeight <= tolerance
    }

    func scrollTranscriptToLiveEnd() {
        guard displayHandle != 0 else { return }
        let cellHeight = max(surface.terminalPresentationCellSize().height, 1)
        let rows = UInt32(max(1, (transcript.contentView.bounds.height / cellHeight).rounded(.up)))
        _ = seyal_bridge_block_viewport_scroll_to_for(displayHandle, UInt64.max, rows)
        syncTranscriptDocumentSize()
        publishBlockOutputFrame()
    }

}
