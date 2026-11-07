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
}
