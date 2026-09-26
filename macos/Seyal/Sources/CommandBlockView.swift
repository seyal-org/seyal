import AppKit

/// One Rust-projected quick action (`seyal_app_block_action_row`, #1010).
/// Rust owns the set, order, placement, label, shortcut hint and
/// availability (ADR-015); the view only draws it and reports `kind` back.
struct CommandBlockActionRow: Equatable {
    let kind: UInt16
    let placement: UInt16
    let label: String
    /// Portable hint such as `shift+cmd+c`; empty when none.
    let shortcut: String
    let enabled: Bool
}

/// Snapshot of one Rust Block row the view renders.
struct CommandBlockRow {
    let command: String
    let state: UInt16
    /// Rust status name for the icon/spinner ("Succeeded", "Running", …).
    let statusLabel: String
    let isSelected: Bool
    let actions: [CommandBlockActionRow]
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
    /// A chosen action's Rust kind (`SEYAL_APP_BLOCK_ACTION_*`).
    var onAction: ((UInt16) -> Void)?

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
        statusIcon.setAccessibilityLabel(row.statusLabel)
        statusIcon.translatesAutoresizingMaskIntoConstraints = false
        spinner.style = .spinning
        spinner.controlSize = .small
        spinner.isDisplayedWhenStopped = false
        spinner.translatesAutoresizingMaskIntoConstraints = false
        // While running the spinner replaces the icon, so it carries the same
        // status identity for VoiceOver (a hidden icon leaves the AX tree).
        spinner.setAccessibilityElement(true)
        spinner.setAccessibilityIdentifier("seyal-block-status")
        spinner.setAccessibilityLabel(row.statusLabel)

        actions.orientation = .horizontal
        actions.spacing = 2
        actions.translatesAutoresizingMaskIntoConstraints = false
        for action in row.actions where action.placement == UInt16(SEYAL_APP_BLOCK_ACTION_SEAM) {
            actions.addArrangedSubview(seamButton(action))
        }

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
        window?.makeFirstResponder(self)
    }

    override var acceptsFirstResponder: Bool { true }

    override func keyDown(with event: NSEvent) {
        // Space/Return select or toggle via Rust SelectBlock; no ⌘↑/⌘↓
        // (Block-to-Block navigation is deferred to a SPEC-024 amendment).
        if event.charactersIgnoringModifiers == " " || event.keyCode == 36 {
            onSelect?(row.isSelected)
            return
        }
        super.keyDown(with: event)
    }

    override func accessibilityPerformPress() -> Bool {
        onSelect?(row.isSelected)
        return true
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
            // Design §8: spinner stops under Reduce Motion (Rust-projected).
            if theme.allowsMotion {
                spinner.isDisplayedWhenStopped = false
                spinner.startAnimation(nil)
            } else {
                spinner.stopAnimation(nil)
                spinner.isDisplayedWhenStopped = true
            }
            return
        }
        spinner.stopAnimation(nil)
        spinner.isDisplayedWhenStopped = false
        statusIcon.isHidden = false
        let (symbol, tint): (String, NSColor)
        switch row.state {
        case UInt16(SEYAL_APP_BLOCK_STATE_COMPLETED): (symbol, tint) = ("checkmark.circle.fill", theme.blockSuccess)
        case UInt16(SEYAL_APP_BLOCK_STATE_FAILED): (symbol, tint) = ("xmark.circle.fill", theme.blockDanger)
        default: (symbol, tint) = ("questionmark.circle", theme.muted)
        }
        statusIcon.image = NSImage(
            systemSymbolName: symbol, accessibilityDescription: row.statusLabel)?
            .withSymbolConfiguration(.init(pointSize: 13, weight: .semibold))
        statusIcon.contentTintColor = tint
    }

    /// Seam buttons either open a Rust-listed menu or report their kind.
    @objc private func seamButtonClicked(_ sender: NSButton) {
        let kind = UInt16(sender.tag)
        let placement: UInt32
        switch UInt32(kind) {
        case SEYAL_APP_BLOCK_ACTION_COPY_MENU: placement = SEYAL_APP_BLOCK_ACTION_IN_COPY_MENU
        case SEYAL_APP_BLOCK_ACTION_MORE_MENU: placement = SEYAL_APP_BLOCK_ACTION_IN_MORE_MENU
        default:
            onAction?(kind)
            return
        }
        let menu = NSMenu()
        menu.autoenablesItems = false
        for action in row.actions where action.placement == UInt16(placement) {
            menu.addItem(menuItem(action))
        }
        menu.popUp(positioning: nil, at: NSPoint(x: 0, y: sender.bounds.maxY + 4), in: sender)
    }

    @objc private func menuItemChosen(_ sender: NSMenuItem) {
        onAction?(UInt16(sender.tag))
    }

    private func menuItem(_ action: CommandBlockActionRow) -> NSMenuItem {
        let (key, modifiers) = Self.keyEquivalent(action.shortcut)
        let item = NSMenuItem(
            title: action.label, action: #selector(menuItemChosen(_:)), keyEquivalent: key)
        item.keyEquivalentModifierMask = modifiers
        item.target = self
        item.tag = Int(action.kind)
        item.isEnabled = action.enabled
        item.image = NSImage(systemSymbolName: Self.symbol(action.kind), accessibilityDescription: nil)
        return item
    }

    private func seamButton(_ action: CommandBlockActionRow) -> NSButton {
        let image = NSImage(
            systemSymbolName: Self.symbol(action.kind), accessibilityDescription: action.label)?
            .withSymbolConfiguration(.init(pointSize: 12, weight: .regular))
        let button = NSButton(image: image ?? NSImage(), target: self, action: #selector(seamButtonClicked(_:)))
        button.tag = Int(action.kind)
        button.isEnabled = action.enabled
        button.isBordered = false
        button.imagePosition = .imageOnly
        button.toolTip = action.label
        button.setAccessibilityLabel(action.label)
        button.setAccessibilityIdentifier("seyal-block-action-\(Self.identifier(action.kind))")
        // Selected Block actions stay in the key-view loop (design §6).
        button.refusesFirstResponder = false
        button.focusRingType = .exterior
        button.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            button.widthAnchor.constraint(equalToConstant: 24),
            button.heightAnchor.constraint(equalToConstant: 22),
        ])
        return button
    }

    /// Platform presentation only: SF Symbol per Rust action kind.
    private static func symbol(_ kind: UInt16) -> String {
        switch UInt32(kind) {
        case SEYAL_APP_BLOCK_ACTION_RERUN: return "play"
        case SEYAL_APP_BLOCK_ACTION_MORE_MENU: return "ellipsis"
        case SEYAL_APP_BLOCK_ACTION_INSPECT: return "sidebar.right"
        default: return "doc.on.doc"
        }
    }

    private static func identifier(_ kind: UInt16) -> String {
        switch UInt32(kind) {
        case SEYAL_APP_BLOCK_ACTION_COPY_MENU: return "copy"
        case SEYAL_APP_BLOCK_ACTION_RERUN: return "rerun"
        case SEYAL_APP_BLOCK_ACTION_MORE_MENU: return "more"
        default: return "\(kind)"
        }
    }

    /// Maps Rust's portable shortcut hint (`shift+cmd+c`) onto AppKit.
    static func keyEquivalent(_ shortcut: String) -> (String, NSEvent.ModifierFlags) {
        let parts = shortcut.split(separator: "+").map(String.init)
        guard let key = parts.last, !key.isEmpty else { return ("", []) }
        var modifiers: NSEvent.ModifierFlags = []
        for part in parts.dropLast() {
            switch part {
            case "cmd": modifiers.insert(.command)
            case "shift": modifiers.insert(.shift)
            case "alt": modifiers.insert(.option)
            case "ctrl": modifiers.insert(.control)
            default: break
            }
        }
        return (key, modifiers)
    }
}
