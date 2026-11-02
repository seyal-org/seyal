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
            lastBlockCount = 0
            return
        }
        blocks.arrangedSubviews.forEach { $0.removeFromSuperview() }
        blockCards.removeAll()
        let composerSnap = seyal_app_composer_for_pane(appHandle, paneLo, paneHi)
        let count = Int(composerSnap.block_count)
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
            let lines = outputLineCount(projection: projection)
            let card = CommandBlockView(
                prompt: prompt,
                title: title,
                detail: detail,
                state: row.flags & UInt16(SEYAL_APP_BLOCK_STATE_MASK),
                cellHeight: cellHeight,
                lines: lines,
                session: row.flags & UInt16(SEYAL_APP_BLOCK_SESSION) != 0
            )
            card.setAccessibilityIdentifier("seyal-secondary-block-\(visibleIndex)")
            card.setBodyAccessibilityIdentifier("seyal-secondary-block-\(visibleIndex)-body")
            visibleIndex += 1
            card.isSelected = row.flags & UInt16(SEYAL_APP_BLOCK_SELECTED) != 0
            card.onSelect = { [weak self] _ in
                self?.onFocus?()
            }
            if blockID != 0, let retainedRange = surface.retainedHistoryRange(blockID: blockID) {
                card.setBodyTranscript(retainedRange.transcriptText)
            }
            blocks.addArrangedSubview(card)
            if blockID != 0 {
                blockCards[blockID] = card
                applyBlockOutputProjection(blockID: blockID, projection: projection)
            }
        }
        surface.discardHistoryRequests(except: retained)
        surface.removeTranscriptRegions(except: retained)
        syncTranscriptDocumentSize()
        publishLiveTailBlocks()
        publishBlockOutputFrame()
        if visibleIndex > lastBlockCount, followingLiveEnd {
            scrollTranscriptToLiveEnd()
        }
        lastBlockCount = visibleIndex
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
        default:
            return 1
        }
    }

    func applyBlockOutputProjection(blockID: UInt64, projection: SeyalAppBlockProjection) {
        guard projection.kind == UInt16(SEYAL_APP_BLOCK_PROJECTION_HISTORY),
              projection.start_line > 0,
              projection.end_line >= projection.start_line
        else { return }
        _ = surface.requestHistoryRange(
            startLine: projection.start_line,
            endLine: projection.end_line,
            blockID: blockID
        )
    }

    func publishLiveTailBlocks() {
        let eligibility = seyal_app_eligibility_for_pane(appHandle, paneLo, paneHi)
        if eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue)
            || eligibility == UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue)
        {
            surface.setLiveTailBlocks([:])
            return
        }
        let composerSnap = seyal_app_composer_for_pane(appHandle, paneLo, paneHi)
        var live: [UInt64: LiveTailClip] = [:]
        for index in 0..<Int(composerSnap.block_count) {
            let row = seyal_app_block_row_for_pane(appHandle, paneLo, paneHi, UInt32(index))
            let projection = seyal_app_block_projection_for_pane(
                appHandle,
                paneLo,
                paneHi,
                UInt32(index)
            )
            guard row.id_lo != 0,
                  projection.kind == UInt16(SEYAL_APP_BLOCK_PROJECTION_PRIMARY_CLIP),
                  projection.start_line > 0,
                  projection.reserved1 > 0
            else { continue }
            live[row.id_lo] = LiveTailClip(
                startLine: projection.start_line,
                firstRow: projection.reserved0,
                rowCount: UInt16(min(projection.reserved1, UInt32(UInt16.max)))
            )
        }
        surface.setLiveTailBlocks(live)
    }

    func publishBlockOutputFrame() {
        let eligibility = seyal_app_eligibility_for_pane(appHandle, paneLo, paneHi)
        let direct = eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue)
            || eligibility == UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue)
        guard !direct, displayHandle != 0 else { return }
        var regions = collectBlockOutputRegions()
        if regions.isEmpty, !blockCards.isEmpty {
            layoutSubtreeIfNeeded()
            regions = collectBlockOutputRegions()
            if regions.isEmpty {
                if blockOutputZeroClipRetries < Self.blockOutputZeroClipRetryLimit {
                    blockOutputZeroClipRetries += 1
                    DispatchQueue.main.async { [weak self] in
                        self?.publishBlockOutputFrame()
                    }
                }
                return
            }
        }
        blockOutputZeroClipRetries = 0
        regions.sort { $0.id < $1.id }
        transcriptFrameRevision &+= 1
        surface.setTranscriptFrame(
            NativeTranscriptFrame(
                revision: transcriptFrameRevision,
                regions: regions,
                surfaceIdentity: ObjectIdentifier(surface)
            )
        )
    }

    private func collectBlockOutputRegions() -> [NativeTranscriptRegion] {
        var regions: [NativeTranscriptRegion] = []
        for (blockID, card) in blockCards {
            let clip = card.body.convert(card.body.bounds, to: surface)
            guard clip.width > 0, clip.height > 0 else { continue }
            regions.append(NativeTranscriptRegion(id: blockID, origin: clip.origin, clip: clip))
        }
        return regions
    }

    func syncTranscriptDocumentSize() {
        layoutSubtreeIfNeeded()
        let fitted = blocks.fittingSize
        let width = max(fitted.width, transcript.contentView.bounds.width, blocks.frame.width)
        let height = max(fitted.height, blocks.frame.height, 1)
        if abs(blocks.frame.width - width) > 0.5 || abs(blocks.frame.height - height) > 0.5 {
            blocks.setFrameSize(NSSize(width: width, height: height))
            transcript.reflectScrolledClipView(transcript.contentView)
            layoutSubtreeIfNeeded()
        }
    }

    func transcriptDocumentScrollHeight() -> CGFloat {
        let document = transcript.documentView ?? blocks
        return max(document.frame.height, document.fittingSize.height, 1)
    }

    func isNearLiveEnd(tolerance: CGFloat = 24) -> Bool {
        let visible = transcript.contentView.bounds
        let maxY = max(transcriptDocumentScrollHeight() - visible.height, 0)
        return visible.origin.y >= maxY - tolerance
    }

    func scrollTranscriptToLiveEnd() {
        isProgrammaticTranscriptScroll = true
        defer { isProgrammaticTranscriptScroll = false }
        let visible = transcript.contentView.bounds.height
        let y = max(transcriptDocumentScrollHeight() - visible, 0)
        transcript.contentView.scroll(to: NSPoint(x: 0, y: y))
        transcript.reflectScrolledClipView(transcript.contentView)
        followingLiveEnd = true
    }
}
