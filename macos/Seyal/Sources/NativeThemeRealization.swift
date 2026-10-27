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
    /// Selected C04 row fill; Rust ColorRole::Selection (#1375).
    let selection: NSColor
    /// Block Component roles (#1010), resolved by Rust.
    let blockFocus: NSColor
    let blockBorderRest: NSColor
    let blockBorderHover: NSColor
    let blockSuccess: NSColor
    let blockDanger: NSColor
    /// Rust-resolved motion: false under Reduce Motion (design §8).
    let allowsMotion: Bool
    let appearance: NSAppearance
    let uiFontSize: CGFloat
    let terminalFontSize: CGFloat
    let windowPadding: CGFloat
    let terminalPadding: CGFloat
    /// True when Rust resolved reduced material / reduced transparency.
    let reduceMaterial: Bool
    /// Rust `utility_material`: 0 opaque, 1 tonal, 2 frosted.
    let utilityMaterial: UInt16
    let utilityOpacity: CGFloat
    /// Rust-resolved terminal defaults converted to Metal's little-byte RGBA packing.
    let terminalDefaultForeground: UInt32
    let terminalDefaultBackground: UInt32
    /// C05 / VF-4 chip width band from Rust `Metrics` (cold path).
    let tabMinWidth: CGFloat
    let tabMaxWidth: CGFloat

    /// Utility chrome should show the host material effect (frosted, not reduced).
    var usesFrostedUtilityMaterial: Bool {
        utilityMaterial == 2 && !reduceMaterial
    }

    init(
        canvas: NSColor,
        container: NSColor,
        utility: NSColor,
        elevated: NSColor,
        text: NSColor,
        secondary: NSColor,
        muted: NSColor,
        accent: NSColor,
        seam: NSColor,
        success: NSColor,
        warning: NSColor,
        danger: NSColor,
        selection: NSColor,
        blockFocus: NSColor,
        blockBorderRest: NSColor,
        blockBorderHover: NSColor,
        blockSuccess: NSColor,
        blockDanger: NSColor,
        allowsMotion: Bool,
        appearance: NSAppearance,
        uiFontSize: CGFloat,
        terminalFontSize: CGFloat,
        windowPadding: CGFloat,
        terminalPadding: CGFloat,
        reduceMaterial: Bool,
        utilityMaterial: UInt16,
        utilityOpacity: CGFloat,
        terminalDefaultForeground: UInt32,
        terminalDefaultBackground: UInt32,
        tabMinWidth: CGFloat = 118,
        tabMaxWidth: CGFloat = 190
    ) {
        self.canvas = canvas
        self.container = container
        self.utility = utility
        self.elevated = elevated
        self.text = text
        self.secondary = secondary
        self.muted = muted
        self.accent = accent
        self.seam = seam
        self.success = success
        self.warning = warning
        self.danger = danger
        self.selection = selection
        self.blockFocus = blockFocus
        self.blockBorderRest = blockBorderRest
        self.blockBorderHover = blockBorderHover
        self.blockSuccess = blockSuccess
        self.blockDanger = blockDanger
        self.allowsMotion = allowsMotion
        self.appearance = appearance
        self.uiFontSize = uiFontSize
        self.terminalFontSize = terminalFontSize
        self.windowPadding = windowPadding
        self.terminalPadding = terminalPadding
        self.reduceMaterial = reduceMaterial
        self.utilityMaterial = utilityMaterial
        self.utilityOpacity = utilityOpacity
        self.terminalDefaultForeground = terminalDefaultForeground
        self.terminalDefaultBackground = terminalDefaultBackground
        self.tabMinWidth = tabMinWidth
        self.tabMaxWidth = tabMaxWidth
    }
}

enum NativeThemeRealization {
    private final class ColdDiagnosticsGate: @unchecked Sendable {
        static let shared = ColdDiagnosticsGate()
        private let lock = NSLock()
        private var surfaced = false

        func runOnce(_ body: () -> Void) {
            lock.lock()
            let already = surfaced
            if !already { surfaced = true }
            lock.unlock()
            guard !already else { return }
            body()
        }

        func resetForTests() {
            lock.lock()
            surfaced = false
            lock.unlock()
        }
    }

    /// Platform appearance input for Rust resolve: 0 dark, 1 light.
    static func platformAppearanceCode(for appearance: NSAppearance) -> UInt16 {
        appearance.bestMatch(from: [.aqua, .darkAqua]) == .aqua ? 1 : 0
    }

    static func visual(for appearance: NSAppearance) -> SeyalAppVisual {
        seyal_app_visual(platformAppearanceCode(for: appearance))
    }

    static func theme(for appearance: NSAppearance) -> NativeTheme {
        theme(from: visual(for: appearance), accessibilityFlags: accessibilityFlags())
    }

    /// Host-forwarded OS accessibility bits for Rust resolve (bit0 reduce_motion).
    static func accessibilityFlags() -> UInt16 {
        var flags: UInt16 = 0
        if NSWorkspace.shared.accessibilityDisplayShouldReduceMotion {
            flags |= 1
        }
        return flags
    }

    static func theme(from packed: SeyalAppVisual, accessibilityFlags: UInt16 = 0) -> NativeTheme {
        let canvas = color(packed.canvas)
        let text = color(packed.text)
        let accent = color(packed.accent)
        let container = color(packed.container)
        let resolvedLight = packed.appearance == 1
        // Block Component roles (#1010) remain on seyal_app_theme; both paths
        // resolve through Rust process UI configuration (ADR-015 / #993).
        let blockPacked = seyal_app_theme(packed.appearance, accessibilityFlags)
        // Chrome + Block roles from Rust seyal_app_theme (ADR-015 / #993 / #1375).
        // The host maps packed roles only — it does not invent Adaptive Depth mixes.
        return NativeTheme(
            canvas: canvas,
            container: container,
            utility: color(blockPacked.utility),
            elevated: color(blockPacked.elevated),
            text: text,
            secondary: color(blockPacked.secondary),
            muted: color(blockPacked.muted),
            accent: accent,
            seam: color(blockPacked.seam),
            success: color(blockPacked.success),
            warning: color(blockPacked.warning),
            danger: color(blockPacked.danger),
            selection: color(blockPacked.selection),
            blockFocus: color(blockPacked.block_focus),
            blockBorderRest: color(blockPacked.block_border_rest),
            blockBorderHover: color(blockPacked.block_border_hover),
            blockSuccess: color(blockPacked.success),
            blockDanger: color(blockPacked.danger),
            allowsMotion: blockPacked.flags & UInt16(SEYAL_APP_THEME_ALLOWS_MOTION) != 0,
            appearance: resolvedLight
                ? NSAppearance(named: .aqua)!
                : NSAppearance(named: .darkAqua)!,
            uiFontSize: CGFloat(packed.ui_font_size),
            terminalFontSize: CGFloat(packed.terminal_font_size),
            windowPadding: CGFloat(packed.window_padding),
            terminalPadding: CGFloat(packed.terminal_padding),
            reduceMaterial: (packed.flags & 1) != 0,
            utilityMaterial: packed.utility_material,
            utilityOpacity: CGFloat(packed.utility_opacity),
            terminalDefaultForeground: packed.text.byteSwapped,
            terminalDefaultBackground: packed.canvas.byteSwapped,
            tabMinWidth: CGFloat(packed.tab_min_width),
            tabMaxWidth: CGFloat(packed.tab_max_width)
        )
    }

    @MainActor
    @discardableResult
    static func apply(to view: NSView, material: NSVisualEffectView, appearance: NSAppearance) -> NativeTheme {
        let packed = visual(for: appearance)
        let theme = theme(from: packed, accessibilityFlags: accessibilityFlags())
        applyMaterial(material, theme: theme)
        view.window?.appearance = theme.appearance
        view.appearance = theme.appearance
        view.wantsLayer = true
        // Keep the window opaque with a solid canvas fill. Frosted utility material is
        // realized on the in-window effect view under clear utility chrome — not by
        // making the window transparent (which can break headed attach on CI).
        view.window?.isOpaque = true
        view.window?.backgroundColor = theme.canvas
        view.layer?.backgroundColor = theme.canvas.cgColor
        applyColors(in: view, theme: theme)
        return theme
    }

    /// Map Rust utility material intent onto the host effect view.
    @MainActor
    static func applyMaterial(_ material: NSVisualEffectView, theme: NativeTheme) {
        material.state = .active
        material.appearance = theme.appearance
        if theme.usesFrostedUtilityMaterial {
            // Within-window frost under clear utility columns; do not clear window opacity.
            material.isHidden = false
            material.blendingMode = .withinWindow
            material.material = .underWindowBackground
            material.alphaValue = max(min(theme.utilityOpacity, 1), 0.35)
        } else {
            // Opaque / tonal / reduced-material: no frost; solid colors own the chrome.
            material.isHidden = true
            material.blendingMode = .behindWindow
            material.alphaValue = 1
        }
    }

    /// Emit non-secret config diagnostics once per process cold load.
    @MainActor
    static func surfaceColdDiagnosticsOnce(for appearance: NSAppearance) {
        ColdDiagnosticsGate.shared.runOnce {
            surfaceDiagnostics(from: visual(for: appearance))
        }
    }

    /// Test hook: allow a subsequent cold-load surface after config reload.
    static func resetColdDiagnosticsSurfacedForTests() {
        ColdDiagnosticsGate.shared.resetForTests()
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

    static func surfaceDiagnostics(from visual: SeyalAppVisual) {
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
        if let surface = view as? MetalSurfaceView {
            surface.renderer.setDefaultTerminalColors(
                foreground: theme.terminalDefaultForeground,
                background: theme.terminalDefaultBackground
            )
        }
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
}
