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
            pane.inputSurface.setLiveTailBlocks([:])
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
            let projection = seyal_app_block_projection(pane.appHandle, UInt32(index))
            // Copy row text before the action calls re-encode the buffers.
            let title = productChromeCopyUTF8(row.title, row.title_len) ?? ""
            let statusLabel = productChromeCopyUTF8(row.detail, row.detail_len) ?? ""
            let blockID = row.id_lo
            let lines = outputLineCount(projection: projection)
            let card = CommandBlockView(
                row: CommandBlockRow(
                    command: title,
                    state: row.flags & UInt16(SEYAL_APP_BLOCK_STATE_MASK),
                    statusLabel: statusLabel,
                    isSelected: row.flags & UInt16(SEYAL_APP_BLOCK_SELECTED) != 0,
                    actions: blockActions(blockIndex: UInt32(index))
                ),
                cellHeight: cellHeight,
                lines: lines
            )
            card.setAccessibilityIdentifier("seyal-block-\(index)")
            card.body.setAccessibilityIdentifier("seyal-block-\(index)-body")
            let idLo = row.id_lo
            let idHi = row.id_hi
            card.onSelect = { [weak self] selected in
                self?.selectBlock(idLo: idLo, idHi: idHi, deselect: selected)
            }
            card.onAction = { [weak self] action in
                self?.performBlockAction(action, blockIndex: UInt32(index), idLo: idLo, idHi: idHi)
            }
            blocks.addArrangedSubview(card)
            if blockID != 0 {
                blockCards[blockID] = card
                retained.insert(blockID)
                applyBlockOutputProjection(blockID: blockID, projection: projection)
            }
        }
        pane.inputSurface.discardHistoryRequests(except: retained)
        layoutSubtreeIfNeeded()
        publishBlockOutputFrame()
        publishLiveTailBlocks()
        if count > lastBlockCount, followingLiveEnd {
            scrollTranscriptToLiveEnd()
        }
        lastBlockCount = count
    }

    /// Rust-projected quick actions for one Block row (#1010).
    private func blockActions(blockIndex: UInt32) -> [CommandBlockActionRow] {
        let count = seyal_app_block_action_count(pane.appHandle, blockIndex)
        return (0..<count).compactMap { actionIndex in
            let row = seyal_app_block_action_row(pane.appHandle, blockIndex, actionIndex)
            guard row.kind != 0 else { return nil }
            return CommandBlockActionRow(
                kind: row.kind,
                placement: (row.flags & UInt16(SEYAL_APP_BLOCK_ACTION_PLACEMENT_MASK))
                    >> UInt16(SEYAL_APP_BLOCK_ACTION_PLACEMENT_SHIFT),
                label: productChromeCopyUTF8(row.title, row.title_len) ?? "",
                shortcut: productChromeCopyUTF8(row.detail, row.detail_len) ?? "",
                enabled: row.flags & UInt16(SEYAL_APP_BLOCK_ACTION_ENABLED) != 0
            )
        }
    }

    /// Routes a Rust action kind. Availability was already decided by Rust;
    /// disabled actions are never delivered by the view. Copy output kinds go
    /// through `seyal_app_request_block_copy` so span/composition stay in Rust.
    private func performBlockAction(
        _ kind: UInt16,
        blockIndex: UInt32,
        idLo: UInt64,
        idHi: UInt64
    ) {
        switch UInt32(kind) {
        case SEYAL_APP_BLOCK_ACTION_COPY_COMMAND:
            let row = seyal_app_block_row(pane.appHandle, blockIndex)
            writePasteboard(productChromeCopyUTF8(row.title, row.title_len) ?? "")
        case SEYAL_APP_BLOCK_ACTION_COPY_OUTPUT, SEYAL_APP_BLOCK_ACTION_COPY_COMMAND_AND_OUTPUT:
            _ = seyal_app_request_block_copy(pane.appHandle, blockIndex, kind)
        case SEYAL_APP_BLOCK_ACTION_RERUN:
            let snapshot = seyal_app_snapshot(pane.appHandle)
            var rerun = SeyalAppAction()
            rerun.version = UInt16(SEYAL_APP_ABI_VERSION)
            rerun.size = UInt16(MemoryLayout<SeyalAppAction>.size)
            rerun.kind = UInt16(SEYAL_APP_ACTION_RERUN_BLOCK.rawValue)
            rerun.applySnapshotFence(snapshot)
            rerun.target_execution_lo = idLo
            rerun.target_execution_hi = idHi
            rerun.target_pty_generation = seyal_app_composer(pane.appHandle).epoch
            guard seyal_app_apply(pane.appHandle, &rerun) == 0 else { return }
            composer.submitRustDraft()
        case SEYAL_APP_BLOCK_ACTION_INSPECT:
            selectBlock(idLo: idLo, idHi: idHi, deselect: false)
        default:
            break
        }
    }

    func writePasteboard(_ text: String) {
        let pasteboard = NSPasteboard.general
        pasteboard.clearContents()
        pasteboard.setString(text, forType: .string)
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
        layoutSubtreeIfNeeded()
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
