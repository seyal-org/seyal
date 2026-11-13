import AppKit

/// Shared AX budget for Block body transcripts: keep the first line plus the
/// newest lines that fit. 480 is the smallest limit that still exposes the
/// full split-sibling acceptance body once padding is trimmed (#1484).
enum BlockAccessibilityText {
    static let limit = 480

    static func trimTrailingPadding(_ line: String) -> String {
        var end = line.endIndex
        while end > line.startIndex {
            let previous = line.index(before: end)
            let scalar = line[previous]
            if scalar != " " && scalar != "\0" { break }
            end = previous
        }
        return String(line[..<end])
    }

    /// First line + newest-tail within `limit`. `line(i)` is 0-based.
    static func summary(lineCount: Int, limit: Int = limit, line: (Int) -> String) -> String {
        guard lineCount > 0 else { return "" }
        let first = line(0)
        if lineCount == 1 {
            return first.count <= limit ? first : String(first.prefix(limit))
        }
        // Phase 1: collect newest lines while the whole text still fits.
        var tail: [String] = []
        var used = first.count
        var reachedStart = false
        for index in stride(from: lineCount - 1, through: 1, by: -1) {
            let text = line(index)
            let next = used + 1 + text.count
            if next > limit { break }
            tail.insert(text, at: 0)
            used = next
            if index == 1 {
                reachedStart = true
            }
        }
        if reachedStart {
            return ([first] + tail).joined(separator: "\n")
        }
        // Phase 2: elide the middle — keep a short onset head and newest tail.
        let separator = "\n…\n"
        let head = String(first.prefix(max(limit / 4, 1)))
        var budget = limit - head.count - separator.count
        if budget <= 0 {
            return String(first.prefix(limit))
        }
        while !tail.isEmpty {
            let joined = tail.joined(separator: "\n")
            if joined.count <= budget { break }
            tail.removeFirst()
        }
        if tail.isEmpty {
            let newest = line(lineCount - 1)
            if newest.count <= budget {
                return head + separator + newest
            }
            return head + separator + String(newest.suffix(budget))
        }
        // Prefer filling leftover budget with more newest lines when phase-1
        // stopped early because of the full-text reserve.
        var nextIndex = lineCount - 1 - tail.count
        while nextIndex >= 1 {
            let text = line(nextIndex)
            let joinedCost = text.count + 1 + tail.joined(separator: "\n").count
            if joinedCost > budget { break }
            tail.insert(text, at: 0)
            nextIndex -= 1
        }
        return head + separator + tail.joined(separator: "\n")
    }
}

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
    /// Explicit ADR-026 setup action. Candidate state is offered from the
    /// running host Block's context menu, never as an automatic banner.
    var canRequestSessionBootstrap = false
    var onRequestSessionBootstrap: (() -> Void)?
    /// Explicit fallback for foreground programs that require direct terminal
    /// input. The request is fenced against the current Runtime target in Rust.
    var canInteractWithRunningProgram = false {
        didSet {
            guard oldValue != canInteractWithRunningProgram else { return }
            interactButton.isHidden = !canInteractWithRunningProgram
            if canInteractWithRunningProgram {
                statusTrailingToHeader.isActive = false
                statusTrailingToInteractButton.isActive = true
            } else {
                statusTrailingToInteractButton.isActive = false
                statusTrailingToHeader.isActive = true
            }
        }
    }
    var onInteractWithRunningProgram: (() -> Void)?
    /// Flow DirectInput: click the Running Block to focus Metal (#1468).
    var onRequestDirectInputFocus: (() -> Void)?
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
    private let interactButton = NSButton(title: "Interact", target: nil, action: nil)
    private let seam = NSView()
    private var state: UInt16
    private var bodyHeight: NSLayoutConstraint!
    private var seamTopConstraint: NSLayoutConstraint!
    private var statusTrailingToHeader: NSLayoutConstraint!
    private var statusTrailingToInteractButton: NSLayoutConstraint!
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
        interactButton.isBordered = false
        interactButton.bezelStyle = .inline
        interactButton.font = .systemFont(ofSize: 11, weight: .medium)
        interactButton.target = self
        interactButton.action = #selector(interactWithRunningProgramFromMenu)
        interactButton.setAccessibilityLabel("Interact with running program")
        interactButton.setAccessibilityIdentifier("seyal-running-program-interact")
        interactButton.isHidden = true
        interactButton.translatesAutoresizingMaskIntoConstraints = false
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
        header.addSubview(interactButton)
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
        statusTrailingToHeader = status.trailingAnchor.constraint(equalTo: header.trailingAnchor)
        statusTrailingToInteractButton = status.trailingAnchor.constraint(
            lessThanOrEqualTo: interactButton.leadingAnchor,
            constant: -8
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
            statusTrailingToHeader,
            interactButton.trailingAnchor.constraint(equalTo: header.trailingAnchor, constant: -8),
            interactButton.centerYAnchor.constraint(equalTo: header.centerYAnchor),
            interactButton.widthAnchor.constraint(greaterThanOrEqualToConstant: 44),
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

    /// First non-empty body line, retained across LiveEnd page swaps so AX
    /// summaries keep stream onset (#1484 SplitFromLivePane / FourPane).
    private(set) var bodyAccessibilityOnsetLine: String = ""

    var bodyAccessibilityIsEmpty: Bool {
        bodyTranscript.stringValue.isEmpty
    }

    func setBodyTranscript(_ text: String) {
        // Prefer caller-budgeted `accessibilityTranscript()`; only summarize
        // when raw text exceeds the AX limit so a second pass cannot drop the
        // live-end tail (#1484 100k / LongBlock AX).
        var ax = text.count <= BlockAccessibilityText.limit
            ? text
            : Self.accessibilitySummary(for: text)
        if bodyAccessibilityOnsetLine.isEmpty {
            let first = ax.split(whereSeparator: \.isNewline)
                .map(String.init)
                .first { !$0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
            if let first { bodyAccessibilityOnsetLine = first }
        } else if !ax.isEmpty, !ax.contains(bodyAccessibilityOnsetLine) {
            // Prepend preserved onset so LiveEnd pages still expose line 1.
            ax = bodyAccessibilityOnsetLine + "\n" + ax
            if ax.count > BlockAccessibilityText.limit {
                ax = Self.accessibilitySummary(for: ax)
            }
        }
        // Never erase a populated body with a transient empty retain — multipane
        // unfocused stream / focus-churn tests observe AX markers while HISTORY
        // pages are still in flight (#1484 FlowSplitSibling / FocusChurn).
        if ax.isEmpty, !bodyTranscript.stringValue.isEmpty { return }
        // Never demote LiveEnd-rich AX back to an onset-only latch (#1484 FourPane).
        let current = bodyTranscript.stringValue
        func nonEmptyLines(_ text: String) -> Int {
            text.split(whereSeparator: \.isNewline)
                .filter { !$0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
                .count
        }
        let currentLines = nonEmptyLines(current)
        let newLines = nonEmptyLines(ax)
        if currentLines > 1,
           ax == bodyAccessibilityOnsetLine || (newLines <= 2 && newLines < currentLines)
        {
            return
        }
        guard current != ax else { return }
        bodyTranscript.stringValue = ax
        bodyTranscript.setAccessibilityLabel(ax)
        bodyTranscript.setAccessibilityValue(ax)
    }

    private func updateAccessibilityValue() {
        var traits: [String] = []
        if isSessionBlock { traits.append("session") }
        if isSelected { traits.append("selected") }
        setAccessibilityValue(traits.joined(separator: ", "))
    }

    /// Bound AX payloads so tall Block bodies do not stall XCUI tree walks,
    /// while still exposing command onset and the live/completed tail (#1484).
    /// Trailing cell padding is stripped per line first so short streamed
    /// markers remain discoverable inside the budget (terminal rows are often
    /// space-padded to the full grid width).
    private static func accessibilitySummary(for text: String, limit: Int = BlockAccessibilityText.limit) -> String {
        let trimmedLines = text.split(separator: "\n", omittingEmptySubsequences: false).map { line in
            BlockAccessibilityText.trimTrailingPadding(String(line))
        }
        // Drop trailing blank rows so COMPLETE/tail markers stay in budget.
        var end = trimmedLines.count
        while end > 0 && trimmedLines[end - 1].isEmpty {
            end -= 1
        }
        let lines = Array(trimmedLines.prefix(end))
        return BlockAccessibilityText.summary(lineCount: lines.count, limit: limit) { lines[$0] }
    }

    override func layout() {
        super.layout()
        bodyTranscript.frame = body.bounds
    }

    func setOutputLines(_ lines: Int, cellHeight: CGFloat, expandOnly: Bool = false) {
        let height = max(cellHeight, 1) * CGFloat(max(lines, 1))
        // Mid-transcript scroll retention measures body frame.minY. A later
        // viewport-sized HISTORY page must not shrink a taller sealed body and
        // yank that frame under a preserved scroll offset (#1484 FocusChurn).
        if expandOnly, height + 0.5 < bodyHeight.constant { return }
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
        var children: [Any] = [prompt, command, status]
        if canInteractWithRunningProgram {
            children.append(interactButton)
        }
        children.append(bodyTranscript)
        return children
    }

    /// Flow's Metal surface returns `nil` from `hitTest`, so Block chrome must
    /// own the click. XCUI (and a user) hit the card center, which is the body
    /// once output exists — a header-only gesture never sees that click.
    override func hitTest(_ point: NSPoint) -> NSView? {
        let hit = super.hitTest(point)
        // Keep the explicit Running Block action interactive. Returning the
        // Block for every child hit previously swallowed NSButton mouse events.
        if hit === interactButton || hit?.isDescendant(of: interactButton) == true {
            return hit
        }
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
        onRequestDirectInputFocus?()
        onSelect?(isSelected)
    }

    override func menu(for event: NSEvent) -> NSMenu? {
        guard canRequestSessionBootstrap || canInteractWithRunningProgram else {
            return super.menu(for: event)
        }
        let menu = NSMenu()
        if canInteractWithRunningProgram {
            let interact = NSMenuItem(
                title: "Interact with running program",
                action: #selector(interactWithRunningProgramFromMenu),
                keyEquivalent: ""
            )
            interact.target = self
            menu.addItem(interact)
        }
        if canRequestSessionBootstrap {
        let enable = NSMenuItem(
            title: seyal_bridge_session_copy(4).map { String(cString: $0) } ?? "",
            action: #selector(requestSessionBootstrapFromMenu),
            keyEquivalent: ""
        )
        enable.target = self
        menu.addItem(enable)
        }
        return menu
    }

    @objc private func interactWithRunningProgramFromMenu() {
        onInteractWithRunningProgram?()
    }

    @objc private func requestSessionBootstrapFromMenu() {
        onRequestSessionBootstrap?()
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
        interactButton.contentTintColor = theme.accent
    }
}

/// Keeps AppKit Block cards alive while Runtime republishes their projections.
/// Replacing the view on every poll/stream update tears down the transcript
/// subtree and causes visible flashes in multipane Flow.
@MainActor
enum CommandBlockStackReconciler {
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
