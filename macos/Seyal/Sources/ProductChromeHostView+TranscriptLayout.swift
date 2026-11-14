import AppKit

@MainActor
enum TranscriptDocumentSizing {
    static func contentHeight(
        arrangedSubviews: [NSView],
        leadingSpacer: NSView,
        spacing: CGFloat,
        edgeInsets: NSEdgeInsets
    ) -> CGFloat {
        let contentSubviews = arrangedSubviews.filter { $0 !== leadingSpacer }
        guard !contentSubviews.isEmpty else { return 0 }
        let intrinsicHeight = contentSubviews.reduce(CGFloat.zero) { total, view in
            total + max(view.fittingSize.height, 0)
        }
        return intrinsicHeight
            + CGFloat(max(arrangedSubviews.count - 1, 0)) * spacing
            + edgeInsets.top
            + edgeInsets.bottom
    }

    static func spacerHeights(
        contentHeight: CGFloat,
        viewportHeight: CGFloat,
        preservedDocumentHeight: CGFloat
    ) -> (leading: CGFloat, trailing: CGFloat, document: CGFloat) {
        let naturalHeight = max(contentHeight, viewportHeight)
        let documentHeight = max(max(naturalHeight, preservedDocumentHeight), 1)
        // Keep slack above the newest Block so live and scrolled layouts dock
        // Blocks consistently above the composer.
        let leadingHeight = max(documentHeight - contentHeight, 0)
        return (leadingHeight, 0, documentHeight)
    }
}

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
        // The focused pane also has a durable secondary host; route its handle
        // there before falling back to the single-pane transcript.
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
    override func layout() {
        super.layout()
        guard !transcript.isHidden, !blockCards.isEmpty else { return }
        let viewportSize = transcript.contentView.bounds.size
        guard viewportSize.width > 8, viewportSize.height > 8,
              abs(viewportSize.width - lastTranscriptViewportSize.width) > 0.5
                || abs(viewportSize.height - lastTranscriptViewportSize.height) > 0.5
        else { return }
        lastTranscriptViewportSize = viewportSize
        guard !hasPendingTranscriptViewportSync else { return }
        hasPendingTranscriptViewportSync = true
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            self.hasPendingTranscriptViewportSync = false
            guard !self.transcript.isHidden, !self.blockCards.isEmpty else { return }
            self.syncTranscriptDocumentSize()
            self.publishBlockOutputFrame()
            if self.followingLiveEnd {
                self.scrollTranscriptToLiveEnd()
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
        // Measure the arranged cards themselves. NSStackView.fittingSize can
        // retain the stretched document height after a previous tall stream;
        // subtracting the spacer from that stale value leaves short Blocks
        // stranded above the composer instead of bottom-docking them (#1426).
        let hasCards = !blockCards.isEmpty
        let contentHeight = hasCards
            ? TranscriptDocumentSizing.contentHeight(
                arrangedSubviews: blocks.arrangedSubviews,
                leadingSpacer: transcriptLeadingSpacer,
                spacing: blocks.spacing,
                edgeInsets: blocks.edgeInsets
            )
            : 0
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
        if abs(blocks.frame.width - width) > 0.5
            || abs(blocks.frame.height - spacers.document) > 0.5
        {
            blocks.setFrameSize(NSSize(width: width, height: spacers.document))
            transcript.reflectScrolledClipView(transcript.contentView)
            layoutSubtreeIfNeeded()
        }
    }
}
