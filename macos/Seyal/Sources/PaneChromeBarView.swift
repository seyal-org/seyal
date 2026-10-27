import AppKit

/// cmux-style **per-pane surface strip** (visual target), projected inside Seyal's
/// Tab-owned Pane model (`M001-MULTIPANE-VIEW` §2/§4).
///
/// Mapping:
/// - cmux Surface chip ↔ this leading chip (Rust pane title; one TerminalExecution
///   per Pane today — not a second execution / fake multi-surface state).
/// - Seyal C05 Tab strip stays Workspace→Tab (shell/ops/deploy); this strip is
///   the in-Pane header under that Tab after Split.
///
/// **No pane-local `+`:** creating another TerminalExecution inside the same Pane
/// needs an ADR (Pane→multi-Surface). Workspace `+` / ⌘T remains CREATE_TAB at
/// Tab scope on the C05 strip.
@MainActor
final class PaneChromeBarView: NSView {
    var onClose: (() -> Void)?

    /// Leading selected-surface chip (title + Close Pane).
    private let surfaceChip = NSView()
    private let titleLabel = NSTextField(labelWithString: "")
    private let closeButton = NSButton(title: "×", target: nil, action: nil)
    private let chipUnderline = NSView()
    private let bottomSeam = NSView()
    private var chipUnderlineHeight: NSLayoutConstraint?

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        translatesAutoresizingMaskIntoConstraints = false
        wantsLayer = true
        setAccessibilityElement(true)
        setAccessibilityRole(.group)
        setAccessibilityIdentifier("seyal-pane-chrome")
        setAccessibilityLabel("Pane surface strip")

        surfaceChip.translatesAutoresizingMaskIntoConstraints = false
        surfaceChip.wantsLayer = true
        surfaceChip.layer?.cornerRadius = ProductChromeAdaptiveDepth.tabChipCornerRadius
        surfaceChip.layer?.cornerCurve = .continuous
        surfaceChip.setAccessibilityElement(true)
        surfaceChip.setAccessibilityRole(.group)
        surfaceChip.setAccessibilityIdentifier("seyal-pane-surface-chip")

        titleLabel.font = .systemFont(ofSize: 12, weight: .semibold)
        titleLabel.lineBreakMode = .byTruncatingMiddle
        titleLabel.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        titleLabel.setContentHuggingPriority(.defaultHigh, for: .horizontal)
        titleLabel.translatesAutoresizingMaskIntoConstraints = false
        titleLabel.setAccessibilityIdentifier("seyal-pane-chrome-title")

        closeButton.bezelStyle = .inline
        closeButton.isBordered = false
        closeButton.setButtonType(.momentaryChange)
        closeButton.font = .systemFont(ofSize: 12, weight: .medium)
        closeButton.toolTip = "Close Pane"
        closeButton.setAccessibilityLabel("Close Pane")
        closeButton.setAccessibilityIdentifier("seyal-pane-chrome-close")
        closeButton.target = self
        closeButton.action = #selector(closeClicked)
        closeButton.translatesAutoresizingMaskIntoConstraints = false
        closeButton.setContentHuggingPriority(.required, for: .horizontal)
        closeButton.setContentCompressionResistancePriority(.required, for: .horizontal)

        chipUnderline.translatesAutoresizingMaskIntoConstraints = false
        chipUnderline.wantsLayer = true
        chipUnderline.isHidden = true

        bottomSeam.translatesAutoresizingMaskIntoConstraints = false
        bottomSeam.wantsLayer = true

        surfaceChip.addSubview(titleLabel)
        surfaceChip.addSubview(closeButton)
        surfaceChip.addSubview(chipUnderline)
        addSubview(surfaceChip)
        addSubview(bottomSeam)

        let underlineHeight = chipUnderline.heightAnchor.constraint(
            equalToConstant: ProductChromeAdaptiveDepth.tabUnderlineHeight
        )
        chipUnderlineHeight = underlineHeight

        NSLayoutConstraint.activate([
            heightAnchor.constraint(equalToConstant: ProductChromeAdaptiveDepth.tabChipHeight),

            surfaceChip.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 8),
            surfaceChip.topAnchor.constraint(equalTo: topAnchor, constant: 2),
            surfaceChip.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -2),
            surfaceChip.trailingAnchor.constraint(lessThanOrEqualTo: trailingAnchor, constant: -8),

            titleLabel.leadingAnchor.constraint(equalTo: surfaceChip.leadingAnchor, constant: 8),
            titleLabel.centerYAnchor.constraint(equalTo: surfaceChip.centerYAnchor, constant: -1),
            titleLabel.trailingAnchor.constraint(equalTo: closeButton.leadingAnchor, constant: -2),

            closeButton.trailingAnchor.constraint(equalTo: surfaceChip.trailingAnchor, constant: -4),
            closeButton.centerYAnchor.constraint(equalTo: surfaceChip.centerYAnchor, constant: -1),
            closeButton.widthAnchor.constraint(equalToConstant: 18),
            closeButton.heightAnchor.constraint(equalToConstant: 18),

            chipUnderline.leadingAnchor.constraint(equalTo: titleLabel.leadingAnchor),
            chipUnderline.trailingAnchor.constraint(equalTo: titleLabel.trailingAnchor),
            chipUnderline.bottomAnchor.constraint(equalTo: surfaceChip.bottomAnchor),
            underlineHeight,

            bottomSeam.leadingAnchor.constraint(equalTo: leadingAnchor),
            bottomSeam.trailingAnchor.constraint(equalTo: trailingAnchor),
            bottomSeam.bottomAnchor.constraint(equalTo: bottomAnchor),
            bottomSeam.heightAnchor.constraint(equalToConstant: ProductChromeAdaptiveDepth.regionSeamWidth),
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("PaneChromeBarView is programmatic")
    }

    /// Title is Rust-owned pane title only — never invent cwd/path strings here.
    func apply(title: String, theme: NativeTheme?, focused: Bool = false) {
        titleLabel.stringValue = title
        surfaceChip.setAccessibilityLabel(title)
        setAccessibilityLabel("Pane surface: \(title)")
        guard let theme else { return }

        // VF-4 / Adaptive Depth: focused = typography + underline; no heavy card.
        let titleColor = focused ? theme.text : theme.muted
        let titleFont = NSFont.systemFont(
            ofSize: max(theme.uiFontSize - 1, 11),
            weight: focused ? .semibold : .regular
        )
        titleLabel.font = titleFont
        titleLabel.textColor = titleColor
        closeButton.contentTintColor = theme.muted
        closeButton.font = .systemFont(ofSize: max(theme.uiFontSize - 1, 11), weight: .medium)

        layer?.backgroundColor = theme.canvas.cgColor
        surfaceChip.layer?.backgroundColor = NSColor.clear.cgColor
        chipUnderline.isHidden = !focused
        chipUnderline.layer?.backgroundColor = theme.blockFocus.cgColor
        bottomSeam.layer?.backgroundColor = (focused ? theme.blockFocus.withAlphaComponent(0.45) : theme.seam)
            .cgColor
    }

    func setCloseVisible(_ visible: Bool) {
        closeButton.isHidden = !visible
    }

    @objc private func closeClicked() {
        onClose?()
    }
}
