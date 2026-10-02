import AppKit

/// Thin host realization of Rust `KeybindingShortcutProjection` (SPEC-024 §11 / ADR-015).
/// Swift never invents product key equivalents; reserved Edit/AppKit items stay outside.
enum KeybindingShortcutRealization {
    static let commandPaletteOpen: UInt16 = 0
    static let tabCreate: UInt16 = 2
    static let tabCloseFocused: UInt16 = 3
    static let tabSelectPrevious: UInt16 = 4
    static let tabSelectNext: UInt16 = 5
    static let paneSplitRight: UInt16 = 7
    static let paneSplitDown: UInt16 = 8
    static let presentationToggleRaw: UInt16 = 15
    static let presentationToggleTui: UInt16 = 16
    static let gotoOpen: UInt16 = 21

    static func item(commandId: UInt16) -> SeyalAppShortcutItem? {
        let count = seyal_app_shortcut_count()
        for index in 0..<count {
            let row = seyal_app_shortcut_item(index)
            if row.size == 0 { continue }
            if row.command_id == commandId {
                return row
            }
        }
        return nil
    }

    /// Apply cold key equivalent + AX label from the Rust projection (startup only).
    /// Title is always realized from the projection so §7.3 unbind cannot blank the item.
    static func realize(_ menuItem: NSMenuItem, commandId: UInt16) {
        menuItem.representedObject = commandId
        guard let row = item(commandId: commandId) else {
            menuItem.title = fallbackTitle(commandId)
            menuItem.keyEquivalent = ""
            menuItem.keyEquivalentModifierMask = []
            return
        }
        if let title = copyUTF8(row.title, row.title_len), !title.isEmpty {
            menuItem.title = title
        } else {
            menuItem.title = fallbackTitle(commandId)
        }
        if row.has_key_equivalent != 0 {
            menuItem.keyEquivalent = keyEquivalentString(row)
            menuItem.keyEquivalentModifierMask = modifierMask(row.modifier_bits)
        } else {
            menuItem.keyEquivalent = ""
            menuItem.keyEquivalentModifierMask = []
        }
        if let label = copyUTF8(row.accessibility_label, row.accessibility_label_len) {
            menuItem.setAccessibilityLabel(label)
        }
    }

    private static func fallbackTitle(_ commandId: UInt16) -> String {
        switch commandId {
        case commandPaletteOpen: return "Command Palette"
        case tabCreate: return "New Tab"
        case tabCloseFocused: return "Close Tab"
        case tabSelectPrevious: return "Previous Tab"
        case tabSelectNext: return "Next Tab"
        case paneSplitRight: return "Split Right"
        case paneSplitDown: return "Split Down"
        case presentationToggleRaw: return "Toggle Raw"
        case presentationToggleTui: return "Toggle TUI"
        case gotoOpen: return "Go to…"
        default: return "Command"
        }
    }

    static func isEnabled(appHandle: UInt64, commandId: UInt16, composerFocused: Bool) -> Bool {
        seyal_app_shortcut_enabled(appHandle, commandId, 0, composerFocused ? 1 : 0) != 0
    }

    private static func modifierMask(_ bits: UInt8) -> NSEvent.ModifierFlags {
        var flags: NSEvent.ModifierFlags = []
        if bits & 1 != 0 { flags.insert(.command) }
        if bits & 2 != 0 { flags.insert(.control) }
        if bits & 4 != 0 { flags.insert(.shift) }
        if bits & 8 != 0 { flags.insert(.option) }
        return flags
    }

    private static func keyEquivalentString(_ row: SeyalAppShortcutItem) -> String {
        if row.key_is_named != 0 {
            return namedKeyEquivalent(row.key_base)
        }
        guard let scalar = UnicodeScalar(row.key_base) else { return "" }
        return String(Character(scalar))
    }

    private static func namedKeyEquivalent(_ base: UInt32) -> String {
        switch base {
        case 0: return "\r" // Enter
        case 1: return "\t"
        case 2: return " "
        case 3: return "\u{1b}"
        case 4: return String(Character(UnicodeScalar(NSBackspaceCharacter)!))
        case 5: return String(Character(UnicodeScalar(NSUpArrowFunctionKey)!))
        case 6: return String(Character(UnicodeScalar(NSDownArrowFunctionKey)!))
        case 7: return String(Character(UnicodeScalar(NSLeftArrowFunctionKey)!))
        case 8: return String(Character(UnicodeScalar(NSRightArrowFunctionKey)!))
        default: return ""
        }
    }

    private static func copyUTF8(_ ptr: UnsafePointer<UInt8>?, _ len: UInt32) -> String? {
        guard len > 0, let ptr else { return nil }
        return String(decoding: UnsafeBufferPointer(start: ptr, count: Int(len)), as: UTF8.self)
    }
}
