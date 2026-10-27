import AppKit
import Foundation

/// Native CoreText font request. Product theme/config authority stays in Rust (#740).
struct SeyalResolvedFontSpec: Equatable, Sendable {
    var family: String
    var fallbacks: [String]
    var pointSize: CGFloat

    /// Cold-start mirror of Rust `UserUiSettings` terminal font defaults
    /// (`crates/seyal-client/src/theme/config.rs`). Prefer `fromProcessVisual()`
    /// once `seyal_app_visual` is available so family/size follow config.
    static let canonicalTerminal = SeyalResolvedFontSpec(
        family: "Menlo",
        fallbacks: [
            "SF Mono",
            "Menlo",
            "JetBrainsMono NFM",
            "JetBrainsMono NF",
            "Symbols Nerd Font Mono",
            "MesloLGS Nerd Font Mono",
            "Courier",
        ],
        pointSize: 14
    )

    /// Map Rust-owned visual font family/size onto the Metal atlas resolver.
    /// Fallbacks stay the Rust default cascade until the visual ABI exports them.
    static func fromProcessVisual(appearance: NSAppearance? = nil) -> SeyalResolvedFontSpec {
        let code: UInt16
        if let appearance, appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua {
            code = 0
        } else {
            code = 1
        }
        let packed = seyal_app_visual(code)
        let family: String
        if packed.terminal_font_family_len > 0, let bytes = packed.terminal_font_family {
            family = String(
                decoding: UnsafeBufferPointer(start: bytes, count: Int(packed.terminal_font_family_len)),
                as: UTF8.self
            )
        } else {
            family = canonicalTerminal.family
        }
        let size = packed.terminal_font_size > 0
            ? CGFloat(packed.terminal_font_size)
            : canonicalTerminal.pointSize
        return SeyalResolvedFontSpec(
            family: family.isEmpty ? canonicalTerminal.family : family,
            fallbacks: canonicalTerminal.fallbacks,
            pointSize: size
        )
    }
}
