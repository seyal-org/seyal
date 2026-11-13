import AppKit

@MainActor
extension ProductChromeHostView {
    @objc func transcriptDidScroll() {
        guard !isProgrammaticTranscriptScroll else {
            publishBlockOutputFrame()
            return
        }
        DispatchQueue.main.async { [weak self] in
            guard let self, !self.isProgrammaticTranscriptScroll else { return }
            self.followingLiveEnd = self.isNearLiveEnd()
            self.publishBlockOutputFrame()
            self.positionInlineComposerAtCurrentCursor()
        }
    }

    func configureTranscriptLeadingSpacer() {
        transcriptLeadingSpacer.translatesAutoresizingMaskIntoConstraints = false
        transcriptLeadingSpacer.setAccessibilityElement(false)
        transcriptLeadingSpacerHeight = transcriptLeadingSpacer.heightAnchor.constraint(
            equalToConstant: 0
        )
        transcriptLeadingSpacerHeight.isActive = true
        blocks.insertArrangedSubview(transcriptLeadingSpacer, at: 0)
    }

    func pinTranscriptToComposer() {
        let fillsToComposer = transcript.bottomAnchor.constraint(
            equalTo: composer.topAnchor,
            constant: -12
        )
        fillsToComposer.priority = .defaultHigh
        fillsToComposer.isActive = true
    }

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
            let blockID = status.targetClass == 2 ? status.targetBlockID : 0
            self.relayTarget = (
                blockID,
                status.targetGeneration,
                status.targetRevision,
                status.targetClass,
                status.targetMode
            )
            let accepted = self.relayComposerStatus(status)
            if accepted {
                // Status can publish DirectInput / Running(b) without bumping
                // snapshot generation. performChromeReconcile then short-circuits
                // and never rebuildBlocks — empty transcript + Busy dock.
                self.ensureRunningDirectInputBlockCards()
                self.rebindDirectInputClickTargets()
                self.reconcileChrome()
                // Multipane hosts own their Busy dock; generation-stable status
                // must still retract/restore DirectInput layout per Controller.
                for host in self.centerColumn.secondaryHosts {
                    host.refreshBlockScopedDirectInputPresentation()
                }
                // After Busy resigns the dock editor, reclaim Metal for DirectInput.
                let snap = seyal_app_snapshot(self.pane.appHandle)
                self.focusBlockScopedDirectInputIfEligible(snap)
            }
            return accepted
        }
    }

    /// Project + rebuild when the Running DirectInput target has no card yet.
    /// Never rebuild against an empty timeline: that would wipe cards and, if
    /// snapshot generation does not move, leave Busy + blank transcript.
    func ensureRunningDirectInputBlockCards() {
        let targetID = relayTarget.blockID
        guard targetID != 0, relayTarget.targetClass == 2 else { return }
        guard blockCards[targetID] == nil else { return }
        let snap = seyal_app_snapshot(pane.appHandle)
        let direct = snap.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue)
            || snap.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue)
        // Multipane Flow owns cards on SecondaryLivePaneHost; legacy transcript
        // stays empty by design when secondary hosts are installed.
        guard !direct, centerColumn.secondaryHosts.isEmpty else { return }
        projectRuntimeBlocks()
        let composer = seyal_app_composer(pane.appHandle)
        let timelineContainsTarget = (0..<Int(composer.block_count)).contains { index in
            seyal_app_block_row(pane.appHandle, UInt32(index)).id_lo == targetID
        }
        guard timelineContainsTarget else { return }
        rebuildBlocks()
        publishLiveTailBlocks()
        publishBlockOutputFrame()
    }

    /// Keep Warp-like click-to-focus wired after late DirectInput publication.
    func rebindDirectInputClickTargets() {
        for (blockID, card) in blockCards {
            let isRunningTarget = blockID != 0
                && blockID == relayTarget.blockID
                && relayTarget.targetClass == 2
            card.onRequestDirectInputFocus = isRunningTarget
                ? { [weak self, blockID] in
                    guard let self else { return }
                    let snap = seyal_app_snapshot(self.pane.appHandle)
                    guard self.snapshotPublishesBlockScopedDirectInput(snap),
                        blockID == self.relayTarget.blockID
                    else { return }
                    _ = self.focusBlockScopedDirectInputSurface()
                }
                : nil
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

    /// Multipane Flow uses one durable presentation host per Pane. Remove the
    /// legacy focused-pane cards from the hidden single-pane transcript so
    /// accessibility exposes each Block only through its owning pane host.
    /// Keep retained history in the input surface: if the layout collapses to
    /// one Pane, `rebuildBlocks()` can project it again without another fetch.
    func clearLegacyTranscriptProjectionForMultipane() {
        for card in blockCards.values {
            blocks.removeArrangedSubview(card)
            card.removeFromSuperview()
        }
        blockCards.removeAll()
        lastBlockCount = 0
        syncTranscriptDocumentSize()
    }

    /// Relay Runtime's composer eligibility and target into Rust unchanged.
    /// Rust decides presentation and input ownership; this is a thin adapter.
    @discardableResult
    func relayComposerStatus(_ status: NativeComposerStatus) -> Bool {
        let accepted = seyal_app_apply_runtime_composer_status(
            pane.appHandle,
            status.eligibility,
            status.revision,
            status.targetBlockID,
            status.targetGeneration,
            status.targetRevision,
            status.targetClass,
            status.targetMode
        ) == 0
        if accepted {
            let snapshot = seyal_app_snapshot(pane.appHandle)
            focusBlockScopedDirectInputIfEligible(snapshot)
        }
        return accepted
    }

    func applyTranscriptPresentation(_ snapshot: SeyalAppSnapshot) {
        pane.inputSurface.activateTranscriptDisplayHandle(activeBlockDisplayHandle)
        // The explicit Running Block action enters Raw without a full chrome
        // reconciliation. Keep AppKit's first-responder and AX route in sync
        // with Rust's authoritative eligibility on every presentation change.
        pane.inputSurface.syncInputRoutePresentation()
        focusBlockScopedDirectInputIfEligible(snapshot)
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
        // Preserve scroll across projection rebuilds so status/timeline
        // churn does not yank intentional history scroll (#1379 / #1424 parity).
        let savedOffsetY = transcript.contentView.bounds.origin.y
        let savedFollowing = followingLiveEnd
        let previousCards = blockCards
        blockCards.removeAll(keepingCapacity: true)
        let composer = seyal_app_composer(pane.appHandle)
        let count = Int(composer.block_count)
        let sessionDisplayHandle = activeBlockDisplayHandle
        let sessionSurface = seyal_bridge_session_surface_for_handle(sessionDisplayHandle)
        let requestableSessionHost = sessionSurface.kind == 0
            ? sessionSurface.host_block_id
            : 0
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
            card.setAccessibilityIdentifier("seyal-block-\(visibleIndex)")
            card.setBodyAccessibilityIdentifier("seyal-block-\(visibleIndex)-body")
            card.canRequestSessionBootstrap = blockID != 0 && blockID == requestableSessionHost
            card.onRequestSessionBootstrap = card.canRequestSessionBootstrap
                ? { [weak self, sessionDisplayHandle, blockID, sessionRevision = sessionSurface.revision] in
                    _ = seyal_bridge_session_request_for_handle(
                        sessionDisplayHandle,
                        blockID,
                        sessionRevision
                    )
                    self?.sessionBootstrap.reconcile()
                }
                : nil
            configureRunningProgramInteraction(for: card, blockID: blockID, flags: row.flags)
            visibleIndex += 1
            var heightLines = lines
            if blockID != 0,
               let retainedRange = pane.inputSurface.retainedHistoryRange(
                   blockID: blockID,
                   displayHandle: activeBlockDisplayHandle
               ) {
                // Transcript text is AX/retain. Card height floors on the Rust
                // projection (reserved1 sealed count) and retained rows so a
                // short retain cannot collapse the body (#1484 Phase G RC2).
                // Also prepare Metal here — plan-only publish can miss shell-exit
                // retain when BlockViewportPlan is already gone (#1484).
                card.setBodyTranscript(retainedRange.accessibilityTranscript())
                heightLines = max(heightLines, max(retainedRange.rows.count, 1))
                card.setOutputLines(heightLines, cellHeight: cellHeight)
                // Floor-hidden → shell-exit reveal needs a laid-out body clip
                // before Metal prepare; zero bounds skip paint (#1484).
                card.layoutSubtreeIfNeeded()
                let clip = card.convert(card.body.frame, to: pane.inputSurface)
                if clip.width > 1, clip.height > 1 {
                    var region = NativeTranscriptRegion(
                        id: blockID,
                        origin: .zero,
                        clip: clip,
                        historyRows: 0..<retainedRange.rows.count,
                        historyRequestID: retainedRange.requestID,
                        historyRevision: retainedRange.revision
                    )
                    region.displayHandle = retainedRange.sourceDisplayHandle
                    pane.inputSurface.renderHistoryRange(retainedRange, region: region)
                }
            } else {
                // Plan owns paint geometry; card height is chrome/AX hit-target only.
                card.setOutputLines(heightLines, cellHeight: cellHeight)
            }
            card.isSelected = row.flags & UInt16(SEYAL_APP_BLOCK_SELECTED) != 0
            let idLo = row.id_lo
            let idHi = row.id_hi
            card.onSelect = { [weak self] selected in
                self?.selectBlock(idLo: idLo, idHi: idHi, deselect: selected)
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
        pane.inputSurface.discardHistoryRequests(except: retained)
        // Prune retained history to the timeline Block set (including
        // floor-hidden cards) so New shell / eviction cannot paint a prior
        // execution's cells under a reused Block id, while clear→exit can
        // still re-materialize pre-clear bodies.
        pane.inputSurface.removeTranscriptRegions(except: retained)
        syncTranscriptDocumentSize()
        publishBlockOutputFrame()
        publishLiveTailBlocks()
        if savedFollowing {
            scrollTranscriptToLiveEnd()
        } else {
            // Keep the reader's intentional history offset across rebuilds.
            // Expand-only document sizing (syncTranscriptDocumentSize) keeps
            // the prior document height as a floor for this restore.
            isProgrammaticTranscriptScroll = true
            defer { isProgrammaticTranscriptScroll = false }
            let maxY = max(transcriptDocumentScrollHeight() - transcript.contentView.bounds.height, 0)
            let y = min(max(savedOffsetY, 0), maxY)
            transcript.contentView.scroll(to: NSPoint(x: 0, y: y))
            transcript.reflectScrolledClipView(transcript.contentView)
            followingLiveEnd = false
            publishBlockOutputFrame()
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
            if self.followingLiveEnd {
                self.scrollTranscriptToLiveEnd()
            }
            self.publishBlockOutputFrame()
        }
    }

    func applyComposerPlacement() {
        let shouldRestoreEditorFocus = composer.editorIsFirstResponder
        let snapshot = seyal_app_snapshot(pane.appHandle)
        let rawOrTui = snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue)
            || snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue)
        if rawOrTui {
            restoreDockComposerPresentation(restoreFocus: shouldRestoreEditorFocus)
            return
        }
        // Warp-like DirectInput: retract the Busy dock and let the Running Block
        // own the live input surface (no second typing place).
        if snapshotPublishesBlockScopedDirectInput(snapshot) {
            presentDirectInputMergedBlockSurface()
            return
        }
        let target = relayTarget.mode == 2 && relayTarget.generation != 0
            ? blockCards[relayTarget.blockID] : nil
        guard relayTarget.blockID != 0, let target else {
            restoreDockComposerPresentation(restoreFocus: shouldRestoreEditorFocus)
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
        composer.translatesAutoresizingMaskIntoConstraints = true
        composer.setInlineRelayPresentation(true)
        // Keep the one Pane composer above the full-pane Metal surface. A
        // Block is an output/history projection and is underneath that surface;
        // reparenting the editor into its body makes AX see input that pixels
        // cannot paint. The converted cursor rect follows transcript scrolling.
        composer.setFrameSize(NSSize(width: composer.frame.width, height: 24))
        liveContent.addSubview(composer, positioned: .above, relativeTo: pane)
        composerInlineBlockID = relayTarget.blockID
        layoutSubtreeIfNeeded()
        positionInlineComposerAtCurrentCursor()
        syncTranscriptDocumentSize()
        if followingLiveEnd {
            scrollTranscriptToLiveEnd()
        }
        positionInlineComposerAtCurrentCursor()
        if shouldRestoreEditorFocus { routeFocus() }
    }

    /// Hide the Pane dock and fill the transcript to the Pane bottom while
    /// Block-scoped DirectInput owns keys in the Running Block live region.
    func presentDirectInputMergedBlockSurface() {
        if composerInlineBlockID != 0 {
            composerInlineBlockID = 0
            composer.setInlineRelayPresentation(false)
            composer.translatesAutoresizingMaskIntoConstraints = false
        }
        NSLayoutConstraint.deactivate(composerDockConstraints)
        setRelayTranscriptViewportBottomAnchored(true)
        composer.isHidden = true
        layoutSubtreeIfNeeded()
        syncTranscriptDocumentSize()
        if followingLiveEnd {
            scrollTranscriptToLiveEnd()
        }
        publishBlockOutputFrame()
    }

    func restoreDockComposerPresentation(restoreFocus: Bool) {
        setRelayTranscriptViewportBottomAnchored(false)
        if composerInlineBlockID != 0 {
            composerInlineBlockID = 0
            composer.setInlineRelayPresentation(false)
            composer.translatesAutoresizingMaskIntoConstraints = false
        }
        composer.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate(composerDockConstraints)
        composer.reconcile()
        if restoreFocus { routeFocus() }
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
        case UInt16(SEYAL_APP_BLOCK_PROJECTION_PRIMARY_CLIP),
             UInt16(SEYAL_APP_BLOCK_PROJECTION_NATIVE_REGIONS):
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
                  projection.kind == UInt16(SEYAL_APP_BLOCK_PROJECTION_PRIMARY_CLIP)
                    || projection.kind == UInt16(SEYAL_APP_BLOCK_PROJECTION_NATIVE_REGIONS),
                  let card = blockCards[row.id_lo]
            else { continue }
            var heightLines = outputLineCount(projection: projection)
            if let retainedRange = pane.inputSurface.retainedHistoryRange(
                blockID: row.id_lo,
                displayHandle: activeBlockDisplayHandle
            ) {
                card.setBodyTranscript(retainedRange.accessibilityTranscript())
                heightLines = max(heightLines, max(retainedRange.rows.count, 1))
            }
            card.setOutputLines(heightLines, cellHeight: cellHeight)
            // Sealed NATIVE only — Running PRIMARY_CLIP paints via LiveTail and
            // must not refill HISTORY on every frame (#1484 100k).
            if projection.kind == UInt16(SEYAL_APP_BLOCK_PROJECTION_NATIVE_REGIONS) {
                applyBlockOutputProjection(blockID: row.id_lo, projection: projection)
            }
        }
        syncTranscriptDocumentSize()
        positionInlineComposerAtCurrentCursor()
        publishBlockOutputFrame()
    }

    /// Flow Block output fetches are owned by `applyBlockViewportPlan` (C/D/E).
    /// Kept as a no-op hook so rebuild call sites stay stable while card chrome
    /// remains a thin AX/header host.
    func applyBlockOutputProjection(blockID: UInt64, projection: SeyalAppBlockProjection) {
        guard activeBlockDisplayHandle != 0 else { return }
        // Finding D/E: sealed NATIVE and LineId HISTORY Flow fetches are retired.
        // `applyBlockViewportPlan` owns visible±overscan sealed-index retrieval;
        // Raw scrollback keeps its own path (#1442).
        _ = blockID
        _ = projection
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
            // Completed NATIVE Blocks paint via RegionId HISTORY retain only.
            // Keeping LiveTail on after completion dual-paints the onset over
            // the sealed tail and blanks OCR live-end checks (#1484 100k).
            let nativeCompleted =
                projection.kind == UInt16(SEYAL_APP_BLOCK_PROJECTION_NATIVE_REGIONS)
            guard row.id_lo != 0,
                  projection.reserved1 > 0,
                  !nativeCompleted,
                  projection.kind == UInt16(SEYAL_APP_BLOCK_PROJECTION_PRIMARY_CLIP)
            else { continue }
            live[row.id_lo] = LiveTailClip(
                startLine: max(projection.start_line, 1),
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
        // Finding D: no HistoryProjectionRequestBook — plan-owned sealed clips
        // continue via RustDisplayBridge truncated paging; this path retains
        // cells for Metal/AX. Always prepare Metal instances here so shell-exit
        // retain is not AX-only when BlockViewportPlan is briefly absent (#1484).
        guard !range.rows.isEmpty else { return }
        pane.inputSurface.retainHistoryRange(range)
        let cellHeight = pane.inputSurface.terminalPresentationCellSize().height
        guard range.sourceDisplayHandle == 0
            || range.sourceDisplayHandle == activeBlockDisplayHandle
        else { return }
        if let card = blockCards[range.blockID] {
            card.setBodyTranscript(range.accessibilityTranscript())
            let projectionLines = nativeProjection(blockID: range.blockID)
                .map { outputLineCount(projection: $0) } ?? 1
            card.setOutputLines(
                max(projectionLines, max(range.rows.count, 1)),
                cellHeight: cellHeight
            )
            card.layoutSubtreeIfNeeded()
            let clip = card.convert(card.body.frame, to: pane.inputSurface)
            if clip.width > 1, clip.height > 1 {
                var region = NativeTranscriptRegion(
                    id: range.blockID,
                    origin: .zero,
                    clip: clip,
                    historyRows: 0..<range.rows.count,
                    historyRequestID: range.requestID,
                    historyRevision: range.revision
                )
                region.displayHandle = range.sourceDisplayHandle
                pane.inputSurface.renderHistoryRange(range, region: region)
            } else {
                pane.inputSurface.renderHistoryRange(range)
            }
        } else {
            pane.inputSurface.renderHistoryRange(range)
        }
        layoutSubtreeIfNeeded()
        publishBlockOutputFrame()
        syncTranscriptDocumentSize()
        publishBlockOutputFrame()
        if followingLiveEnd {
            scrollTranscriptToLiveEnd()
        }
    }

    func nativeProjection(blockID: UInt64) -> SeyalAppBlockProjection? {
        let composer = seyal_app_composer(pane.appHandle)
        for index in 0..<Int(composer.block_count) {
            let row = seyal_app_block_row(pane.appHandle, UInt32(index))
            guard row.id_lo == blockID else { continue }
            let projection = seyal_app_block_projection(pane.appHandle, UInt32(index))
            guard projection.kind == UInt16(SEYAL_APP_BLOCK_PROJECTION_NATIVE_REGIONS) else {
                return nil
            }
            return projection
        }
        return nil
    }

    func nativeProjectionMinimumRows(blockID: UInt64) -> Int? {
        guard let projection = nativeProjection(blockID: blockID) else { return nil }
        return max(Int(projection.reserved1), 1)
    }

    func hasRunningPrimaryClip() -> Bool {
        let composer = seyal_app_composer(pane.appHandle)
        for index in 0..<Int(composer.block_count) {
            let projection = seyal_app_block_projection(pane.appHandle, UInt32(index))
            if projection.kind == UInt16(SEYAL_APP_BLOCK_PROJECTION_PRIMARY_CLIP) {
                return true
            }
        }
        return false
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
        // Mirror Secondary: when the user was already at the live end, keep the
        // viewport pinned across Running→sealed height jumps so OCR/live-end
        // paint sees the Block tail, not the onset (#1484 100k).
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
        // Measure the arranged cards themselves. NSStackView.fittingSize can
        // retain the stretched document height after a previous tall stream;
        // subtracting the spacer from that stale value leaves short Blocks
        // stranded above the composer instead of bottom-docking them (#1426).
        let hasCards = !blockCards.isEmpty
        let cardHeight = hasCards
            ? TranscriptDocumentSizing.contentHeight(
                arrangedSubviews: blocks.arrangedSubviews,
                leadingSpacer: transcriptLeadingSpacer,
                spacing: blocks.spacing,
                edgeInsets: blocks.edgeInsets
            )
            : 0
        // Rust indexed row height is the sole Flow content authority when the
        // native map publishes (#1484 / ADR-015). Per-card chrome (header/gap/
        // seam) is a fixed derived inset, not AppKit transcript line counts.
        // Cleared canvas (no cards) collapses regardless of a stale Rust floor (#1424).
        // Finding A/C/D: per-display Rust viewport index / plan is document
        // extent authority. Do not invent chrome/card geometry when Rust has
        // published indexed height (#1484 architecture exit).
        let rustRows = seyal_bridge_native_regions_total_height_for(activeBlockDisplayHandle)
        let cellHeight = pane.inputSurface.terminalPresentationCellSize().height
        let rustBody = rustRows > 0 ? CGFloat(rustRows) * cellHeight : 0
        // Finding D: Rust viewport index / plan is sole document extent.
        // Card AppKit heights are never authority.
        let contentHeight: CGFloat = rustBody > 0 ? rustBody : 0
        _ = (hasCards, cardHeight)
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
        // Finding C/D: sole Flow paint authority is BlockViewportPlan. Card-clip
        // geometry and collectBlockOutputRegions are deleted — never fallback.
        // Screen clips only map plan-selected Blocks onto AppKit body rects.
        guard activeBlockDisplayHandle != 0 else { return }
        layoutSubtreeIfNeeded()
        var screenClips: [UInt64: NSRect] = [:]
        screenClips.reserveCapacity(blockCards.count)
        for (blockID, card) in blockCards {
            card.layoutSubtreeIfNeeded()
            let clip = card.convert(card.body.frame, to: surface)
            guard clip.width > 1, clip.height > 1 else { continue }
            screenClips[blockID] = clip
        }
        if surface.applyBlockViewportPlan(
            displayHandle: activeBlockDisplayHandle,
            screenClips: screenClips
        ) {
            blockOutputZeroClipRetries = 0
            surface.refreshRecoveryAccessibilityValue()
            return
        }
        // Plan absent during revision churn — retry only; do not invent geometry.
        if blockOutputZeroClipRetries < Self.blockOutputZeroClipRetryLimit {
            blockOutputZeroClipRetries += 1
            DispatchQueue.main.async { [weak self] in
                self?.publishBlockOutputFrame()
            }
        }
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
        layoutSubtreeIfNeeded()
        let visible = transcript.contentView.bounds.height
        let y = max(transcriptDocumentScrollHeight() - visible, 0)
        transcript.contentView.scroll(to: NSPoint(x: 0, y: y))
        transcript.reflectScrolledClipView(transcript.contentView)
        // Second pass after scroll/layout so sealed height + spacer settle
        // before headed OCR samples the live end (#1484 100k).
        layoutSubtreeIfNeeded()
        let settledY = max(transcriptDocumentScrollHeight() - visible, 0)
        if abs(settledY - y) > 0.5 {
            transcript.contentView.scroll(to: NSPoint(x: 0, y: settledY))
            transcript.reflectScrolledClipView(transcript.contentView)
        }
        followingLiveEnd = true
    }

}
