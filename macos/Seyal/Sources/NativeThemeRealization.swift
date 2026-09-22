import AppKit

/// Maps Rust-resolved tokens onto AppKit. Not a product theme authority.
struct NativeTheme {
    let canvas: NSColor
    let container: NSColor
    let utility: NSColor
    let elevated: NSColor
    let text: NSColor
    let secondary: NSColor
    let muted: NSColor
    let accent: NSColor
    let seam: NSColor
    let success: NSColor
    let warning: NSColor
    let danger: NSColor
    let appearance: NSAppearance
    let uiFontSize: CGFloat
    let terminalFontSize: CGFloat
    let windowPadding: CGFloat
    let terminalPadding: CGFloat
    let reduceMaterial: Bool
}

enum NativeThemeRealization {
    /// Platform appearance input for Rust resolve: 0 dark, 1 light.
    static func platformAppearanceCode(for appearance: NSAppearance) -> UInt16 {
        appearance.bestMatch(from: [.aqua, .darkAqua]) == .aqua ? 1 : 0
    }

    static func visual(for appearance: NSAppearance) -> SeyalAppVisual {
        seyal_app_visual(platformAppearanceCode(for: appearance))
    }

    static func theme(for appearance: NSAppearance) -> NativeTheme {
        theme(from: visual(for: appearance))
    }

    static func theme(from packed: SeyalAppVisual) -> NativeTheme {
        let canvas = color(packed.canvas)
        let text = color(packed.text)
        let accent = color(packed.accent)
        let container = color(packed.container)
        let resolvedLight = packed.appearance == 1
        return NativeTheme(
            canvas: canvas,
            container: container,
            utility: mix(canvas, text, 0.08),
            elevated: mix(canvas, text, 0.12),
            text: text,
            secondary: mix(text, canvas, 0.32),
            muted: mix(text, canvas, 0.52),
            accent: accent,
            seam: mix(canvas, text, 0.16),
            success: NSColor(srgbRed: 0.22, green: 0.83, blue: 0.62, alpha: 1),
            warning: NSColor(srgbRed: 0.96, green: 0.65, blue: 0.14, alpha: 1),
            danger: NSColor(srgbRed: 0.98, green: 0.44, blue: 0.40, alpha: 1),
            appearance: resolvedLight
                ? NSAppearance(named: .aqua)!
                : NSAppearance(named: .darkAqua)!,
            uiFontSize: CGFloat(packed.ui_font_size),
            terminalFontSize: CGFloat(packed.terminal_font_size),
            windowPadding: CGFloat(packed.window_padding),
            terminalPadding: CGFloat(packed.terminal_padding),
            reduceMaterial: (packed.flags & 1) != 0
        )
    }

    @MainActor
    @discardableResult
    static func apply(to view: NSView, material: NSVisualEffectView, appearance: NSAppearance) -> NativeTheme {
        let packed = visual(for: appearance)
        let theme = theme(from: packed)
        view.window?.backgroundColor = theme.canvas
        view.window?.appearance = theme.appearance
        view.appearance = theme.appearance
        view.wantsLayer = true
        view.layer?.backgroundColor = theme.canvas.cgColor
        material.isHidden = true
        applyColors(in: view, theme: theme)
        surfaceDiagnosticsIfNeeded(from: packed)
        return theme
    }

    static func utf8String(_ pointer: UnsafePointer<UInt8>?, length: UInt32) -> String {
        guard let pointer, length > 0 else { return "" }
        let buffer = UnsafeBufferPointer(start: pointer, count: Int(length))
        return String(bytes: buffer, encoding: .utf8) ?? ""
    }

    static func diagnosticMessages(from visual: SeyalAppVisual) -> [String] {
        guard visual.warning_count > 0 else { return [] }
        return (0..<visual.warning_count).compactMap { index in
            let warning = seyal_app_visual_warning(UInt32(index))
            let text = utf8String(warning.text, length: warning.text_len)
            return text.isEmpty ? nil : text
        }
    }

    static func surfaceDiagnosticsIfNeeded(from visual: SeyalAppVisual) {
        let messages = diagnosticMessages(from: visual)
        guard !messages.isEmpty || (visual.flags & 2) != 0 else { return }
        var lines = messages
        if (visual.flags & 2) != 0 {
            lines.insert("configuration used full default fallback", at: 0)
        }
        // Non-secret bounded diagnostics only — never log file contents.
        for line in lines.prefix(16) {
            NSLog("Seyal config: %@", line)
        }
    }

    @MainActor
    private static func applyColors(in view: NSView, theme: NativeTheme) {
        if let field = view as? NSTextField {
            field.textColor = field.tag == 2 ? theme.muted : (field.tag == 1 ? theme.secondary : theme.text)
            field.backgroundColor = .clear
            field.drawsBackground = false
        }
        if let textView = view as? NSTextView {
            textView.textColor = theme.text
            textView.font = .monospacedSystemFont(ofSize: theme.terminalFontSize, weight: .regular)
            textView.backgroundColor = .clear
            textView.insertionPointColor = theme.accent
        }
        if let button = view as? NSButton {
            button.appearance = theme.appearance
            button.contentTintColor = theme.text
        }
        for child in view.subviews {
            applyColors(in: child, theme: theme)
        }
    }

    static func color(_ packed: UInt32) -> NSColor {
        let red = CGFloat((packed >> 24) & 0xff) / 255
        let green = CGFloat((packed >> 16) & 0xff) / 255
        let blue = CGFloat((packed >> 8) & 0xff) / 255
        let alpha = CGFloat(packed & 0xff) / 255
        return NSColor(srgbRed: red, green: green, blue: blue, alpha: alpha)
    }

    private static func mix(_ a: NSColor, _ b: NSColor, _ amount: CGFloat) -> NSColor {
        let src = a.usingColorSpace(.sRGB) ?? a
        let dst = b.usingColorSpace(.sRGB) ?? b
        return NSColor(
            srgbRed: src.redComponent + (dst.redComponent - src.redComponent) * amount,
            green: src.greenComponent + (dst.greenComponent - src.greenComponent) * amount,
            blue: src.blueComponent + (dst.blueComponent - src.blueComponent) * amount,
            alpha: 1
        )
    }
}
