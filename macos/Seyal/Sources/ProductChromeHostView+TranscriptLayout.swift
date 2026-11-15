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
        guard !transcript.isHidden else { return }
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
            guard !self.transcript.isHidden else { return }
            self.syncTranscriptDocumentSize()
            self.publishBlockOutputFrame()
        }
    }
}
