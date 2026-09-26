import AppKit

enum TerminalKeyIntent: UInt16 {
    case enter = 1
    case tab = 2
    case backspace = 3
    case escape = 4
    case arrowUp = 5
    case arrowDown = 6
    case arrowRight = 7
    case arrowLeft = 8
    case controlASCII = 9
}

struct TerminalNativeKeyV2: Equatable {
    let kind: UInt16
    let modifiers: UInt16
    let value: UInt32
    let shiftedASCII: UInt32
}

enum TerminalNativeKeyClassifier {
    /// Hardware key-code → V2 function index. Immutable; not rebuilt per event.
    private static let functionKeyCodes: [UInt16: UInt32] = [
        122: 1, 120: 2, 99: 3, 118: 4, 96: 5, 97: 6, 98: 7, 100: 8, 101: 9, 109: 10, 103: 11,
        111: 12,
    ]
    /// Hardware key-code → V2 keypad value. Immutable; not rebuilt per event.
    private static let keypadKeyCodes: [UInt16: UInt32] = [
        82: 0, 83: 1, 84: 2, 85: 3, 86: 4, 87: 5, 88: 6, 89: 7, 91: 8, 92: 9, 65: 10, 75: 11,
        67: 12, 78: 13, 69: 14, 81: 15, 76: 16,
    ]

    static func v2(
        keyCode: UInt16,
        specialKey: NSEvent.SpecialKey?,
        charactersIgnoringModifiers: String?,
        characters: String?,
        modifierFlags: NSEvent.ModifierFlags,
        optionAsAlt: Bool
    ) -> TerminalNativeKeyV2? {
        let flags = modifierFlags.intersection(.deviceIndependentFlagsMask)
        if let function = functionKeyCodes[keyCode] {
            return assembleV2(
                kind: 15, value: function, semantic: true, flags: flags, optionAsAlt: optionAsAlt,
                characters: characters)
        }
        if flags.contains(.numericPad) {
            if let value = keypadKeyCodes[keyCode] {
                return assembleV2(
                    kind: 16, value: value, semantic: true, flags: flags, optionAsAlt: optionAsAlt,
                    characters: characters)
            }
        }
        let kind: UInt16
        let value: UInt32
        var semantic = specialKey != nil
        switch specialKey {
        case .carriageReturn, .newline, .enter:
            kind = 1
            value = 0
        case .tab, .backTab:
            kind = 2
            value = 0
        case .backspace:
            kind = 3
            value = 0
        case .upArrow: kind = 5; value = 0
        case .downArrow: kind = 6; value = 0
        case .rightArrow: kind = 7; value = 0
        case .leftArrow: kind = 8; value = 0
        case .home: kind = 9; value = 0
        case .end: kind = 10; value = 0
        case .insert: kind = 11; value = 0
        case .delete: kind = 12; value = 0
        case .pageUp: kind = 13; value = 0
        case .pageDown: kind = 14; value = 0
        default:
            if charactersIgnoringModifiers == "\u{1b}" {
                kind = 4
                value = 0
                semantic = true
            } else {
                guard let chars = charactersIgnoringModifiers, chars.unicodeScalars.count == 1,
                    let scalar = chars.unicodeScalars.first?.value, (0x20...0x7e).contains(scalar)
                else { return nil }
                var preview: UInt16 = 0
                if flags.contains(.option) && optionAsAlt { preview |= 2 }
                if flags.contains(.control) { preview |= 4 }
                guard preview & 6 != 0 else { return nil }
                kind = 17
                value = scalar >= 0x41 && scalar <= 0x5a ? scalar + 0x20 : scalar
                semantic = false
            }
        }
        return assembleV2(
            kind: kind, value: value, semantic: semantic, flags: flags, optionAsAlt: optionAsAlt,
            characters: characters)
    }

    private static func assembleV2(
        kind: UInt16,
        value: UInt32,
        semantic: Bool,
        flags: NSEvent.ModifierFlags,
        optionAsAlt: Bool,
        characters: String?
    ) -> TerminalNativeKeyV2? {
        var modifiers: UInt16 = 0
        if flags.contains(.shift) { modifiers |= 1 }
        if flags.contains(.option) && (optionAsAlt || semantic) { modifiers |= 2 }
        if flags.contains(.control) { modifiers |= 4 }
        let shiftedASCII: UInt32
        if kind == 17, flags.contains(.shift) {
            guard let chars = characters, chars.unicodeScalars.count == 1,
                let scalar = chars.unicodeScalars.first?.value, (0x20...0x7e).contains(scalar)
            else { return nil }
            shiftedASCII = scalar
        } else {
            shiftedASCII = 0
        }
        return TerminalNativeKeyV2(
            kind: kind, modifiers: modifiers, value: value, shiftedASCII: shiftedASCII)
    }

    static func controlASCII(
        modifierFlags: NSEvent.ModifierFlags,
        charactersIgnoringModifiers: String?
    ) -> UInt32? {
        let flags = modifierFlags.intersection(.deviceIndependentFlagsMask)
        guard flags.contains(.control) else { return nil }
        let allowed: NSEvent.ModifierFlags = [.control, .shift, .capsLock]
        guard flags.subtracting(allowed).isEmpty,
            let candidate = charactersIgnoringModifiers,
            candidate.unicodeScalars.count == 1,
            let scalar = candidate.unicodeScalars.first?.value,
            scalar <= 0x7f
        else {
            return nil
        }
        let normalized: UInt32
        if scalar >= 0x61 && scalar <= 0x7a {
            normalized = scalar - 0x20
        } else {
            normalized = scalar
        }
        return matchesControlBase(normalized) ? normalized : nil
    }

    static func semanticKey(
        specialKey: NSEvent.SpecialKey?,
        charactersIgnoringModifiers: String?,
        modifierFlags: NSEvent.ModifierFlags
    ) -> TerminalKeyIntent? {
        let candidate: TerminalKeyIntent?
        switch specialKey {
        case .carriageReturn, .newline, .enter:
            candidate = .enter
        case .tab:
            candidate = .tab
        case .backspace:
            candidate = .backspace
        case .upArrow:
            candidate = .arrowUp
        case .downArrow:
            candidate = .arrowDown
        case .rightArrow:
            candidate = .arrowRight
        case .leftArrow:
            candidate = .arrowLeft
        default:
            switch charactersIgnoringModifiers {
            case "\r", "\n": candidate = .enter
            case "\t": candidate = .tab
            case "\u{8}", "\u{7f}": candidate = .backspace
            case "\u{1b}": candidate = .escape
            default: candidate = nil
            }
        }
        guard let candidate else { return nil }
        let flags = modifierFlags.intersection(.deviceIndependentFlagsMask)
        var allowed: NSEvent.ModifierFlags = [.capsLock]
        if candidate == .enter {
            allowed.insert(.numericPad)
        }
        return flags.subtracting(allowed).isEmpty ? candidate : nil
    }

    private static func matchesControlBase(_ scalar: UInt32) -> Bool {
        scalar == 0x20
            || scalar == 0x3f
            || scalar == 0x40
            || (scalar >= 0x41 && scalar <= 0x5f)
    }
}
