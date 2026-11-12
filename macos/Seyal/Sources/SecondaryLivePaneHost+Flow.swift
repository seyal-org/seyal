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
            syncTranscriptDocumentSize()
            setTranscriptDocumentOffsetY(0)
            followingLiveEnd = true
            lastBlockCount = 0
            rememberTranscriptScrollState()
            return
        }
        // Preserve scroll across projection rebuilds so focus/reconcile churn
        // does not jump the transcript (#1424).
        let savedScrollState: PaneTranscriptScrollState = {
            if restoresScrollForDisplayHandleChange {
                restoresScrollForDisplayHandleChange = false
                return transcriptScrollStates[displayHandle]
                    ?? PaneTranscriptScrollState(offsetY: 0, documentHeight: 0, followsLiveEnd: true)
            }
            return currentTranscriptScrollState()
        }()
        let savedOffsetY = savedScrollState.offsetY
        let savedFollowing = savedScrollState.followsLiveEnd
        let savedDocumentHeight = savedScrollState.documentHeight
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
            let lines = outputLineCount(projection: projection)
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
                    lines: lines,
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
            card.setOutputLines(lines, cellHeight: cellHeight)
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
            if blockID != 0,
               let retainedRange = surface.retainedHistoryRange(
                   blockID: blockID,
                   displayHandle: displayHandle
               ) {
                card.setBodyTranscript(retainedRange.transcriptText)
                card.setOutputLines(
                    CommandBlockView.visibleOutputLineCount(retainedRange.transcriptText),
                    cellHeight: surface.terminalPresentationCellSize().height
                )
            }
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
        historyProjectionRequests.retain(blockIDs: retained, forDisplayHandle: displayHandle)
        historyBridge?.discardHistoryRequests(except: retained, forDisplayHandle: displayHandle)
        surface.removeTranscriptRegions(except: retained)
        followingLiveEnd = savedFollowing
        syncTranscriptDocumentSize()
        publishLiveTailBlocks()
        publishBlockOutputFrame()
        if visibleIndex == 0 {
            // Empty/cleared canvas: never leave a blank scrollable transcript
            // from a prior preserved document height or test/orphan spacers
            // still arranged in the stack (#1424).
            clearPreservedDocumentHeight()
            blocks.arrangedSubviews
                .filter { $0 !== transcriptLeadingSpacer }
                .forEach {
                    blocks.removeArrangedSubview($0)
                    $0.removeFromSuperview()
                }
            syncTranscriptDocumentSize()
            setTranscriptDocumentOffsetY(0)
            followingLiveEnd = true
            lastBlockCount = 0
            rememberTranscriptScrollState()
            return
        }
        let addedBlock = visibleIndex > lastBlockCount
        if savedFollowing {
            // A pane split can shrink the transcript viewport without adding
            // a Block. Keep a reader who was following live output at the new
            // live end after that resize instead of restoring the old host's
            // absolute scroll offset into the shortened viewport.
            clearPreservedDocumentHeight()
            scrollTranscriptToLiveEnd()
        } else {
            // New Blocks must not pull a reader back to the live end. Keep the
            // prior document extent and offset when the user has scrolled away,
            // including while a stream or registry poll adds output (#1424).
            let currentContentHeight = transcriptCardsContentHeight()
            let viewportFloor = blockCards.isEmpty ? 0 : transcript.contentView.bounds.height
            let currentDocumentMinimumHeight = max(max(currentContentHeight, viewportFloor), 1)
            if savedDocumentHeight > currentDocumentMinimumHeight + 0.5 {
                preserveDocumentHeight(savedDocumentHeight)
            } else {
                clearPreservedDocumentHeight()
            }
            setTranscriptDocumentOffsetY(savedOffsetY)
            followingLiveEnd = savedFollowing
        }
        lastBlockCount = visibleIndex
        rememberTranscriptScrollState()
        if addedBlock {
            DispatchQueue.main.async { [weak self] in
                guard let self else { return }
                self.layoutSubtreeIfNeeded()
                self.syncTranscriptDocumentSize()
                if self.followingLiveEnd {
                    self.scrollTranscriptToLiveEnd()
                }
                self.publishBlockOutputFrame()
                self.rememberTranscriptScrollState()
            }
        }
    }

    func currentTranscriptScrollState() -> PaneTranscriptScrollState {
        PaneTranscriptScrollState(
            offsetY: transcript.contentView.bounds.origin.y,
            documentHeight: transcriptDocumentScrollHeight(),
            followsLiveEnd: followingLiveEnd
        )
    }

    func rememberTranscriptScrollState() {
        guard displayHandle != 0, !restoresScrollForDisplayHandleChange else { return }
        transcriptScrollStates[displayHandle] = currentTranscriptScrollState()
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
                card.setOutputLines(
                    CommandBlockView.visibleOutputLineCount(retainedRange.transcriptText),
                    cellHeight: cellHeight
                )
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
        case UInt16(SEYAL_APP_BLOCK_PROJECTION_PRIMARY_CLIP),
             UInt16(SEYAL_APP_BLOCK_PROJECTION_NATIVE_REGIONS):
            return max(Int(projection.reserved1), 1)
        default:
            return 1
        }
    }

    /// Sealed NATIVE_REGIONS: reserved0 == UInt16.max (not a LiveTail clip).
    private func isNativeSealedProjection(_ projection: SeyalAppBlockProjection) -> Bool {
        projection.kind == UInt16(SEYAL_APP_BLOCK_PROJECTION_NATIVE_REGIONS)
            && projection.reserved0 == UInt16.max
            && projection.reserved1 > 0
    }

    func applyBlockOutputProjection(blockID: UInt64, projection: SeyalAppBlockProjection) {
        // On-window NATIVE_REGIONS paints via publishLiveTailBlocks. Sealed
        // NATIVE fetches RegionId-owned content by block_id (Runtime ignores
        // LineId span authority). Legacy HISTORY LineId reconstruction is retired
        // for native Blocks (#1484).
        guard displayHandle != 0 else { return }
        if isNativeSealedProjection(projection) {
            let key = HistoryProjectionRequestKey(
                displayHandle: displayHandle,
                blockID: blockID,
                startLine: 0,
                endLine: 0
            )
            guard historyProjectionRequests.begin(key) else { return }
            // start/end are ignored when Runtime resolves sealed RegionId cells.
            let result = historyBridge?.requestHistoryRange(
                forDisplayHandle: displayHandle,
                startLine: 1,
                endLine: 1,
                blockID: blockID
            ) ?? -10
            if result != 0 {
                historyProjectionRequests.retry(key)
            }
            return
        }
        guard projection.kind == UInt16(SEYAL_APP_BLOCK_PROJECTION_HISTORY),
              projection.start_line > 0,
              projection.end_line >= projection.start_line
        else { return }
        // Surfaces intentionally have bridge == nil; borrow the window bridge
        // and address this pane's Controller (#1424).
        let key = HistoryProjectionRequestKey(
            displayHandle: displayHandle,
            blockID: blockID,
            startLine: projection.start_line,
            endLine: projection.end_line
        )
        guard historyProjectionRequests.begin(key) else { return }
        let result = historyBridge?.requestHistoryRange(
            forDisplayHandle: displayHandle,
            startLine: projection.start_line,
            endLine: projection.end_line,
            blockID: blockID
        ) ?? -10
        if result != 0 {
            historyProjectionRequests.retry(key)
        }
    }

    /// Deliver a completed history range into this leaf's Metal surface + card.
    func applyHistoryRange(_ range: NativeHistoryRange) {
        guard range.sourceDisplayHandle == 0 || range.sourceDisplayHandle == displayHandle else {
            return
        }
        let key = HistoryProjectionRequestKey(
            displayHandle: range.sourceDisplayHandle == 0 ? displayHandle : range.sourceDisplayHandle,
            blockID: range.blockID,
            startLine: range.startLine,
            endLine: range.endLine
        )
        guard !range.rows.isEmpty else {
            // Native sealed fetches may complete empty after eviction; settle
            // by block id so we do not hot-loop retries (#1484).
            historyProjectionRequests.complete(key)
            return
        }
        historyProjectionRequests.receive(key, status: range.status)
        surface.retainHistoryRange(range)
        let cellHeight = surface.terminalPresentationCellSize().height
        if let card = blockCards[range.blockID] {
            card.setBodyTranscript(range.transcriptText)
            card.setOutputLines(
                CommandBlockView.visibleOutputLineCount(range.transcriptText),
                cellHeight: cellHeight
            )
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
            // Sealed NATIVE (reserved0 == UInt16.max) paints via RegionId content
            // fetch — never LiveTail-clip the primary frame.
            let nativeSealed = projection.kind == UInt16(SEYAL_APP_BLOCK_PROJECTION_NATIVE_REGIONS)
                && projection.reserved0 == UInt16.max
            guard row.id_lo != 0,
                  projection.reserved1 > 0,
                  !nativeSealed,
                  projection.kind == UInt16(SEYAL_APP_BLOCK_PROJECTION_PRIMARY_CLIP)
                    || projection.kind == UInt16(SEYAL_APP_BLOCK_PROJECTION_NATIVE_REGIONS)
            else { continue }
            // PRIMARY_CLIP: startLine is ViewportLineId. On-window NATIVE_REGIONS:
            // header RegionId; firstRow/rowCount clip the primary frame.
            live[row.id_lo] = LiveTailClip(
                startLine: max(projection.start_line, 1),
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
        lastPublishedTranscriptRegions = regions
        transcriptFrameRevision &+= 1
        surface.setTranscriptFrame(
            NativeTranscriptFrame(
                revision: transcriptFrameRevision,
                regions: regions,
                surfaceIdentity: ObjectIdentifier(surface)
            )
        )
        if ProcessInfo.processInfo.environment["SEYAL_UI_TEST_FORWARD_RUNTIME_COMMAND"] == "1",
           lastVisibleHistoryPixelCount == 0,
           Date().timeIntervalSince(lastHistoryPixelSampleAt) >= 0.5
        {
            lastHistoryPixelSampleAt = Date()
            lastVisibleHistoryPixelCount = surface.inspectFlowPaint(sampleGPU: true).opaquePixelsInsideClips
        }
    }

    private func collectBlockOutputRegions() -> [NativeTranscriptRegion] {
        var regions: [NativeTranscriptRegion] = []
        for (blockID, card) in blockCards {
            let clip = card.body.convert(card.body.bounds, to: surface)
            guard clip.width > 0, clip.height > 0 else { continue }
            let historyRows: Range<Int>?
            let historyRequestID: UInt64
            let historyRevision: UInt64
            if let range = surface.retainedHistoryRange(
                blockID: blockID,
                displayHandle: displayHandle
            ) {
                let visibleBody = card.body.convert(surface.bounds, from: surface)
                    .intersection(card.body.bounds)
                let cellHeight = max(surface.terminalPresentationCellSize().height, 1)
                let firstVisibleRow = HistoryRenderWindow.firstVisibleRow(
                    documentBounds: card.body.bounds,
                    visibleBounds: visibleBody,
                    isFlipped: card.body.isFlipped,
                    cellHeight: cellHeight
                )
                let visibleRowCount = Int((visibleBody.height / cellHeight).rounded(.up))
                historyRows = HistoryRenderWindow(
                    totalRows: range.rows.count,
                    firstVisibleRow: firstVisibleRow,
                    visibleRowCount: visibleRowCount
                ).rows
                historyRequestID = range.requestID
                historyRevision = range.revision
            } else {
                historyRows = nil
                historyRequestID = 0
                historyRevision = 0
            }
            regions.append(
                NativeTranscriptRegion(
                    id: blockID,
                    displayHandle: displayHandle,
                    origin: clip.origin,
                    clip: clip,
                    historyRows: historyRows,
                    historyRequestID: historyRequestID,
                    historyRevision: historyRevision
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
        let cardHeight = transcriptCardsContentHeight()
        let rustRows = seyal_bridge_native_regions_total_height_for(displayHandle)
        let cellHeight = surface.terminalPresentationCellSize().height
        let rustFloor = rustRows > 0 ? CGFloat(rustRows) * cellHeight : 0
        // Rust indexed height is a document floor when native regions publish
        // (#1484 / ADR-009). Card chrome may still win so painted bodies are
        // never clipped by an under-counted row index. Cleared canvas (no
        // cards) collapses even if a stale Rust floor remains (#1424).
        let contentHeight: CGFloat = {
            if !hasCards { return 0 }
            return max(cardHeight, rustFloor)
        }()
        let viewportHeight = hasCards ? transcript.contentView.bounds.height : 0
        let width = max(fitted.width, transcript.contentView.bounds.width, blocks.frame.width)
        let preservedHeight = preservedDocumentHeightConstraint?.constant ?? 0
        let spacers = TranscriptDocumentSizing.spacerHeights(
            contentHeight: contentHeight,
            viewportHeight: viewportHeight,
            preservedDocumentHeight: preservedHeight
        )
        if abs(transcriptLeadingSpacerHeight.constant - spacers.leading) > 0.5 {
            transcriptLeadingSpacerHeight.constant = spacers.leading
            layoutSubtreeIfNeeded()
        }
        if abs(blocks.frame.width - width) > 0.5 || abs(blocks.frame.height - spacers.document) > 0.5 {
            blocks.setFrameSize(NSSize(width: width, height: spacers.document))
            transcript.reflectScrolledClipView(transcript.contentView)
            layoutSubtreeIfNeeded()
        }
    }

    /// Measure the current cards rather than subtracting the spacer from
    /// `NSStackView.fittingSize`, which can retain the prior document frame
    /// while Blocks are rebuilt after focus/projection changes (#1426).
    func transcriptCardsContentHeight() -> CGFloat {
        TranscriptDocumentSizing.contentHeight(
            arrangedSubviews: blocks.arrangedSubviews,
            leadingSpacer: transcriptLeadingSpacer,
            spacing: blocks.spacing,
            edgeInsets: blocks.edgeInsets
        )
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
        rememberTranscriptScrollState()
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
        rememberTranscriptScrollState()
    }
}
