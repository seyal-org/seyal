import AppKit

final class TranscriptClipView: NSClipView {
    override var isFlipped: Bool { true }
}

final class IdentityButton: NSButton {
    var kind: UInt16 = 0
    var idLo: UInt64 = 0
    var idHi: UInt64 = 0
    /// Owning PaneId for left C03 TerminalTab rows (#1412); unused otherwise.
    var paneLo: UInt64 = 0
    var paneHi: UInt64 = 0
    var containmentGeneration: UInt64 = 0

    /// Transparent hit target for C04 rows that paint their own labels.
    override func draw(_ dirtyRect: NSRect) {
        if title.isEmpty, image == nil, !isBordered {
            return
        }
        super.draw(dirtyRect)
    }
}

/// Accessibility stand-in for Metal-painted Block output. It is not in Auto
/// Layout: an `NSTextField` intrinsic width shrinks Flow cards to the string
/// and pins them to the trailing edge. `draw` stays empty so Metal remains
/// the only glyphs.
final class BlockBodyTranscript: NSTextField {
    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        isEditable = false
        isSelectable = false
        isBezeled = false
        isBordered = false
        drawsBackground = false
        font = .monospacedSystemFont(ofSize: 13, weight: .regular)
        lineBreakMode = .byClipping
        maximumNumberOfLines = 1
        cell?.isScrollable = false
        translatesAutoresizingMaskIntoConstraints = true
        autoresizingMask = [.width, .height]
        setContentHuggingPriority(.defaultLow, for: .horizontal)
        setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
    }

    override func draw(_ dirtyRect: NSRect) {
        _ = dirtyRect
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("BlockBodyTranscript is programmatic")
    }
}

final class CommandBlockView: NSView {
    let body = NSView()
    private let bodyTranscript = BlockBodyTranscript(frame: .zero)
    /// Host click on the header; `true` when the card is already selected.
    var onSelect: ((Bool) -> Void)?
    /// Projected from the Rust block row's SEYAL_APP_BLOCK_SELECTED flag.
    var isSelected = false {
        didSet {
            guard oldValue != isSelected else { return }
            updateAccessibilityValue()
            if let theme { apply(theme: theme) }
        }
    }
    /// Projected from the Rust block row's SEYAL_APP_BLOCK_SESSION flag.
    private var isSessionBlock: Bool
    private let header = NSView()
    private let prompt = NSTextField(labelWithString: "")
    private let command = NSTextField(labelWithString: "")
    private let status = NSTextField(labelWithString: "")
    private let seam = NSView()
    private var state: UInt16
    private var bodyHeight: NSLayoutConstraint!
    private var seamTopConstraint: NSLayoutConstraint!
    private var theme: NativeTheme?

    static func visibleOutputLineCount(_ text: String) -> Int {
        var lines = text.split(separator: "\n", omittingEmptySubsequences: false)
        while let last = lines.last, last.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            lines.removeLast()
        }
        return max(lines.count, 1)
    }

    init(
        prompt: String,
        title: String,
        detail: String,
        state: UInt16,
        cellHeight: CGFloat,
        lines: Int,
        session: Bool
    ) {
        self.state = state
        self.isSessionBlock = session
        super.init(frame: .zero)
        translatesAutoresizingMaskIntoConstraints = false
        wantsLayer = false
        setAccessibilityElement(true)
        setAccessibilityRole(.group)
        updateAccessibilityValue()
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
        bodyTranscript.setAccessibilityElement(true)
        body.addSubview(bodyTranscript)
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
        // Rust owns session classification; SSH Blocks retain the normal Block geometry.
        seamTopConstraint = seam.topAnchor.constraint(equalTo: body.bottomAnchor, constant: 12)
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
            body.trailingAnchor.constraint(equalTo: header.trailingAnchor),
            body.topAnchor.constraint(equalTo: header.bottomAnchor, constant: 4),
            bodyHeight,
            seam.leadingAnchor.constraint(equalTo: header.leadingAnchor),
            seam.trailingAnchor.constraint(equalTo: header.trailingAnchor),
            seamTopConstraint,
            seam.bottomAnchor.constraint(equalTo: bottomAnchor),
            seam.heightAnchor.constraint(equalToConstant: 1),
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("CommandBlockView is programmatic")
    }

    func setBodyAccessibilityIdentifier(_ identifier: String) {
        bodyTranscript.setAccessibilityIdentifier(identifier)
    }

    func setBodyTranscript(_ text: String) {
        guard bodyTranscript.stringValue != text else { return }
        bodyTranscript.stringValue = text
        // Bound AX payloads so tall Block bodies do not stall XCUI tree walks.
        let ax = Self.accessibilitySummary(for: text)
        bodyTranscript.setAccessibilityLabel(ax)
        bodyTranscript.setAccessibilityValue(ax)
    }

    private func updateAccessibilityValue() {
        var traits: [String] = []
        if isSessionBlock { traits.append("session") }
        if isSelected { traits.append("selected") }
        setAccessibilityValue(traits.joined(separator: ", "))
    }

    private static func accessibilitySummary(for text: String, limit: Int = 240) -> String {
        guard text.count > limit else { return text }
        // Keep both command onset and the live/completed tail available to
        // assistive technology. A tail-only summary made it impossible to
        // verify the first output line after a long Block completed.
        let prefixCount = limit / 10
        let separator = "…\n"
        let suffixCount = max(limit - prefixCount - separator.count, 0)
        return String(text.prefix(prefixCount)) + separator + String(text.suffix(suffixCount))
    }

    override func layout() {
        super.layout()
        bodyTranscript.frame = body.bounds
    }

    func setOutputLines(_ lines: Int, cellHeight: CGFloat) {
        let height = max(cellHeight, 1) * CGFloat(max(lines, 1))
        guard abs(bodyHeight.constant - height) > 0.5 else { return }
        bodyHeight.constant = height
    }

    func updatePresentation(prompt: String, title: String, detail: String, state: UInt16, session: Bool) {
        if self.prompt.stringValue != prompt {
            self.prompt.stringValue = prompt
        }
        let resolvedTitle = title.isEmpty ? "command" : title
        if command.stringValue != resolvedTitle {
            command.stringValue = resolvedTitle
            setAccessibilityLabel(resolvedTitle)
        }
        if status.stringValue != detail {
            status.stringValue = detail
        }
        if status.isHidden != detail.isEmpty {
            status.isHidden = detail.isEmpty
        }
        let appearanceChanged = self.state != state || isSessionBlock != session
        self.state = state
        isSessionBlock = session
        updateAccessibilityValue()
        if appearanceChanged, let theme { apply(theme: theme) }
    }

    override func accessibilityChildren() -> [Any]? {
        // The card is itself an accessibility element, so XCUI cannot see the
        // body (the live-tail target) without an explicit child list. Header
        // labels stay in the tree so VoiceOver still hears prompt, command, and status.
        return [prompt, command, status, bodyTranscript]
    }

    /// Flow's Metal surface returns `nil` from `hitTest`, so Block chrome must
    /// own the click. XCUI (and a user) hit the card center, which is the body
    /// once output exists — a header-only gesture never sees that click.
    override func hitTest(_ point: NSPoint) -> NSView? {
        let hit = super.hitTest(point)
        return hit == nil ? nil : self
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

/// Keeps AppKit Block cards alive while Runtime republishes their projections.
/// Replacing the view on every poll/stream update tears down the transcript
/// subtree and causes visible flashes in multipane Flow.
@MainActor
enum CommandBlockStackReconciler {
    static func reuseOrCreate(
        key: PaneBlockKey,
        previousCards: [PaneBlockKey: CommandBlockView],
        stack: NSStackView,
        make: @MainActor () -> CommandBlockView
    ) -> CommandBlockView {
        if let existing = previousCards[key] {
            if !stack.arrangedSubviews.contains(where: { $0 === existing }) {
                stack.addArrangedSubview(existing)
            }
            return existing
        }
        let card = make()
        stack.addArrangedSubview(card)
        return card
    }

    static func removeStale(
        previousCards: [PaneBlockKey: CommandBlockView],
        retainedKeys: Set<PaneBlockKey>,
        from stack: NSStackView
    ) {
        for (key, card) in previousCards where !retainedKeys.contains(key) {
            stack.removeArrangedSubview(card)
            card.removeFromSuperview()
        }
    }

    static func reuseOrCreate(
        blockID: UInt64,
        previousCards: [UInt64: CommandBlockView],
        stack: NSStackView,
        make: @MainActor () -> CommandBlockView
    ) -> CommandBlockView {
        if blockID != 0, let existing = previousCards[blockID] {
            if !stack.arrangedSubviews.contains(where: { $0 === existing }) {
                stack.addArrangedSubview(existing)
            }
            return existing
        }
        let card = make()
        stack.addArrangedSubview(card)
        return card
    }

    static func removeStale(
        previousCards: [UInt64: CommandBlockView],
        retainedBlockIDs: Set<UInt64>,
        from stack: NSStackView
    ) {
        for (blockID, card) in previousCards where !retainedBlockIDs.contains(blockID) {
            stack.removeArrangedSubview(card)
            card.removeFromSuperview()
        }
    }
}


func productChromeCopyUTF8(_ pointer: UnsafePointer<UInt8>?, _ length: UInt32) -> String? {
    guard length > 0, let pointer else { return nil }
    return String(decoding: UnsafeBufferPointer(start: pointer, count: Int(length)), as: UTF8.self)
}
