import AppKit

@MainActor
extension SecondaryLivePaneHost {
    func rebuildBlocks() {
        let eligibility = seyal_app_eligibility_for_pane(appHandle, paneLo, paneHi)
        if eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue)
            || eligibility == UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue)
        {
            clearPreservedDocumentHeight()
            blocks.arrangedSubviews
                .filter { $0 !== transcriptLeadingSpacer }
                .forEach { $0.removeFromSuperview() }
            blockCards.removeAll()
            lastBlockCount = 0
            return
        }
        // Preserve scroll across projection rebuilds so focus/reconcile churn
        // does not jump the transcript (#1424).
        let savedOffsetY = transcript.contentView.bounds.origin.y
        let savedFollowing = followingLiveEnd
        let savedDocumentHeight = transcriptDocumentScrollHeight()
        blocks.arrangedSubviews
            .filter { $0 !== transcriptLeadingSpacer }
            .forEach { $0.removeFromSuperview() }
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
            if blockID != 0,
               let retainedRange = surface.retainedHistoryRange(
                   blockID: blockID,
                   displayHandle: displayHandle
               ) {
                card.setBodyTranscript(retainedRange.transcriptText)
            }
            blocks.addArrangedSubview(card)
            if blockID != 0 {
                blockCards[blockID] = card
                applyBlockOutputProjection(blockID: blockID, projection: projection)
            }
        }
        // Bridgeless surfaces: discard through the borrowed window bridge,
        // scoped to this pane's own handle (#1426 residual).
        historyBridge?.discardHistoryRequests(except: retained, forDisplayHandle: displayHandle)
        surface.removeTranscriptRegions(except: retained)
        syncTranscriptDocumentSize()
        publishLiveTailBlocks()
        publishBlockOutputFrame()
        if visibleIndex == 0 {
            // Empty/cleared canvas: never leave a blank scrollable transcript
            // from a prior preserved document height (#1424).
            clearPreservedDocumentHeight()
            syncTranscriptDocumentSize()
            setTranscriptDocumentOffsetY(0)
            followingLiveEnd = true
            lastBlockCount = 0
            return
        }
        let addedBlock = visibleIndex > lastBlockCount
        if addedBlock && savedFollowing {
            clearPreservedDocumentHeight()
            scrollTranscriptToLiveEnd()
        } else {
            // New Blocks must not pull a reader back to the live end. Keep the
            // prior document extent and offset when the user has scrolled away,
            // including while a stream or registry poll adds output (#1424).
            let currentContentHeight = transcriptCardsContentHeight()
            let viewportFloor = blockCards.isEmpty ? 0 : transcript.contentView.bounds.height
            let currentDocumentMinimumHeight = max(max(currentContentHeight, viewportFloor), 1)
            if !savedFollowing, savedDocumentHeight > currentDocumentMinimumHeight + 0.5 {
                preserveDocumentHeight(savedDocumentHeight)
            } else {
                clearPreservedDocumentHeight()
            }
            setTranscriptDocumentOffsetY(savedOffsetY)
            followingLiveEnd = savedFollowing
        }
        lastBlockCount = visibleIndex
        if addedBlock {
            DispatchQueue.main.async { [weak self] in
                guard let self else { return }
                self.layoutSubtreeIfNeeded()
                self.syncTranscriptDocumentSize()
                if self.followingLiveEnd {
                    self.scrollTranscriptToLiveEnd()
                }
                self.publishBlockOutputFrame()
            }
        }
    }

    func preserveDocumentHeight(_ height: CGFloat) {
        clearPreservedDocumentHeight()
        let constraint = blocks.heightAnchor.constraint(greaterThanOrEqualToConstant: height)
        constraint.priority = .defaultHigh
        constraint.isActive = true
        preservedDocumentHeightConstraint = constraint
        syncTranscriptDocumentSize()
    }

    func clearPreservedDocumentHeight() {
        preservedDocumentHeightConstraint?.isActive = false
        preservedDocumentHeightConstraint = nil
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
            card.setOutputLines(outputLineCount(projection: projection), cellHeight: cellHeight)
            if let retainedRange = surface.retainedHistoryRange(
                blockID: row.id_lo,
                displayHandle: displayHandle
            ) {
                card.setBodyTranscript(retainedRange.transcriptText)
            }
            applyBlockOutputProjection(blockID: row.id_lo, projection: projection)
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
        default:
            return 1
        }
    }

    func applyBlockOutputProjection(blockID: UInt64, projection: SeyalAppBlockProjection) {
        guard projection.kind == UInt16(SEYAL_APP_BLOCK_PROJECTION_HISTORY),
              projection.start_line > 0,
              projection.end_line >= projection.start_line,
              displayHandle != 0
        else { return }
        // Surfaces intentionally have bridge == nil; borrow the window bridge
        // and address this pane's Controller (#1424).
        _ = historyBridge?.requestHistoryRange(
            forDisplayHandle: displayHandle,
            startLine: projection.start_line,
            endLine: projection.end_line,
            blockID: blockID
        )
    }

    /// Deliver a completed history range into this leaf's Metal surface + card.
    func applyHistoryRange(_ range: NativeHistoryRange) {
        guard range.sourceDisplayHandle == 0 || range.sourceDisplayHandle == displayHandle else {
            return
        }
      guard !range.rows.isEmpty else { return }
        surface.retainHistoryRange(range)
        let cellHeight = surface.terminalPresentationCellSize().height
        if let card = blockCards[range.blockID] {
            card.setOutputLines(max(range.rows.count, 1), cellHeight: cellHeight)
            card.setBodyTranscript(range.transcriptText)
        }
        layoutSubtreeIfNeeded()
        publishBlockOutputFrame()
        syncTranscriptDocumentSize()
        publishBlockOutputFrame()
        if followingLiveEnd {
            scrollTranscriptToLiveEnd()
        }
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
            regions.append(
                NativeTranscriptRegion(
                    id: blockID,
                    displayHandle: displayHandle,
                    origin: clip.origin,
                    clip: clip
                )
            )
        }
        return regions
    }

    /// Mirrors `ProductChromeHostView+Blocks.syncTranscriptDocumentSize`: when
    /// at least one real Block card is present, the document floors at the
    /// scroll viewport height so sparse Flow content never under-fills the
    /// visible transcript, while a viewport-sized leading spacer (not a
    /// Block card) absorbs that slack and pushes real Blocks down to hug the
    /// composer, Warp-style (#1426 follow-up). A genuinely empty canvas (no
    /// cards at all) must still collapse toward zero rather than hold a
    /// blank scrollable viewport (#1424); the viewport floor is skipped in
    /// that case. This replaced an older "expand, never shrink" policy that
    /// left a permanent dead zone below cards once content shrank after
    /// growing tall.
    func syncTranscriptDocumentSize() {
        let shouldFollowLiveEnd = followingLiveEnd
        isProgrammaticTranscriptScroll = true
        defer {
            isProgrammaticTranscriptScroll = false
            if shouldFollowLiveEnd {
                scrollTranscriptToLiveEnd()
            }
        }
        layoutSubtreeIfNeeded()
        let fitted = blocks.fittingSize
        let hasCards = !blockCards.isEmpty
        let contentHeight = transcriptCardsContentHeight()
        let viewportHeight = hasCards ? transcript.contentView.bounds.height : 0
        let width = max(fitted.width, transcript.contentView.bounds.width, blocks.frame.width)
        let preservedHeight = preservedDocumentHeightConstraint?.constant ?? 0
        let height = max(max(contentHeight, viewportHeight), max(preservedHeight, 1))
        let spacerHeight = max(height - contentHeight, 0)
        if abs(transcriptLeadingSpacerHeight.constant - spacerHeight) > 0.5 {
            transcriptLeadingSpacerHeight.constant = spacerHeight
            layoutSubtreeIfNeeded()
        }
        if abs(blocks.frame.width - width) > 0.5 || abs(blocks.frame.height - height) > 0.5 {
            blocks.setFrameSize(NSSize(width: width, height: height))
            transcript.reflectScrolledClipView(transcript.contentView)
            layoutSubtreeIfNeeded()
        }
    }

    /// Measure the current cards rather than subtracting the spacer from
    /// `NSStackView.fittingSize`, which can retain the prior document frame
    /// while Blocks are rebuilt after focus/projection changes (#1426).
    func transcriptCardsContentHeight() -> CGFloat {
        let cards = blocks.arrangedSubviews.filter { $0 !== transcriptLeadingSpacer }
        guard !cards.isEmpty else { return 0 }
        let cardHeight = cards.reduce(CGFloat.zero) { total, view in
            total + max(view.fittingSize.height, view.bounds.height)
        }
        return cardHeight + CGFloat(max(cards.count - 1, 0)) * blocks.spacing
    }

    func transcriptDocumentScrollHeight() -> CGFloat {
        let document = transcript.documentView ?? blocks
        return max(document.frame.height, document.fittingSize.height, 1)
    }

    /// Stretch the transcript document so scroll-offset retention can be tested
    /// without depending on Runtime Block projection content.
    func ensureTranscriptScrollableForTests(minimumHeight: CGFloat = 2400) {
        clearPreservedDocumentHeight()
        let marker = NSUserInterfaceItemIdentifier("seyal-test-scroll-spacer")
        blocks.arrangedSubviews
            .filter { $0.identifier == marker }
            .forEach { blocks.removeArrangedSubview($0); $0.removeFromSuperview() }
        let spacer = NSView(frame: .zero)
        spacer.identifier = marker
        spacer.translatesAutoresizingMaskIntoConstraints = false
        blocks.addArrangedSubview(spacer)
        NSLayoutConstraint.activate([
            spacer.heightAnchor.constraint(equalToConstant: minimumHeight),
            spacer.widthAnchor.constraint(greaterThanOrEqualToConstant: 1),
        ])
        syncTranscriptDocumentSize()
        preserveDocumentHeight(max(minimumHeight, transcriptDocumentScrollHeight()))
        layoutSubtreeIfNeeded()
        followingLiveEnd = false
    }

    func isNearLiveEnd(tolerance: CGFloat = 24) -> Bool {
        let visible = transcript.contentView.bounds
        let maxY = max(transcriptDocumentScrollHeight() - visible.height, 0)
        return visible.origin.y >= maxY - tolerance
    }

    func scrollTranscriptToLiveEnd() {
        isProgrammaticTranscriptScroll = true
        defer { isProgrammaticTranscriptScroll = false }
        let visible = transcript.contentView.bounds
        let maxY = max(transcriptDocumentScrollHeight() - visible.height, 0)
        transcript.contentView.scroll(to: NSPoint(x: visible.origin.x, y: maxY))
        transcript.reflectScrolledClipView(transcript.contentView)
        followingLiveEnd = true
    }
}
