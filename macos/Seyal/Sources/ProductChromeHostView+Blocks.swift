import AppKit

@MainActor
extension ProductChromeHostView {
    func projectRuntimeBlocks() {
        let snapshot = seyal_app_snapshot(pane.appHandle)
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_APPLY_RUNTIME_BLOCKS.rawValue)
        action.applySnapshotFence(snapshot)
        _ = seyal_app_apply(pane.appHandle, &action)
    }

    /// Relay Runtime's composer eligibility into the Rust root unchanged
    /// (#978). Rust decides what the composer shows; this only carries it.
    func relayComposerStatus(_ status: NativeComposerStatus) {
        let snapshot = seyal_app_snapshot(pane.appHandle)
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_APPLY_COMPOSER_STATUS.rawValue)
        action.applySnapshotFence(snapshot)
        action.reserved = UInt32(status.eligibility)
        action.target_execution_lo = status.revision
        _ = seyal_app_apply(pane.appHandle, &action)
    }

    func applyTranscriptPresentation(_ snapshot: SeyalAppSnapshot) {
        let direct = snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue)
            || snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue)
        transcript.isHidden = direct
        if direct {
            NSLayoutConstraint.deactivate(paneFollowsTranscript)
            NSLayoutConstraint.activate(paneFillsCenter)
            let mode: TerminalPresentationMode =
                snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue) ? .tui : .raw
            pane.inputSurface.applyRendererPresentation(.fullPane(mode))
            pane.inputSurface.removeTranscriptRegions(except: [])
            layoutSubtreeIfNeeded()
        } else {
            NSLayoutConstraint.deactivate(paneFillsCenter)
            NSLayoutConstraint.activate(paneFollowsTranscript)
            pane.inputSurface.applyRendererPresentation(.flow())
            layoutSubtreeIfNeeded()
            publishBlockOutputFrame()
        }
    }

    func rebuildBlocks() {
        blocks.arrangedSubviews.forEach { $0.removeFromSuperview() }
        blockCards.removeAll()
        let composer = seyal_app_composer(pane.appHandle)
        let count = Int(composer.block_count)
        let cellHeight = pane.inputSurface.terminalPresentationCellSize().height
        var retained = Set<UInt64>()
        for index in 0..<count {
            let row = seyal_app_block_row(pane.appHandle, UInt32(index))
            let span = seyal_app_block_span(pane.appHandle, UInt32(index))
            let title = productChromeCopyUTF8(row.title, row.title_len) ?? "command"
            let detail = productChromeCopyUTF8(row.detail, row.detail_len) ?? ""
            let promptRow = seyal_app_copy(pane.appHandle, UInt16(SEYAL_APP_COPY_BLOCK_PROMPT))
            let prompt = productChromeCopyUTF8(promptRow.title, promptRow.title_len) ?? "$"
            let blockID = row.id_lo
            let lines = outputLineCount(span)
            let card = CommandBlockView(
                prompt: prompt,
                title: title,
                detail: detail,
                state: row.flags & UInt16(SEYAL_APP_BLOCK_STATE_MASK),
                cellHeight: cellHeight,
                lines: lines
            )
            card.setAccessibilityIdentifier("seyal-block-\(index)")
            card.body.setAccessibilityIdentifier("seyal-block-\(index)-body")
            card.isSelected = row.flags & UInt16(SEYAL_APP_BLOCK_SELECTED) != 0
            let idLo = row.id_lo
            let idHi = row.id_hi
            card.onSelect = { [weak self] selected in
                self?.selectBlock(idLo: idLo, idHi: idHi, deselect: selected)
            }
            blocks.addArrangedSubview(card)
            if blockID != 0 {
                blockCards[blockID] = card
                retained.insert(blockID)
                requestBlockOutput(blockID: blockID, span: span)
            }
        }
        pane.inputSurface.discardHistoryRequests(except: retained)
        layoutSubtreeIfNeeded()
        publishBlockOutputFrame()
        if count > lastBlockCount, followingLiveEnd {
            scrollTranscriptToLiveEnd()
        }
        lastBlockCount = count
    }

    func outputLineCount(_ span: SeyalAppBlockSpan) -> Int {
        guard span.start_line > 0 else { return 1 }
        if span.end_line >= span.start_line {
            return Int(min(span.end_line - span.start_line + 1, 512))
        }
        return 8
    }

    func refreshRunningBlockOutput() {
        let snapshot = seyal_app_snapshot(pane.appHandle)
        if snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue)
            || snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue)
        {
            return
        }
        let composer = seyal_app_composer(pane.appHandle)
        for index in 0..<Int(composer.block_count) {
            let row = seyal_app_block_row(pane.appHandle, UInt32(index))
            let span = seyal_app_block_span(pane.appHandle, UInt32(index))
            guard row.id_lo != 0, span.start_line > 0, span.end_line == 0 else { continue }
            requestBlockOutput(blockID: row.id_lo, span: span)
        }
    }

    func requestBlockOutput(blockID: UInt64, span: SeyalAppBlockSpan) {
        guard span.start_line > 0 else { return }
        let end = span.end_line >= span.start_line
            ? span.end_line
            : span.start_line &+ 511
        _ = pane.inputSurface.requestHistoryRange(
            startLine: span.start_line,
            endLine: max(end, span.start_line),
            blockID: blockID
        )
    }

    func applyHistoryRange(_ range: NativeHistoryRange) {
        pane.inputSurface.retainHistoryRange(range)
        let cellHeight = pane.inputSurface.terminalPresentationCellSize().height
        if let card = blockCards[range.blockID] {
            card.setOutputLines(max(range.rows.count, 1), cellHeight: cellHeight)
        }
        layoutSubtreeIfNeeded()
        publishBlockOutputFrame()
        // History replies arrive after the initial live-end scroll and can grow
        // earlier cards. Keep following only when the user was already at the
        // live end so the newly submitted Block stays hittable.
        if followingLiveEnd {
            scrollTranscriptToLiveEnd()
        }
    }

    func publishBlockOutputFrame() {
        let snapshot = seyal_app_snapshot(pane.appHandle)
        let direct = snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue)
            || snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue)
        guard !direct else { return }
        let surface = pane.inputSurface
        var regions: [NativeTranscriptRegion] = []
        for (blockID, card) in blockCards {
            let clip = card.body.convert(card.body.bounds, to: surface)
            guard clip.width > 0, clip.height > 0 else { continue }
            regions.append(NativeTranscriptRegion(id: blockID, origin: clip.origin, clip: clip))
        }
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

    func isNearLiveEnd(tolerance: CGFloat = 24) -> Bool {
        let document = transcript.documentView ?? blocks
        let visible = transcript.contentView.bounds
        let height = document.fittingSize.height
        let maxY = max(height - visible.height, 0)
        return visible.origin.y >= maxY - tolerance
    }

    func scrollTranscriptToLiveEnd() {
        isProgrammaticTranscriptScroll = true
        defer { isProgrammaticTranscriptScroll = false }
        let document = transcript.documentView ?? blocks
        let visible = transcript.contentView.bounds.height
        let height = document.fittingSize.height
        let y = max(height - visible, 0)
        transcript.contentView.scroll(to: NSPoint(x: 0, y: y))
        transcript.reflectScrolledClipView(transcript.contentView)
        followingLiveEnd = true
    }

}
