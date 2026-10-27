import AppKit

/// Adaptive Depth chrome helpers for Core Terminal regions (C03–C05, C09–C10).
/// Product authority stays in Rust projections; this file only paints geometry.
@MainActor
enum ProductChromeAdaptiveDepth {
    static let regionSeamWidth: CGFloat = 1
    static let tabUnderlineHeight: CGFloat = 2
    static let composerCornerRadius: CGFloat = 8
    static let contextAccentWidth: CGFloat = 2
    static let contextRowMinHeight: CGFloat = 36
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
        button.title = ""
        button.setAccessibilityIdentifier(identifier)
        button.setAccessibilityLabel(
            detail.flatMap { $0.isEmpty ? nil : $0 }.map { "\(title), \($0)" } ?? title
        )
        button.state = selected ? .on : .off
        button.translatesAutoresizingMaskIntoConstraints = false

        accent.wantsLayer = true
        accent.translatesAutoresizingMaskIntoConstraints = false
        accent.isHidden = !selected

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
        accent.layer?.backgroundColor = theme.blockFocus.cgColor
        layer?.backgroundColor = selected ? theme.selection.cgColor : NSColor.clear.cgColor
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
