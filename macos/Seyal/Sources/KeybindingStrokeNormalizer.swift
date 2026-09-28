import AppKit

/// Thin host normalization for SPEC-024 §3.2. Rust owns the match (ADR-015).
enum KeybindingStrokeNormalizer {
    static let namedEnter: UInt32 = 0
    static let namedTab: UInt32 = 1
    static let namedSpace: UInt32 = 2
    static let namedEscape: UInt32 = 3
    static let namedBackspace: UInt32 = 4
    static let namedUp: UInt32 = 5
    static let namedDown: UInt32 = 6
    static let namedLeft: UInt32 = 7
    static let namedRight: UInt32 = 8
    static let namedHome: UInt32 = 21
    static let namedEnd: UInt32 = 22
    static let namedPageUp: UInt32 = 23
    static let namedPageDown: UInt32 = 24
    static let namedDelete: UInt32 = 25

    struct Payload {
        var modifierBits: UInt8
        var namedKey: UInt8
        var base: UInt32
        var shiftApplied: UInt32
    }

    static func normalize(_ event: NSEvent) -> Payload? {
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        var bits: UInt8 = 0
        if flags.contains(.command) { bits |= 1 }
        if flags.contains(.control) { bits |= 2 }
        if flags.contains(.shift) { bits |= 4 }
        if flags.contains(.option) { bits |= 8 }

        if let named = namedKey(for: event) {
            return Payload(modifierBits: bits, namedKey: 1, base: named, shiftApplied: 0)
        }

        guard let ignoring = event.charactersIgnoringModifiers,
              ignoring.unicodeScalars.count == 1,
              let baseScalar = ignoring.unicodeScalars.first
        else {
            return nil
        }
        var base = baseScalar.value
        if (0x41...0x5a).contains(base) {
            base += 0x20
        }
        var shiftApplied: UInt32 = 0
        if flags.contains(.shift),
           let chars = event.characters,
           chars.unicodeScalars.count == 1,
           let shifted = chars.unicodeScalars.first
        {
            shiftApplied = shifted.value
        }
        return Payload(modifierBits: bits, namedKey: 0, base: base, shiftApplied: shiftApplied)
    }

    private static func namedKey(for event: NSEvent) -> UInt32? {
        switch event.specialKey {
        case .carriageReturn, .newline, .enter: return namedEnter
        case .tab, .backTab: return namedTab
        case .backspace: return namedBackspace
        case .upArrow: return namedUp
        case .downArrow: return namedDown
        case .leftArrow: return namedLeft
        case .rightArrow: return namedRight
        case .home: return namedHome
        case .end: return namedEnd
        case .pageUp: return namedPageUp
        case .pageDown: return namedPageDown
        case .delete: return namedDelete
        default:
            if event.charactersIgnoringModifiers == "\u{1b}" {
                return namedEscape
            }
            if event.charactersIgnoringModifiers == " " {
                return namedSpace
            }
            return nil
        }
    }

    /// Returns true when Rust consumed the stroke or directed native Command handling.
    @discardableResult
    static func route(
        appHandle: UInt64,
        event: NSEvent,
        composerFocused: Bool,
        compositionActive: Bool
    ) -> RouteResult {
        guard let payload = normalize(event) else { return .fallsThrough }
        let code = seyal_app_route_keystroke(
            appHandle,
            payload.modifierBits,
            payload.namedKey,
            payload.base,
            payload.shiftApplied,
            composerFocused ? 1 : 0,
            compositionActive ? 1 : 0
        )
        switch code {
        case 1: return .consumed
        case 2: return .nativeCommand
        default: return .fallsThrough
        }
    }

    enum RouteResult {
        case fallsThrough
        case consumed
        case nativeCommand
    }
}
