import Foundation

/// Native applicator mirror of Rust `RendererPlan` (ADR-015 / #1476).
/// Construct only from `SeyalAppRendererPlan` FFI records — never invent flags.
enum TerminalPresentationMode: String, Equatable, Sendable {
    case flow
    case raw
    case tui

    init(eligibilityOrMode code: UInt16) {
        switch code {
        case UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue):
            self = .tui
        case UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue):
            self = .raw
        default:
            self = .flow
        }
    }
}

struct RendererPresentationPlan: Equatable, Sendable {
    var mode: TerminalPresentationMode
    var drawsFullGridBackground: Bool
    var drawsLiveGrid: Bool
    var drawsCursorOutsideBlockRegions: Bool

    /// Apply Rust-authored plan flags. Fail closed on version/size mismatch.
    init(_ plan: SeyalAppRendererPlan) {
        let compatible = plan.version == UInt16(SEYAL_APP_RENDERER_PLAN_ABI_VERSION)
            && plan.size == UInt16(MemoryLayout<SeyalAppRendererPlan>.size)
        if compatible {
            mode = TerminalPresentationMode(eligibilityOrMode: plan.mode)
            drawsFullGridBackground =
                (plan.flags & UInt16(SEYAL_APP_RENDERER_PLAN_FULL_GRID_BACKGROUND)) != 0
            drawsLiveGrid = (plan.flags & UInt16(SEYAL_APP_RENDERER_PLAN_LIVE_GRID)) != 0
            drawsCursorOutsideBlockRegions =
                (plan.flags & UInt16(SEYAL_APP_RENDERER_PLAN_CURSOR_OUTSIDE_BLOCKS)) != 0
        } else {
            mode = .flow
            drawsFullGridBackground = false
            drawsLiveGrid = false
            drawsCursorOutsideBlockRegions = false
        }
    }

    /// Resting plan for the focused application snapshot eligibility.
    static func fromApp(_ handle: UInt64) -> RendererPresentationPlan {
        RendererPresentationPlan(seyal_app_renderer_plan(handle))
    }

    /// Resting plan for a pane-scoped eligibility proxy.
    static func fromPane(appHandle: UInt64, paneLo: UInt64, paneHi: UInt64)
        -> RendererPresentationPlan
    {
        RendererPresentationPlan(
            seyal_app_renderer_plan_for_pane(appHandle, paneLo, paneHi)
        )
    }

    /// Pure resolve for resting eligibility or Raw scrollback overlay.
    static func resolve(eligibility: UInt16, variant: UInt16) -> RendererPresentationPlan {
        RendererPresentationPlan(seyal_app_renderer_plan_resolve(eligibility, variant))
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
    /// Painted glyph instances in live-tail Block buffers (running command).
    /// Kept separate from history so clear→exit ink checks stay history-only.
    var liveTailInstanceCount: Int
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
        // A retained history buffer can outlive a narrowed Block after a pane
        // split. Those source instances are intentionally left untouched; the
        // encoder applies a per-region Metal scissor, so only pixels escaping
        // that clip constitute visible leakage. Keep instancesOutsideClips as
        // a geometry diagnostic instead of treating it as painted output.
        return !liveGridSubmitted
            && !fullGridBackgroundSubmitted
            && opaquePixelsOutsideClips == 0
            && historyClipsDistinct
    }

    var accessibilityToken: String {
        mode == .flow ? (isClean ? "ok" : "leak") : "n/a"
    }
}
