import Foundation

/// Native realization of Rust Flow/Raw/TUI renderer intent. Not a product reducer.
enum TerminalPresentationMode: String, Equatable, Sendable {
    case flow
    case raw
    case tui
}

struct RendererPresentationPlan: Equatable, Sendable {
    var mode: TerminalPresentationMode
    var drawsFullGridBackground: Bool
    var drawsLiveGrid: Bool
    var drawsCursorOutsideBlockRegions: Bool

    static func flow() -> RendererPresentationPlan {
        RendererPresentationPlan(
            mode: .flow,
            drawsFullGridBackground: false,
            drawsLiveGrid: false,
            drawsCursorOutsideBlockRegions: false
        )
    }

    static func fullPane(_ mode: TerminalPresentationMode) -> RendererPresentationPlan {
        RendererPresentationPlan(
            mode: mode,
            drawsFullGridBackground: true,
            drawsLiveGrid: true,
            drawsCursorOutsideBlockRegions: true
        )
    }
}

struct RendererPresentationInspection: Equatable, Sendable {
    var mode: TerminalPresentationMode
    var drawsFullGridBackground: Bool
    var drawsLiveGrid: Bool
    var drawsCursorOutsideBlockRegions: Bool
    var blockRegionIDs: [UInt64]
}

struct FlowPaintInspection: Equatable, Sendable {
    var mode: TerminalPresentationMode
    var liveGridSubmitted: Bool
    var fullGridBackgroundSubmitted: Bool
    var historyInstanceCount: Int
    /// History Block buffers that still have at least one drawable glyph.
    /// clear→exit false-green: total instances can be >0 while later cards
    /// were culled to empty after a clip collapse (#1379).
    var paintedHistoryRegionCount: Int
    /// Distinct Block body clips — `collapse` when two history clips largely
    /// overlap so later Block output paints over an earlier card.
    var historyClipsDistinct: Bool
    var instancesOutsideClips: Int
    var opaquePixelsOutsideClips: Int
    var opaquePixelsInsideClips: Int

    var isClean: Bool {
        if mode != .flow {
            return true
        }
        return !liveGridSubmitted
            && !fullGridBackgroundSubmitted
            && instancesOutsideClips == 0
            && opaquePixelsOutsideClips == 0
            && historyClipsDistinct
    }

    var accessibilityToken: String {
        mode == .flow ? (isClean ? "ok" : "leak") : "n/a"
    }
}
