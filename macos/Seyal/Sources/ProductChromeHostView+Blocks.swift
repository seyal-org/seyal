import AppKit

enum HistoryRangeRouting {
    enum Destination: Equatable {
        case primary
        case secondary(UInt64)
        case primaryAndSecondary(UInt64)
        case discard
    }

    static func destination(
        sourceDisplayHandle: UInt64,
        primaryDisplayHandle: UInt64,
        secondaryDisplayHandles: Set<UInt64>
    ) -> Destination {
        if sourceDisplayHandle == 0 {
            return secondaryDisplayHandles.isEmpty ? .primary : .discard
        }
        // In multipane Flow, PaneLayoutView installs a durable
        // SecondaryLivePaneHost for every visible leaf, including the focused
        // pane. Its handle is also `primaryDisplayHandle` because that pane
        // owns input, but its Block cards live in the per-pane host. Prefer
        // the exact host before falling back to the single-pane transcript.
        if secondaryDisplayHandles.contains(sourceDisplayHandle),
           sourceDisplayHandle == primaryDisplayHandle,
           primaryDisplayHandle != 0
        {
            return .primaryAndSecondary(sourceDisplayHandle)
        }
        if secondaryDisplayHandles.contains(sourceDisplayHandle) {
            return .secondary(sourceDisplayHandle)
        }
        if sourceDisplayHandle == primaryDisplayHandle, primaryDisplayHandle != 0 {
            return .primary
        }
        return .discard
    }
}

@MainActor
extension ProductChromeHostView {
    private var activeBlockDisplayHandle: UInt64 {
        let bridge = pane.inputSurface.bridge
        let focusedHandle = bridge?.windowFocusedDisplayHandle ?? 0
        return focusedHandle != 0 ? focusedHandle : bridge?.clientHandle ?? 0
    }

    func setupTranscriptTopSpacer() {
        transcriptTopSpacer.translatesAutoresizingMaskIntoConstraints = false
        transcriptTopSpacerHeight = transcriptTopSpacer.heightAnchor.constraint(equalToConstant: 0)
        transcriptTopSpacerHeight.isActive = true
        blocks.addArrangedSubview(transcriptTopSpacer)
    }

    func setupComposerDockConstraints() {
        composerDockConstraints = [
            composer.leadingAnchor.constraint(equalTo: liveContent.leadingAnchor, constant: 24),
            composer.trailingAnchor.constraint(equalTo: liveContent.trailingAnchor, constant: -24),
            composer.topAnchor.constraint(equalTo: sessionBootstrap.bottomAnchor),
            composer.bottomAnchor.constraint(equalTo: liveContent.bottomAnchor, constant: -16),
        ]
    }

    func installTimelineChangeHandler() {
        pane.inputSurface.onTimelineChanged = { [weak self] in
            self?.projectRuntimeBlocks()
            self?.reconcileChrome()
            self?.refreshRunningBlockOutput()
        }
    }

    func installComposerStatusHandler() {
        pane.inputSurface.onComposerStatusChanged = { [weak self] status in
            guard let self else { return false }
            let blockID = status.eligibility == 4 && status.targetMode == 2
                ? status.targetBlockID : 0
            self.relayTarget = (blockID, status.targetGeneration, status.targetMode)
            let accepted = self.relayComposerStatus(status)
            if accepted {
                self.reconcileChrome()
            }
            return accepted
        }
    }

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
    @discardableResult
    func relayComposerStatus(_ status: NativeComposerStatus) -> Bool {
        let snapshot = seyal_app_snapshot(pane.appHandle)
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_APPLY_COMPOSER_STATUS.rawValue)
        action.applySnapshotFence(snapshot)
        action.reserved = UInt32(status.eligibility)
        action.target_execution_lo = status.revision
        return seyal_app_apply(pane.appHandle, &action) == 0
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
            if mode == .tui {
                pane.inputSurface.clearRawScrollbackFollowTail()
            }
            pane.inputSurface.applyRendererPresentation(.fullPane(mode))
            pane.inputSurface.setLiveTailBlocks([:])
            pane.inputSurface.removeTranscriptRegions(except: [])
            layoutSubtreeIfNeeded()
            pane.inputSurface.preserveRawScrollbackAfterPresentationChange(
                eligibility: snapshot.eligibility)
        } else {
            // Leaving Raw must drop presentation-local scroll state; otherwise
            // frame ticks keep painting u64::MAX history and wipe Block bodies.
            pane.inputSurface.clearRawScrollbackFollowTail()
            NSLayoutConstraint.deactivate(paneFillsCenter)
            NSLayoutConstraint.activate(paneFollowsTranscript)
            pane.inputSurface.applyRendererPresentation(.flow())
            layoutSubtreeIfNeeded()
            publishBlockOutputFrame()
        }
    }

    func rebuildBlocks() {
        let previousCards = blockCards
        blocks.arrangedSubviews
            .filter { $0 !== transcriptLeadingSpacer }
            .forEach { blocks.removeArrangedSubview($0) }
        blockCards.removeAll()
        let composer = seyal_app_composer(pane.appHandle)
        let count = Int(composer.block_count)
        let floorLo = composer.canvas_floor_block_id_lo
        let floorHi = composer.canvas_floor_block_id_hi
        let cellHeight = pane.inputSurface.terminalPresentationCellSize().height
        var retained = Set<UInt64>()
        // Visible canvas index (not timeline index) so AX ids stay dense after clear.
        var visibleIndex = 0
        for index in 0..<count {
            let row = seyal_app_block_row(pane.appHandle, UInt32(index))
            let blockID = row.id_lo
            // Keep timeline history for floor-hidden Blocks so shell-exit can
            // re-reveal retained bodies. Omitting the card must not prune
            // historyRanges / pending history requests (#1379 clear→exit).
            if blockID != 0 {
                retained.insert(blockID)
            }
            // Rust owns the clear canvas floor; Swift only omits cards at/below
            // it so local `clear` leaves a fully empty live canvas (no clear
            // card). Defense for #1398: never omit a Local Running host if a
            // floor is present (session clear must not floor; host must stay).
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
            let projection = seyal_app_block_projection(pane.appHandle, UInt32(index))
            let title = productChromeCopyUTF8(row.title, row.title_len) ?? "command"
            let detail = productChromeCopyUTF8(row.detail, row.detail_len) ?? ""
            let promptRow = seyal_app_copy(pane.appHandle, UInt16(SEYAL_APP_COPY_BLOCK_PROMPT))
            let prompt = productChromeCopyUTF8(promptRow.title, promptRow.title_len) ?? "$"
            let lines = outputLineCount(projection: projection)
            let card = previousCards[blockID] ?? CommandBlockView(
                prompt: prompt,
                title: title,
                detail: detail,
                state: row.flags & UInt16(SEYAL_APP_BLOCK_STATE_MASK),
                cellHeight: cellHeight,
                lines: lines,
                session: row.flags & UInt16(SEYAL_APP_BLOCK_SESSION) != 0
            )
            card.updatePresentation(
                prompt: prompt,
                title: title,
                detail: detail,
                state: row.flags & UInt16(SEYAL_APP_BLOCK_STATE_MASK),
                session: row.flags & UInt16(SEYAL_APP_BLOCK_SESSION) != 0
            )
            card.setAccessibilityIdentifier("seyal-block-\(visibleIndex)")
            card.setBodyAccessibilityIdentifier("seyal-block-\(visibleIndex)-body")
            visibleIndex += 1
            if blockID != 0,
               let retainedRange = pane.inputSurface.retainedHistoryRange(
                   blockID: blockID,
                   displayHandle: activeBlockDisplayHandle
               ) {
                card.setBodyTranscript(retainedRange.transcriptText)
            }
            card.isSelected = row.flags & UInt16(SEYAL_APP_BLOCK_SELECTED) != 0
            let idLo = row.id_lo
            let idHi = row.id_hi
            card.onSelect = { [weak self] selected in
                self?.selectBlock(idLo: idLo, idHi: idHi, deselect: selected)
            }
            blocks.addArrangedSubview(card)
            if blockID != 0 {
                blockCards[blockID] = card
                applyBlockOutputProjection(blockID: blockID, projection: projection)
            }
        }
        for (blockID, card) in previousCards where blockCards[blockID] == nil {
            card.removeFromSuperview()
        }
        pane.inputSurface.discardHistoryRequests(except: retained)
        // Prune retained history to the timeline Block set (including
        // floor-hidden cards) so New shell / eviction cannot paint a prior
        // execution's cells under a reused Block id, while clear→exit can
        // still re-materialize pre-clear bodies.
        pane.inputSurface.removeTranscriptRegions(except: retained)
        syncTranscriptDocumentSize()
        publishBlockOutputFrame()
        publishLiveTailBlocks()
        if visibleIndex > lastBlockCount, followingLiveEnd {
            scrollTranscriptToLiveEnd()
        }
        lastBlockCount = visibleIndex
        applyComposerPlacement()
        // clear→exit: card heights change after live-tail drop and History
        // settle; one async republish re-encodes retained history against the
        // final AppKit clips so prior origins cannot stick to earlier cards.
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            self.layoutSubtreeIfNeeded()
            self.syncTranscriptDocumentSize()
            self.publishBlockOutputFrame()
        }
    }

    func applyComposerPlacement() {
        let shouldRestoreEditorFocus = composer.editorIsFirstResponder
        let snapshot = seyal_app_snapshot(pane.appHandle)
        let direct = snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue)
            || snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue)
        let target = relayTarget.mode == 2 && relayTarget.generation != 0
            ? blockCards[relayTarget.blockID] : nil
        guard !direct, relayTarget.blockID != 0, let target else {
            setRelayTranscriptViewportBottomAnchored(false)
            if composerInlineBlockID != 0 {
                composerInlineBlockID = 0
                composer.setInlineRelayPresentation(false)
                composer.translatesAutoresizingMaskIntoConstraints = false
                NSLayoutConstraint.activate(composerDockConstraints)
                if shouldRestoreEditorFocus { composer.focusEditor() }
            }
            return
        }
        // Relay positions the Pane composer over the running prompt. Keep the
        // transcript as the full-height Pane viewport so the short live Block
        // and its prompt stay bottom-aligned like Warp's live input surface.
        setRelayTranscriptViewportBottomAnchored(true)
        if composerInlineBlockID == relayTarget.blockID {
            positionInlineComposerAtCurrentCursor()
            return
        }
        NSLayoutConstraint.deactivate(composerDockConstraints)
        composer.setInlineRelayPresentation(true)
        // Keep the one Pane composer above the full-pane Metal surface. A
        // Block is an output/history projection and is underneath that surface;
        // reparenting the editor into its body makes AX see input that pixels
        // cannot paint. The converted cursor rect follows transcript scrolling.
        composer.translatesAutoresizingMaskIntoConstraints = true
        liveContent.addSubview(composer, positioned: .above, relativeTo: pane)
        composerInlineBlockID = relayTarget.blockID
        layoutSubtreeIfNeeded()
        positionInlineComposerAtCurrentCursor()
        syncTranscriptDocumentSize()
        if followingLiveEnd {
            scrollTranscriptToLiveEnd()
        }
        positionInlineComposerAtCurrentCursor()
        if shouldRestoreEditorFocus { composer.focusEditor() }
    }

    func setRelayTranscriptViewportBottomAnchored(_ anchored: Bool) {
        if anchored {
            let constraint = transcriptRelayBottomConstraint ?? {
                let constraint = transcript.bottomAnchor.constraint(
                    equalTo: liveContent.bottomAnchor,
                    constant: -16
                )
                constraint.identifier = "seyal-flow-relay-transcript-bottom-inset"
                transcriptRelayBottomConstraint = constraint
                return constraint
            }()
            if !constraint.isActive {
                constraint.isActive = true
            }
        } else if let constraint = transcriptRelayBottomConstraint, constraint.isActive {
            constraint.isActive = false
        }
    }

    func positionInlineComposerAtCurrentCursor() {
        guard relayTarget.mode == 2,
              relayTarget.blockID != 0,
              let card = blockCards[relayTarget.blockID],
              let frame = pane.inputSurface.terminalCurrentFrame()
        else { return }
        let composer = seyal_app_composer(pane.appHandle)
        for index in 0..<Int(composer.block_count) {
            let row = seyal_app_block_row(pane.appHandle, UInt32(index))
            guard row.id_lo == relayTarget.blockID else { continue }
            let projection = seyal_app_block_projection(pane.appHandle, UInt32(index))
            guard projection.kind == UInt16(SEYAL_APP_BLOCK_PROJECTION_PRIMARY_CLIP) else { return }
            let rowOffset = Int(frame.cursor_row) - Int(projection.reserved0)
            guard rowOffset >= 0, rowOffset < Int(projection.reserved1) else { return }
            let cellSize = pane.inputSurface.terminalPresentationCellSize()
            let column = max(Int(frame.cursor_column), 0)
            let localY = card.body.isFlipped
                ? CGFloat(rowOffset) * cellSize.height
                : card.body.bounds.maxY - CGFloat(rowOffset + 1) * cellSize.height
            let cursorCell = NSRect(
                x: CGFloat(column) * cellSize.width,
                y: localY,
                width: cellSize.width,
                height: cellSize.height
            )
            let cursorInLiveContent = card.body.convert(cursorCell, to: liveContent)
            let composerHeight: CGFloat = 24
            let origin = NSPoint(
                x: cursorInLiveContent.minX,
                y: cursorInLiveContent.midY - composerHeight / 2
            )
            self.composer.setFrameSize(NSSize(
                width: max(liveContent.bounds.maxX - origin.x - 20, cellSize.width),
                height: composerHeight
            ))
            self.composer.setFrameOrigin(origin)
            return
        }
    }

    func outputLineCount(projection: SeyalAppBlockProjection) -> Int {
        switch projection.kind {
        case UInt16(SEYAL_APP_BLOCK_PROJECTION_HISTORY):
            guard projection.start_line > 0, projection.end_line >= projection.start_line else {
                return 1
            }
            return Int(min(projection.end_line - projection.start_line + 1, 512))
        case UInt16(SEYAL_APP_BLOCK_PROJECTION_PRIMARY_CLIP):
            // Rust-owned row slice height (not the full prepared viewport).
            return max(Int(projection.reserved1), 1)
        default:
            return 1
        }
    }

    func refreshRunningBlockOutput() {
        let snapshot = seyal_app_snapshot(pane.appHandle)
        if snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue)
            || snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue)
        {
            return
        }
        publishLiveTailBlocks()
        // Damage-driven primary clips refresh from Candidate-D frame updates.
        // Re-publish geometry so Block body height tracks the prepared rows.
        let composer = seyal_app_composer(pane.appHandle)
        let cellHeight = pane.inputSurface.terminalPresentationCellSize().height
        for index in 0..<Int(composer.block_count) {
            let row = seyal_app_block_row(pane.appHandle, UInt32(index))
            let projection = seyal_app_block_projection(pane.appHandle, UInt32(index))
            guard row.id_lo != 0,
                  projection.kind == UInt16(SEYAL_APP_BLOCK_PROJECTION_PRIMARY_CLIP),
                  let card = blockCards[row.id_lo]
            else { continue }
            card.setOutputLines(outputLineCount(projection: projection), cellHeight: cellHeight)
        }
        syncTranscriptDocumentSize()
        positionInlineComposerAtCurrentCursor()
        publishBlockOutputFrame()
    }

    /// Completed Blocks request their trusted history span. Running Blocks
    /// draw the prepared primary frame; the host never invents a history end.
    func applyBlockOutputProjection(blockID: UInt64, projection: SeyalAppBlockProjection) {
        guard projection.kind == UInt16(SEYAL_APP_BLOCK_PROJECTION_HISTORY),
              projection.start_line > 0,
              projection.end_line >= projection.start_line
        else { return }
        _ = pane.inputSurface.requestHistoryRange(
            startLine: projection.start_line,
            endLine: projection.end_line,
            blockID: blockID
        )
    }

    func publishLiveTailBlocks() {
        let snapshot = seyal_app_snapshot(pane.appHandle)
        if snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue)
            || snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue)
        {
            pane.inputSurface.setLiveTailBlocks([:])
            return
        }
        let composer = seyal_app_composer(pane.appHandle)
        var live: [UInt64: LiveTailClip] = [:]
        for index in 0..<Int(composer.block_count) {
            let row = seyal_app_block_row(pane.appHandle, UInt32(index))
            let projection = seyal_app_block_projection(pane.appHandle, UInt32(index))
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
        pane.inputSurface.setLiveTailBlocks(live)
    }

    func applyHistoryRange(_ range: NativeHistoryRange) {
        let bridge = pane.inputSurface.bridge
        let primaryDisplayHandle = activeBlockDisplayHandle
        let secondaryDisplayHandles = Set(centerColumn.secondaryHosts.map(\.displayHandle))
        switch HistoryRangeRouting.destination(
            sourceDisplayHandle: range.sourceDisplayHandle,
            primaryDisplayHandle: primaryDisplayHandle,
            secondaryDisplayHandles: secondaryDisplayHandles
        ) {
        case .secondary(let handle):
            guard let host = centerColumn.secondaryHost(displayHandle: handle) else { return }
            host.applyHistoryRange(range)
            return
        case .primaryAndSecondary(let handle):
            // Multipane keeps a durable per-pane Block host while the focused
            // input surface also paints the primary transcript. Feed both
            // projections for the same Controller so a focus swap cannot
            // reveal a host whose history buffer never received this range.
            guard let host = centerColumn.secondaryHost(displayHandle: handle) else { return }
            host.applyHistoryRange(range)
        case .discard:
            return
        case .primary:
            break
        }
        pane.inputSurface.retainHistoryRange(range)
        let cellHeight = pane.inputSurface.terminalPresentationCellSize().height
        guard range.sourceDisplayHandle == 0
            || range.sourceDisplayHandle == activeBlockDisplayHandle
        else { return }
        if let card = blockCards[range.blockID] {
            card.setOutputLines(max(range.rows.count, 1), cellHeight: cellHeight)
            card.setBodyTranscript(range.transcriptText)
        }
        // Paint with Auto Layout sizes first, then expand the document frame.
        // setFrameSize-before-publish can yield zero-height clips that wipe Metal
        // history and leave titles-only white bodies (#1379).
        layoutSubtreeIfNeeded()
        publishBlockOutputFrame()
        syncTranscriptDocumentSize()
        publishBlockOutputFrame()
        // History replies arrive after the initial live-end scroll and can grow
        // earlier cards. Keep following only when the user was already at the
        // live end so the newly submitted Block stays hittable.
        if followingLiveEnd {
            scrollTranscriptToLiveEnd()
        }
    }

    /// AppKit scrolls with `documentView.frame`; `blocks.fittingSize` is the
    /// authoritative content height once `layoutSubtreeIfNeeded()` has run
    /// (every call site updates card heights before calling this). When at
    /// least one real Block card is present, the document floors at the
    /// scroll viewport height — never below it — so sparse Flow content
    /// never under-fills the visible transcript area, while a
    /// viewport-sized leading spacer (not a Block card) absorbs that slack
    /// and pushes real Blocks down to hug the composer, Warp-style (#1426
    /// follow-up). A genuinely empty canvas (no cards at all) must still
    /// collapse toward zero rather than hold a blank scrollable viewport
    /// (#1424); the viewport floor is skipped in that case so a cleared
    /// transcript never regains a preserved/blank scroll extent. This
    /// replaced an older "expand, never shrink" policy (#1379) that also
    /// left a permanent dead zone below cards once content shrank after
    /// growing tall; the viewport floor keeps the same anti-collapse intent
    /// for non-empty content without that stale-gap regression.
    func syncTranscriptDocumentSize() {
        layoutSubtreeIfNeeded()
        let fitted = blocks.fittingSize
        let contentHeight = max(fitted.height - transcriptLeadingSpacerHeight.constant, 0)
        let hasCards = !blockCards.isEmpty
        let viewportHeight = hasCards ? transcript.contentView.bounds.height : 0
        let width = max(fitted.width, transcript.contentView.bounds.width, blocks.frame.width)
        let height = max(contentHeight, viewportHeight, 1)
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

    func publishBlockOutputFrame() {
        let snapshot = seyal_app_snapshot(pane.appHandle)
        let direct = snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue)
            || snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue)
        guard !direct else { return }
        let surface = pane.inputSurface
        var regions = collectBlockOutputRegions(surface: surface)
        // Cards exist but clips are still zero-sized: retry once after layout.
        // Never publish an empty region set while cards remain — that clears
        // retained history paint (#1379 titles-only white bodies). After clear
        // shrinks a tall PRIMARY_CLIP body, a zero-clip publish would also leave
        // prior history origins stuck over earlier cards (clear→exit mispaint).
        if regions.isEmpty, !blockCards.isEmpty {
            layoutSubtreeIfNeeded()
            regions = collectBlockOutputRegions(surface: surface)
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
        // Scroll / clip republish must not propose geometry; refresh the
        // read-only AX probe so headed tests can observe unchanged PTY size.
        surface.refreshRecoveryAccessibilityValue()
    }

    private func collectBlockOutputRegions(surface: InteractiveMetalSurfaceView) -> [NativeTranscriptRegion] {
        var regions: [NativeTranscriptRegion] = []
        for (blockID, card) in blockCards {
            let clip = card.body.convert(card.body.bounds, to: surface)
            guard clip.width > 0, clip.height > 0 else { continue }
            regions.append(NativeTranscriptRegion(
                id: blockID,
                displayHandle: activeBlockDisplayHandle,
                origin: clip.origin,
                clip: clip,
                suppressesCursor: blockID == relayTarget.blockID && relayTarget.mode == 2
            ))
        }
        return regions
    }

    /// NSScrollView scrolls against `documentView.frame`. `syncTranscriptDocumentSize`
    /// expands that frame and never shrinks it, so `fittingSize` can lag below the
    /// frame after a tall live-tail settles. Live-end math must use the frame or
    /// follow sticks true and every history republish snaps the user back (#1379).
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
