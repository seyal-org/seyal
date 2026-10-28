import AppKit

final class TranscriptClipView: NSClipView {
    override var isFlipped: Bool { true }
}

final class IdentityButton: NSButton {
    var kind: UInt16 = 0
    var idLo: UInt64 = 0
    var idHi: UInt64 = 0
    var containmentGeneration: UInt64 = 0
}

/// Accessibility stand-in for Metal-painted Block output. The glyphs stay on
/// the renderer; this label only publishes the retained transcript to XCUI
/// and VoiceOver. `draw` is empty so the string is not painted twice.
final class BlockBodyView: NSTextField {
    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        isEditable = false
        isSelectable = false
        isBezeled = false
        isBordered = false
        drawsBackground = false
        font = .monospacedSystemFont(ofSize: 13, weight: .regular)
        lineBreakMode = .byWordWrapping
        maximumNumberOfLines = 0
        cell?.wraps = true
        cell?.isScrollable = false
        setContentHuggingPriority(.defaultLow, for: .horizontal)
        setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
    }

    override func draw(_ dirtyRect: NSRect) {
        _ = dirtyRect
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("BlockBodyView is programmatic")
    }
}

final class CommandBlockView: NSView {
    let body = BlockBodyView(frame: .zero)
    /// Host click on the header; `true` when the card is already selected.
    var onSelect: ((Bool) -> Void)?
    /// Projected from the Rust block row's SEYAL_APP_BLOCK_SELECTED flag.
    var isSelected = false {
        didSet {
            setAccessibilityValue(isSelected ? "selected" : "")
            if let theme { apply(theme: theme) }
        }
    }
    private let header = NSView()
    private let prompt = NSTextField(labelWithString: "")
    private let command = NSTextField(labelWithString: "")
    private let status = NSTextField(labelWithString: "")
    private let seam = NSView()
    private let state: UInt16
    private var bodyHeight: NSLayoutConstraint!
    private var theme: NativeTheme?

    init(
        prompt: String,
        title: String,
        detail: String,
        state: UInt16,
        cellHeight: CGFloat,
        lines: Int
    ) {
        self.state = state
        super.init(frame: .zero)
        translatesAutoresizingMaskIntoConstraints = false
        wantsLayer = false
        setAccessibilityElement(true)
        setAccessibilityRole(.group)
        self.prompt.stringValue = prompt
        self.prompt.font = .monospacedSystemFont(ofSize: 13, weight: .medium)
        self.prompt.setContentHuggingPriority(.required, for: .horizontal)
        self.prompt.translatesAutoresizingMaskIntoConstraints = false
        command.stringValue = title.isEmpty ? "command" : title
        setAccessibilityLabel(command.stringValue)
        command.font = .monospacedSystemFont(ofSize: 13, weight: .medium)
        command.lineBreakMode = .byTruncatingTail
        command.translatesAutoresizingMaskIntoConstraints = false
        status.stringValue = detail
        status.isHidden = detail.isEmpty
        status.font = .monospacedSystemFont(ofSize: 11, weight: .regular)
        status.tag = 2
        status.setContentHuggingPriority(.required, for: .horizontal)
        status.translatesAutoresizingMaskIntoConstraints = false
        seam.translatesAutoresizingMaskIntoConstraints = false
        seam.wantsLayer = true
        header.translatesAutoresizingMaskIntoConstraints = false
        body.translatesAutoresizingMaskIntoConstraints = false
        body.wantsLayer = true
        body.layer?.isOpaque = false
        body.layer?.backgroundColor = NSColor.clear.cgColor
        body.setAccessibilityElement(true)
        header.addSubview(self.prompt)
        header.addSubview(command)
        header.addSubview(status)
        header.wantsLayer = true
        header.layer?.cornerRadius = 6
        addSubview(header)
        addSubview(body)
        addSubview(seam)
        bodyHeight = body.heightAnchor.constraint(
            equalToConstant: max(cellHeight, 1) * CGFloat(max(lines, 1))
        )
        NSLayoutConstraint.activate([
            header.leadingAnchor.constraint(equalTo: leadingAnchor),
            header.trailingAnchor.constraint(equalTo: trailingAnchor),
            header.topAnchor.constraint(equalTo: topAnchor),
            header.heightAnchor.constraint(greaterThanOrEqualToConstant: 22),
            self.prompt.leadingAnchor.constraint(equalTo: header.leadingAnchor),
            self.prompt.centerYAnchor.constraint(equalTo: header.centerYAnchor),
            command.leadingAnchor.constraint(equalTo: self.prompt.trailingAnchor, constant: 8),
            command.centerYAnchor.constraint(equalTo: header.centerYAnchor),
            status.trailingAnchor.constraint(equalTo: header.trailingAnchor),
            status.centerYAnchor.constraint(equalTo: header.centerYAnchor),
            command.trailingAnchor.constraint(lessThanOrEqualTo: status.leadingAnchor, constant: -12),
            body.leadingAnchor.constraint(equalTo: command.leadingAnchor),
            body.trailingAnchor.constraint(equalTo: trailingAnchor),
            body.topAnchor.constraint(equalTo: header.bottomAnchor, constant: 4),
            bodyHeight,
            seam.leadingAnchor.constraint(equalTo: leadingAnchor),
            seam.trailingAnchor.constraint(equalTo: trailingAnchor),
            seam.topAnchor.constraint(equalTo: body.bottomAnchor, constant: 12),
            seam.bottomAnchor.constraint(equalTo: bottomAnchor),
            seam.heightAnchor.constraint(equalToConstant: 1),
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("CommandBlockView is programmatic")
    }

    func setBodyTranscript(_ text: String) {
        body.stringValue = text
    }

    func setOutputLines(_ lines: Int, cellHeight: CGFloat) {
        bodyHeight.constant = max(cellHeight, 1) * CGFloat(max(lines, 1))
    }

    override func accessibilityChildren() -> [Any]? {
        // The card is itself an accessibility element, so XCUI cannot see the
        // body (the live-tail target) without an explicit child list. Header
        // labels stay in the tree so VoiceOver still hears prompt, command, and status.
        [prompt, command, status, body]
    }

    /// Flow's Metal surface returns `nil` from `hitTest`, so Block chrome must
    /// own the click. XCUI (and a user) hit the card center, which is the body
    /// once output exists — a header-only gesture never sees that click.
    override func hitTest(_ point: NSPoint) -> NSView? {
        super.hitTest(point) == nil ? nil : self
    }

    /// `hitTest` returns `self` for selection, which would otherwise swallow
    /// trackpad/mouse wheel over Block bodies. Flow scrolling is Pane-owned by
    /// the transcript `NSScrollView` (#1379 Gate 1).
    override func scrollWheel(with event: NSEvent) {
        if let scroll = enclosingScrollView {
            scroll.scrollWheel(with: event)
            return
        }
        super.scrollWheel(with: event)
    }

    override func mouseDown(with event: NSEvent) {
        onSelect?(isSelected)
    }

    func apply(theme: NativeTheme) {
        self.theme = theme
        body.layer?.isOpaque = false
        body.layer?.backgroundColor = NSColor.clear.cgColor
        prompt.font = .monospacedSystemFont(ofSize: theme.terminalFontSize, weight: .medium)
        command.font = .monospacedSystemFont(ofSize: theme.terminalFontSize, weight: .medium)
        status.font = .monospacedSystemFont(ofSize: max(theme.terminalFontSize - 2, 9), weight: .regular)
        prompt.textColor = theme.accent
        command.textColor = theme.accent
        header.layer?.backgroundColor = isSelected
            ? theme.accent.withAlphaComponent(0.14).cgColor
            : NSColor.clear.cgColor
        if state == UInt16(SEYAL_APP_BLOCK_STATE_FAILED) {
            status.textColor = theme.danger
            seam.layer?.backgroundColor = theme.danger.withAlphaComponent(0.45).cgColor
        } else {
            status.textColor = theme.muted
            seam.layer?.backgroundColor = theme.seam.cgColor
        }
    }
}


func productChromeCopyUTF8(_ pointer: UnsafePointer<UInt8>?, _ length: UInt32) -> String? {
    guard length > 0, let pointer else { return nil }
    return String(decoding: UnsafeBufferPointer(start: pointer, count: Int(length)), as: UTF8.self)
}
