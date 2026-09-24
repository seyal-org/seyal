import AppKit

/// Quick actions a Block can request (M003-BLOCK-COMPONENT-DESIGN §6). The
/// host routes each to Rust (Rerun, Inspect) or to the pasteboard with
/// Rust-built text (Copy); this view owns no product state.
enum CommandBlockAction {
    case copyCommand
    case copyOutput
    case copyCommandAndOutput
    case rerun
    case inspect
}

/// Snapshot of one Rust Block row the view renders.
struct CommandBlockRow {
    let command: String
    let state: UInt16
    let isSelected: Bool
    let canRerun: Bool
}

/// AppKit chrome around one Runtime Block (`C-BLOCK`). Terminal output is
/// Metal-composited into `body`; this view draws the surface, the top line
/// (staged command label + semantic seam) and the focus border only.
@MainActor
final class CommandBlockView: NSView {
    /// Metal composites terminal output into this clip. Must stay transparent.
    let body = NSView()
    /// Click on the Block; argument is `true` when it is already selected.
    var onSelect: ((Bool) -> Void)?
    var onAction: ((CommandBlockAction) -> Void)?

    private enum Metrics {
        static let insetH: CGFloat = 14
        static let insetBottom: CGFloat = 10
        static let radius: CGFloat = 6
        static let border: CGFloat = 1
        static let focusBorder: CGFloat = 1.5
        static let minTopLine: CGFloat = 24
        static let statusSize: CGFloat = 14
    }

    private let row: CommandBlockRow
    private let topLine = NSView()
    private let command = NSTextField(labelWithString: "")
    private let actions = NSStackView()
    private let statusIcon = NSImageView()
    private let spinner = NSProgressIndicator()
    private let copyButton = CommandBlockView.actionButton(
        symbol: "doc.on.doc", label: "Copy", id: "copy")
    private let rerunButton = CommandBlockView.actionButton(
        symbol: "play", label: "Rerun", id: "rerun")
    private let moreButton = CommandBlockView.actionButton(
        symbol: "ellipsis", label: "More", id: "more")
    private var bodyHeight: NSLayoutConstraint!
    private var theme: NativeTheme?
    private var isHovered = false
    private var hoverArea: NSTrackingArea?

    init(row: CommandBlockRow, cellHeight: CGFloat, lines: Int) {
        self.row = row
        super.init(frame: .zero)
        translatesAutoresizingMaskIntoConstraints = false
        wantsLayer = true
        layer?.cornerRadius = Metrics.radius
        setAccessibilityElement(true)
        setAccessibilityRole(.group)
        setAccessibilityLabel(row.command)
        setAccessibilityValue(row.isSelected ? "selected" : "")

        // Staged until #1042 draws the shell's own command row in Metal.
        command.stringValue = row.command
        command.font = .monospacedSystemFont(ofSize: 12, weight: .regular)
        command.lineBreakMode = .byTruncatingTail
        command.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        command.translatesAutoresizingMaskIntoConstraints = false

        statusIcon.imageScaling = .scaleProportionallyUpOrDown
        statusIcon.setAccessibilityElement(true)
        statusIcon.setAccessibilityRole(.image)
        statusIcon.setAccessibilityIdentifier("seyal-block-status")
        statusIcon.setAccessibilityLabel(Self.statusName(row.state))
        statusIcon.translatesAutoresizingMaskIntoConstraints = false
        spinner.style = .spinning
        spinner.controlSize = .small
        spinner.isDisplayedWhenStopped = false
        spinner.translatesAutoresizingMaskIntoConstraints = false
        // While running the spinner replaces the icon, so it carries the same
        // status identity for VoiceOver (a hidden icon leaves the AX tree).
        spinner.setAccessibilityElement(true)
        spinner.setAccessibilityIdentifier("seyal-block-status")
        spinner.setAccessibilityLabel(Self.statusName(UInt16(SEYAL_APP_BLOCK_STATE_RUNNING)))

        copyButton.target = self
        copyButton.action = #selector(showCopyMenu(_:))
        rerunButton.target = self
        rerunButton.action = #selector(rerun)
        rerunButton.isEnabled = row.canRerun
        moreButton.target = self
        moreButton.action = #selector(showMoreMenu(_:))
        actions.orientation = .horizontal
        actions.spacing = 2
        actions.translatesAutoresizingMaskIntoConstraints = false
        [copyButton, rerunButton, moreButton].forEach(actions.addArrangedSubview)

        topLine.translatesAutoresizingMaskIntoConstraints = false
        [command, actions, statusIcon, spinner].forEach(topLine.addSubview)
        body.translatesAutoresizingMaskIntoConstraints = false
        body.wantsLayer = true
        body.layer?.isOpaque = false
        body.layer?.backgroundColor = NSColor.clear.cgColor
        body.setAccessibilityElement(true)
        body.setAccessibilityRole(.group)
        addSubview(topLine)
        addSubview(body)

        bodyHeight = body.heightAnchor.constraint(
            equalToConstant: max(cellHeight, 1) * CGFloat(max(lines, 1)))
        NSLayoutConstraint.activate([
            topLine.leadingAnchor.constraint(equalTo: leadingAnchor, constant: Metrics.insetH),
            topLine.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -10),
            topLine.topAnchor.constraint(equalTo: topAnchor, constant: 2),
            topLine.heightAnchor.constraint(
                equalToConstant: max(cellHeight, Metrics.minTopLine) + 4),
            command.leadingAnchor.constraint(equalTo: topLine.leadingAnchor),
            command.centerYAnchor.constraint(equalTo: topLine.centerYAnchor),
            command.trailingAnchor.constraint(
                lessThanOrEqualTo: actions.leadingAnchor, constant: -12),
            actions.trailingAnchor.constraint(equalTo: statusIcon.leadingAnchor, constant: -8),
            actions.centerYAnchor.constraint(equalTo: topLine.centerYAnchor),
            statusIcon.trailingAnchor.constraint(equalTo: topLine.trailingAnchor),
            statusIcon.centerYAnchor.constraint(equalTo: topLine.centerYAnchor),
            statusIcon.widthAnchor.constraint(equalToConstant: Metrics.statusSize),
            statusIcon.heightAnchor.constraint(equalToConstant: Metrics.statusSize),
            spinner.centerXAnchor.constraint(equalTo: statusIcon.centerXAnchor),
            spinner.centerYAnchor.constraint(equalTo: statusIcon.centerYAnchor),
            spinner.widthAnchor.constraint(equalToConstant: Metrics.statusSize),
            spinner.heightAnchor.constraint(equalToConstant: Metrics.statusSize),
            body.leadingAnchor.constraint(equalTo: leadingAnchor, constant: Metrics.insetH),
            body.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -Metrics.insetH),
            body.topAnchor.constraint(equalTo: topLine.bottomAnchor),
            bodyHeight,
            body.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -Metrics.insetBottom),
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("CommandBlockView is programmatic")
    }

    /// Hover or selection reveals the quick actions (design §6, board rule 3).
    var actionsVisible: Bool { !actions.isHidden }

    func setOutputLines(_ lines: Int, cellHeight: CGFloat) {
        bodyHeight.constant = max(cellHeight, 1) * CGFloat(max(lines, 1))
    }

    func apply(theme: NativeTheme) {
        self.theme = theme
        refresh()
    }

    /// Flow's Metal surface returns `nil` from `hitTest`, so Block chrome owns
    /// the click. Seam buttons keep their own hits; everything else selects.
    override func hitTest(_ point: NSPoint) -> NSView? {
        guard let hit = super.hitTest(point) else { return nil }
        if hit is NSButton, hit.isDescendant(of: actions) { return hit }
        return self
    }

    override func mouseDown(with event: NSEvent) {
        onSelect?(row.isSelected)
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let hoverArea { removeTrackingArea(hoverArea) }
        let area = NSTrackingArea(
            rect: .zero,
            options: [.mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect],
            owner: self,
            userInfo: nil)
        addTrackingArea(area)
        hoverArea = area
        // Cards are rebuilt when Rust Block rows change (e.g. a running Block);
        // a rebuilt card under the pointer must stay hovered, not flicker.
        if let window {
            let pointer = convert(window.mouseLocationOutsideOfEventStream, from: nil)
            let inside = bounds.contains(pointer)
            if inside != isHovered {
                isHovered = inside
                refresh()
            }
        }
    }

    override func mouseEntered(with event: NSEvent) {
        isHovered = true
        refresh()
    }

    override func mouseExited(with event: NSEvent) {
        isHovered = false
        refresh()
    }

    private func refresh() {
        actions.isHidden = !(isHovered || row.isSelected)
        guard let theme else { return }
        // Metal paints terminal cell backgrounds in the canvas color, so the
        // surface must match it; states are carried by the border (§9).
        layer?.backgroundColor = theme.canvas.cgColor
        body.layer?.backgroundColor = NSColor.clear.cgColor
        if row.isSelected {
            layer?.borderColor = theme.blockFocus.cgColor
            layer?.borderWidth = Metrics.focusBorder
        } else {
            layer?.borderColor = (isHovered ? theme.blockSeamHover : theme.blockSeamRest).cgColor
            layer?.borderWidth = Metrics.border
        }
        command.textColor = theme.muted
        for case let button as NSButton in actions.arrangedSubviews {
            button.contentTintColor = button.isEnabled
                ? theme.secondary : theme.muted.withAlphaComponent(0.5)
        }
        applyStatus(theme)
    }

    private func applyStatus(_ theme: NativeTheme) {
        guard row.state != UInt16(SEYAL_APP_BLOCK_STATE_RUNNING) else {
            statusIcon.isHidden = true
            spinner.startAnimation(nil)
            return
        }
        spinner.stopAnimation(nil)
        statusIcon.isHidden = false
        let (symbol, tint): (String, NSColor)
        switch row.state {
        case UInt16(SEYAL_APP_BLOCK_STATE_COMPLETED): (symbol, tint) = ("checkmark.circle.fill", theme.blockSuccess)
        case UInt16(SEYAL_APP_BLOCK_STATE_FAILED): (symbol, tint) = ("xmark.circle.fill", theme.blockDanger)
        default: (symbol, tint) = ("questionmark.circle", theme.muted)
        }
        statusIcon.image = NSImage(
            systemSymbolName: symbol, accessibilityDescription: Self.statusName(row.state))?
            .withSymbolConfiguration(.init(pointSize: 13, weight: .semibold))
        statusIcon.contentTintColor = tint
    }

    @objc private func showCopyMenu(_ sender: NSButton) {
        let menu = NSMenu()
        menu.addItem(item("Copy command", .copyCommand, key: [.command]))
        menu.addItem(item("Copy output", .copyOutput, key: [.command, .shift]))
        menu.addItem(item("Copy command + output", .copyCommandAndOutput, key: [.command, .option]))
        menu.popUp(positioning: nil, at: NSPoint(x: 0, y: sender.bounds.maxY + 4), in: sender)
    }

    @objc private func showMoreMenu(_ sender: NSButton) {
        let menu = NSMenu()
        menu.autoenablesItems = false
        menu.addItem(item("Inspect", .inspect, symbol: "sidebar.right"))
        menu.addItem(.separator())
        menu.addItem(item("Copy command", .copyCommand))
        menu.addItem(item("Copy output", .copyOutput))
        let rerunItem = item("Rerun", .rerun, symbol: "play")
        rerunItem.isEnabled = row.canRerun
        menu.addItem(rerunItem)
        menu.popUp(positioning: nil, at: NSPoint(x: 0, y: sender.bounds.maxY + 4), in: sender)
    }

    @objc private func rerun() {
        onAction?(.rerun)
    }

    @objc private func performMenuAction(_ sender: NSMenuItem) {
        guard let box = sender.representedObject as? ActionBox else { return }
        onAction?(box.action)
    }

    private func item(
        _ title: String,
        _ action: CommandBlockAction,
        symbol: String = "doc.on.doc",
        key: NSEvent.ModifierFlags? = nil
    ) -> NSMenuItem {
        let item = NSMenuItem(
            title: title, action: #selector(performMenuAction(_:)), keyEquivalent: key == nil ? "" : "c")
        if let key { item.keyEquivalentModifierMask = key }
        item.target = self
        item.image = NSImage(systemSymbolName: symbol, accessibilityDescription: nil)
        item.representedObject = ActionBox(action)
        return item
    }

    private static func statusName(_ state: UInt16) -> String {
        switch state {
        case UInt16(SEYAL_APP_BLOCK_STATE_RUNNING): return "Running"
        case UInt16(SEYAL_APP_BLOCK_STATE_COMPLETED): return "Succeeded"
        case UInt16(SEYAL_APP_BLOCK_STATE_FAILED): return "Failed"
        default: return "Status unknown"
        }
    }

    private static func actionButton(symbol: String, label: String, id: String) -> NSButton {
        let image = NSImage(systemSymbolName: symbol, accessibilityDescription: label)?
            .withSymbolConfiguration(.init(pointSize: 12, weight: .regular))
        let button = NSButton(image: image ?? NSImage(), target: nil, action: nil)
        button.isBordered = false
        button.imagePosition = .imageOnly
        button.toolTip = label
        button.setAccessibilityLabel(label)
        button.setAccessibilityIdentifier("seyal-block-action-\(id)")
        button.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            button.widthAnchor.constraint(equalToConstant: 24),
            button.heightAnchor.constraint(equalToConstant: 22),
        ])
        return button
    }
}

private final class ActionBox {
    let action: CommandBlockAction
    init(_ action: CommandBlockAction) { self.action = action }
}
