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
    static let namedF1: UInt32 = 9
    static let namedF2: UInt32 = 10
    static let namedF3: UInt32 = 11
    static let namedF4: UInt32 = 12
    static let namedF5: UInt32 = 13
    static let namedF6: UInt32 = 14
    static let namedF7: UInt32 = 15
    static let namedF8: UInt32 = 16
    static let namedF9: UInt32 = 17
    static let namedF10: UInt32 = 18
    static let namedF11: UInt32 = 19
    static let namedF12: UInt32 = 20
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
        case .f1: return namedF1
        case .f2: return namedF2
        case .f3: return namedF3
        case .f4: return namedF4
        case .f5: return namedF5
        case .f6: return namedF6
        case .f7: return namedF7
        case .f8: return namedF8
        case .f9: return namedF9
        case .f10: return namedF10
        case .f11: return namedF11
        case .f12: return namedF12
        case .home: return namedHome
        case .end: return namedEnd
        case .pageUp: return namedPageUp
        case .pageDown: return namedPageDown
        case .delete: return namedDelete
        default:
            if let fromFunctionScalar = namedKeyFromFunctionKeyScalar(event) {
                return fromFunctionScalar
            }
            if event.charactersIgnoringModifiers == "\u{1b}" {
                return namedEscape
            }
            if event.charactersIgnoringModifiers == " " {
                return namedSpace
            }
            return nil
        }
    }

    /// Synthetic / some hardware paths expose NSF* scalars without `.specialKey`.
    private static func namedKeyFromFunctionKeyScalar(_ event: NSEvent) -> UInt32? {
        guard let ignoring = event.charactersIgnoringModifiers,
              ignoring.unicodeScalars.count == 1,
              let scalar = ignoring.unicodeScalars.first
        else {
            return nil
        }
        switch scalar.value {
        case UInt32(NSF1FunctionKey): return namedF1
        case UInt32(NSF2FunctionKey): return namedF2
        case UInt32(NSF3FunctionKey): return namedF3
        case UInt32(NSF4FunctionKey): return namedF4
        case UInt32(NSF5FunctionKey): return namedF5
        case UInt32(NSF6FunctionKey): return namedF6
        case UInt32(NSF7FunctionKey): return namedF7
        case UInt32(NSF8FunctionKey): return namedF8
        case UInt32(NSF9FunctionKey): return namedF9
        case UInt32(NSF10FunctionKey): return namedF10
        case UInt32(NSF11FunctionKey): return namedF11
        case UInt32(NSF12FunctionKey): return namedF12
        case UInt32(NSHomeFunctionKey): return namedHome
        case UInt32(NSEndFunctionKey): return namedEnd
        case UInt32(NSPageUpFunctionKey): return namedPageUp
        case UInt32(NSPageDownFunctionKey): return namedPageDown
        case UInt32(NSDeleteFunctionKey): return namedDelete
        default: return nil
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
        /// SPEC-024 Fallthrough — continue host/IME/terminal input (not a Swift `fallthrough`).
        case fallsThrough
        case consumed
        case nativeCommand
    }
}
