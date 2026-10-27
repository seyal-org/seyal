import AppKit

/// Adaptive Depth chrome helpers for Core Terminal regions (C03–C05, C09–C10).
/// Product authority stays in Rust projections; this file only paints geometry.
@MainActor
enum ProductChromeAdaptiveDepth {
    static let regionSeamWidth: CGFloat = 1
    static let tabUnderlineHeight: CGFloat = 2
    static let tabChipCornerRadius: CGFloat = 6
    static let tabChipHeight: CGFloat = 28
    static let composerCornerRadius: CGFloat = 8
    static let contextAccentWidth: CGFloat = 2
    static let contextRowMinHeight: CGFloat = 36
}

/// Compact C05 tab chip: title + Close Tab × (ADR-018 hierarchical close).
@MainActor
final class TabChipView: NSStackView {
    let titleButton: IdentityButton
    let closeButton: IdentityButton
    private let titleText: String
    private(set) var isSelected = false
    private var widthConstraint: NSLayoutConstraint?
    private var minWidthConstraint: NSLayoutConstraint?
    private var maxWidthConstraint: NSLayoutConstraint?

    init(
        title: String,
        selected: Bool,
        showClose: Bool,
        tabIndex: Int,
        idLo: UInt64,
        idHi: UInt64,
        containmentGeneration: UInt64,
        target: AnyObject?,
        selectAction: Selector,
        closeAction: Selector
    ) {
        titleText = title
        titleButton = IdentityButton(title: title, target: target, action: selectAction)
        closeButton = IdentityButton(title: "×", target: target, action: closeAction)
        super.init(frame: .zero)
        orientation = .horizontal
        alignment = .centerY
        spacing = 2
        edgeInsets = NSEdgeInsets(top: 2, left: 8, bottom: 2, right: 4)
        translatesAutoresizingMaskIntoConstraints = false
        wantsLayer = true
        layer?.cornerRadius = ProductChromeAdaptiveDepth.tabChipCornerRadius
        layer?.cornerCurve = .continuous
        isSelected = selected
        setContentHuggingPriority(.defaultHigh, for: .horizontal)
        setContentCompressionResistancePriority(.defaultLow, for: .horizontal)

        titleButton.idLo = idLo
        titleButton.idHi = idHi
        titleButton.kind = UInt16(SEYAL_APP_ACTION_SELECT_TAB.rawValue)
        titleButton.bezelStyle = .inline
        titleButton.isBordered = false
        titleButton.setButtonType(.momentaryChange)
        titleButton.font = .systemFont(ofSize: 13, weight: selected ? .semibold : .regular)
        titleButton.setAccessibilityIdentifier("seyal-tab-strip-tab-\(tabIndex)")
        titleButton.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        titleButton.lineBreakMode = .byTruncatingTail
        addArrangedSubview(titleButton)

        closeButton.idLo = idLo
        closeButton.idHi = idHi
        closeButton.kind = UInt16(SEYAL_APP_ACTION_CLOSE_TAB.rawValue)
        closeButton.containmentGeneration = containmentGeneration
        closeButton.bezelStyle = .inline
        closeButton.isBordered = false
        closeButton.setButtonType(.momentaryChange)
        closeButton.font = .systemFont(ofSize: 12, weight: .medium)
        closeButton.toolTip = "Close Tab"
        closeButton.setAccessibilityLabel("Close Tab")
        closeButton.setAccessibilityIdentifier(
            selected ? "seyal-close-tab" : "seyal-tab-strip-close-\(tabIndex)"
        )
        closeButton.isHidden = !showClose
        closeButton.setContentHuggingPriority(.required, for: .horizontal)
        closeButton.setContentCompressionResistancePriority(.required, for: .horizontal)
        addArrangedSubview(closeButton)

        heightAnchor.constraint(equalToConstant: ProductChromeAdaptiveDepth.tabChipHeight).isActive = true
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("TabChipView is programmatic")
    }

    /// Natural title+close width clamped into the Rust min…max band.
    func preferredWidth(minWidth: CGFloat, maxWidth: CGFloat, fontSize: CGFloat) -> CGFloat {
        let titleFont = NSFont.systemFont(
            ofSize: fontSize,
            weight: isSelected ? .semibold : .regular
        )
        let titleWidth = (titleText as NSString).size(withAttributes: [.font: titleFont]).width
        let closeWidth: CGFloat = closeButton.isHidden ? 0 : 14
        let chrome = edgeInsets.left + edgeInsets.right + (closeButton.isHidden ? 0 : spacing)
        return min(max(titleWidth + closeWidth + chrome, minWidth), maxWidth)
    }

    func applyWidth(_ width: CGFloat, minWidth: CGFloat, maxWidth: CGFloat) {
        let clamped = min(max(width, minWidth), maxWidth)
        if let widthConstraint {
            if abs(widthConstraint.constant - clamped) > 0.25 {
                widthConstraint.constant = clamped
            }
        } else {
            let constraint = widthAnchor.constraint(equalToConstant: clamped)
            constraint.priority = .required
            constraint.isActive = true
            widthConstraint = constraint
        }
        if let minWidthConstraint {
            if abs(minWidthConstraint.constant - minWidth) > 0.25 {
                minWidthConstraint.constant = minWidth
            }
        } else {
            let minConstraint = widthAnchor.constraint(greaterThanOrEqualToConstant: minWidth)
            minConstraint.isActive = true
            minWidthConstraint = minConstraint
        }
        if let maxWidthConstraint {
            if abs(maxWidthConstraint.constant - maxWidth) > 0.25 {
                maxWidthConstraint.constant = maxWidth
            }
        } else {
            let maxConstraint = widthAnchor.constraint(lessThanOrEqualToConstant: maxWidth)
            maxConstraint.isActive = true
            maxWidthConstraint = maxConstraint
        }
    }

    func apply(theme: NativeTheme) {
        // VF-4 / C05: active = typography + underline only; no pill fill.
        let titleColor = isSelected ? theme.text : theme.muted
        let titleFont = NSFont.systemFont(
            ofSize: theme.uiFontSize,
            weight: isSelected ? .semibold : .regular
        )
        titleButton.font = titleFont
        titleButton.contentTintColor = titleColor
        titleButton.attributedTitle = NSAttributedString(
            string: titleText,
            attributes: [
                .font: titleFont,
                .foregroundColor: titleColor,
            ]
        )
        let closeFont = NSFont.systemFont(ofSize: max(theme.uiFontSize - 1, 11), weight: .medium)
        closeButton.font = closeFont
        closeButton.contentTintColor = theme.muted
        closeButton.attributedTitle = NSAttributedString(
            string: "×",
            attributes: [
                .font: closeFont,
                .foregroundColor: theme.muted,
            ]
        )
        layer?.backgroundColor = NSColor.clear.cgColor
        applyWidth(
            preferredWidth(
                minWidth: theme.tabMinWidth,
                maxWidth: theme.tabMaxWidth,
                fontSize: theme.uiFontSize
            ),
            minWidth: theme.tabMinWidth,
            maxWidth: theme.tabMaxWidth
        )
    }
}

/// Leading accent + two-line C04 context row. Selection fill is restrained tint.
@MainActor
final class ChromeContextRowView: NSView {
    let button: IdentityButton
    private let accent = NSView()
    private let titleField = NSTextField(labelWithString: "")
    private let detailField = NSTextField(labelWithString: "")
    private let labels = NSStackView()
    private var selected = false
    private var theme: NativeTheme?

    init(
        title: String,
        detail: String?,
        identifier: String,
        selected: Bool,
        showsAccent: Bool = true,
        target: AnyObject?,
        action: Selector,
        kind: UInt16,
        idLo: UInt64,
        idHi: UInt64
    ) {
        button = IdentityButton(title: "", target: target, action: action)
        super.init(frame: .zero)
        translatesAutoresizingMaskIntoConstraints = false
        wantsLayer = true
        self.selected = selected

        button.idLo = idLo
        button.idHi = idHi
        button.kind = kind
        button.bezelStyle = .inline
        button.isBordered = false
        button.setButtonType(.momentaryChange)
        button.title = ""
        button.setAccessibilityIdentifier(identifier)
        button.setAccessibilityLabel(
            detail.flatMap { $0.isEmpty ? nil : $0 }.map { "\(title), \($0)" } ?? title
        )
        // Never use NSButton.state/.on — AppKit paints system selection blue over Adaptive Depth.
        button.setAccessibilityValue(selected ? "selected" : "")
        button.translatesAutoresizingMaskIntoConstraints = false

        accent.wantsLayer = true
        accent.translatesAutoresizingMaskIntoConstraints = false
        accent.isHidden = !(selected && showsAccent)

        titleField.stringValue = title
        titleField.font = .systemFont(ofSize: 12, weight: selected ? .semibold : .regular)
        titleField.lineBreakMode = .byTruncatingTail
        titleField.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        titleField.setAccessibilityElement(false)

        let detailText = detail.flatMap { $0.isEmpty ? nil : $0 }
        detailField.stringValue = detailText ?? ""
        detailField.isHidden = detailText == nil
        detailField.font = .systemFont(ofSize: 10, weight: .regular)
        detailField.lineBreakMode = .byTruncatingTail
        detailField.setAccessibilityElement(false)

        labels.orientation = .vertical
        labels.alignment = .leading
        labels.spacing = 1
        labels.translatesAutoresizingMaskIntoConstraints = false
        labels.addArrangedSubview(titleField)
        labels.addArrangedSubview(detailField)

        addSubview(accent)
        addSubview(labels)
        addSubview(button)
        NSLayoutConstraint.activate([
            heightAnchor.constraint(greaterThanOrEqualToConstant: ProductChromeAdaptiveDepth.contextRowMinHeight),
            accent.leadingAnchor.constraint(equalTo: leadingAnchor),
            accent.topAnchor.constraint(equalTo: topAnchor, constant: 4),
            accent.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -4),
            accent.widthAnchor.constraint(equalToConstant: ProductChromeAdaptiveDepth.contextAccentWidth),
            labels.leadingAnchor.constraint(equalTo: accent.trailingAnchor, constant: 8),
            labels.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -8),
            labels.centerYAnchor.constraint(equalTo: centerYAnchor),
            button.leadingAnchor.constraint(equalTo: leadingAnchor),
            button.trailingAnchor.constraint(equalTo: trailingAnchor),
            button.topAnchor.constraint(equalTo: topAnchor),
            button.bottomAnchor.constraint(equalTo: bottomAnchor),
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("ChromeContextRowView is programmatic")
    }

    var showsSelectionAccent: Bool { !accent.isHidden }

    func apply(theme: NativeTheme) {
        self.theme = theme
        titleField.textColor = selected ? theme.text : theme.secondary
        detailField.textColor = theme.muted
        titleField.font = .systemFont(
            ofSize: max(theme.uiFontSize - 1, 11),
            weight: selected ? .semibold : .regular
        )
        detailField.font = .systemFont(ofSize: max(theme.uiFontSize - 3, 10), weight: .regular)
        // VF §2.4: selected fill is a near-panel lift (UtilityElevated), not ColorRole::Selection.
        accent.layer?.backgroundColor = theme.blockFocus.cgColor
        layer?.backgroundColor = selected ? theme.elevated.cgColor : NSColor.clear.cgColor
    }
}

/// 1 px vertical region seam (C03↔C06 / C06↔C10).
@MainActor
final class ChromeRegionSeamView: NSView {
    init() {
        super.init(frame: .zero)
        translatesAutoresizingMaskIntoConstraints = false
        wantsLayer = true
        setContentHuggingPriority(.required, for: .horizontal)
        setContentCompressionResistancePriority(.required, for: .horizontal)
        widthAnchor.constraint(equalToConstant: ProductChromeAdaptiveDepth.regionSeamWidth).isActive = true
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("ChromeRegionSeamView is programmatic")
    }

    func apply(color: NSColor) {
        layer?.backgroundColor = color.cgColor
    }
}
