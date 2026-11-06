import AppKit

@MainActor
final class PaneTerminalTabStrip: NSView {
    static let height: CGFloat = 30
    var onSelect: ((PaneLayoutView.TerminalTab) -> Void)?
    var onClose: ((PaneLayoutView.TerminalTab) -> Void)?
    var onNew: (() -> Void)?
    var onSplit: ((PaneLayoutView.SplitDirection) -> Void)?
    var onClosePane: (() -> Void)?
    private let tabs: [PaneLayoutView.TerminalTab]
    private let canClosePane: Bool
    private let focused: Bool
    private var tabButtons: [NSButton] = []
    private var tabUnderlines: [NSView] = []
    private var closeButtons: [NSButton] = []
    private let tabStack = NSStackView()
    private let tabViewport = NSScrollView()
    private let actionStack = NSStackView()
    private var actionWidth: CGFloat = 0
    private var selectedTabButton: NSButton?
    private var selectedTabIndex: Int?
    private var terminalGroups: [NSView] = []
    private var revealedSelectedTab = false

    init(
        paneLo: UInt64,
        paneHi: UInt64,
        paneTitle: String,
        tabs: [PaneLayoutView.TerminalTab],
        focused: Bool,
        canSplit: Bool,
        canClosePane: Bool
    ) {
        self.tabs = tabs
        self.canClosePane = canClosePane
        self.focused = focused
        super.init(frame: .zero)
        wantsLayer = true
        translatesAutoresizingMaskIntoConstraints = false
        setAccessibilityElement(true)
        setAccessibilityRole(.group)
        setAccessibilityIdentifier("seyal-pane-\(paneHi)-\(paneLo)-terminal-tabs")
        setAccessibilityValue(focused ? "focused" : "")
        tabStack.orientation = .horizontal
        tabStack.alignment = .centerY
        tabStack.spacing = 2
        tabStack.translatesAutoresizingMaskIntoConstraints = true
        tabViewport.drawsBackground = false
        tabViewport.borderType = .noBorder
        tabViewport.hasHorizontalScroller = false
        tabViewport.hasVerticalScroller = false
        tabViewport.horizontalScrollElasticity = .allowed
        tabViewport.setAccessibilityIdentifier("seyal-pane-\(paneHi)-\(paneLo)-terminal-tab-viewport")
        tabViewport.documentView = tabStack
        actionStack.orientation = .horizontal
        actionStack.alignment = .centerY
        actionStack.spacing = 2
        actionStack.translatesAutoresizingMaskIntoConstraints = true
        let actionCount = 1 + (canSplit ? 2 : 0)
        actionWidth = CGFloat(actionCount) * 24 + CGFloat(actionCount - 1) * 4
        addSubview(tabViewport)
        addSubview(actionStack)
        actionStack.frame = CGRect(x: 0, y: 0, width: actionWidth, height: 24)
        setAccessibilityChildren([tabViewport, actionStack])
        for (index, tab) in tabs.enumerated() {
            // C05: one quiet tab row. Selection is typography plus a 2 pt
            // semantic seam, not a recessed/pill AppKit button.
            let group = NSStackView()
            group.orientation = .vertical
            group.alignment = .width
            group.spacing = 0

            let content = NSStackView()
            content.orientation = .horizontal
            content.alignment = .centerY
            content.spacing = 2
            let title = Self.terminalTabTitle(
                paneTitle: paneTitle,
                index: index,
                tabCount: tabs.count
            )
            let button = NSButton(title: title, target: self, action: #selector(selectTab(_:)))
            button.bezelStyle = .inline
            button.isBordered = false
            button.font = .systemFont(ofSize: 11, weight: tab.selected ? .semibold : .regular)
            button.contentTintColor = tab.selected ? .labelColor : .secondaryLabelColor
            button.cell?.lineBreakMode = .byTruncatingMiddle
            button.identifier = NSUserInterfaceItemIdentifier("seyal-terminal-tab-\(tab.hi)-\(tab.lo)")
            button.setAccessibilityIdentifier(button.identifier!.rawValue)
            button.setAccessibilityLabel(title)
            button.setAccessibilityValue(tab.selected ? "selected" : "")
            button.state = tab.selected ? .on : .off
            button.tag = index
            button.heightAnchor.constraint(equalToConstant: 24).isActive = true
            button.widthAnchor.constraint(greaterThanOrEqualToConstant: 64).isActive = true
            button.widthAnchor.constraint(lessThanOrEqualToConstant: 150).isActive = true
            if tab.selected {
                selectedTabButton = button
                selectedTabIndex = index
            }
            content.addArrangedSubview(button)
            tabButtons.append(button)

            // Reserve close-button geometry on every tab; only the selected
            // tab exposes its action. Switching selection never shifts chips.
            let close = NSButton(title: "×", target: self, action: #selector(closeTab(_:)))
            close.bezelStyle = .inline
            close.isBordered = false
            close.font = .systemFont(ofSize: 12, weight: .regular)
            close.setAccessibilityIdentifier("seyal-terminal-tab-\(tab.hi)-\(tab.lo)-close")
            close.setAccessibilityLabel("Close \(title)")
            close.toolTip = "Close Terminal"
            close.tag = index
            close.alphaValue = tab.selected ? 1 : 0
            close.isEnabled = tab.selected
            close.widthAnchor.constraint(equalToConstant: 18).isActive = true
            close.heightAnchor.constraint(equalToConstant: 24).isActive = true
            content.addArrangedSubview(close)
            closeButtons.append(close)

            let underline = NSView()
            underline.wantsLayer = true
            underline.layer?.backgroundColor = tab.selected
                ? NSColor.controlAccentColor.cgColor : NSColor.clear.cgColor
            underline.heightAnchor.constraint(equalToConstant: 2).isActive = true
            underline.setAccessibilityElement(false)
            group.addArrangedSubview(content)
            group.addArrangedSubview(underline)
            tabUnderlines.append(underline)
            tabStack.addArrangedSubview(group)
            terminalGroups.append(group)
        }
        let add = NSButton(title: "+", target: self, action: #selector(newTerminal))
        add.bezelStyle = .inline
        add.isBordered = false
        add.image = NSImage(systemSymbolName: "plus", accessibilityDescription: "New Terminal")
        add.imagePosition = .imageOnly
        add.toolTip = "New Terminal"
        add.widthAnchor.constraint(equalToConstant: 24).isActive = true
        add.heightAnchor.constraint(equalToConstant: 24).isActive = true
        add.setAccessibilityIdentifier("seyal-pane-\(paneHi)-\(paneLo)-new-terminal")
        add.setAccessibilityLabel("New Terminal")
        actionStack.addArrangedSubview(add)
        tabStack.setFrameSize(tabStack.fittingSize)

        let splitRight = Self.iconButton(
            symbol: "rectangle.split.2x1",
            label: "Split Pane Right",
            identifier: "seyal-split-right",
            target: self,
            action: #selector(splitRightPane)
        )
        splitRight.isHidden = !canSplit
        actionStack.addArrangedSubview(splitRight)
        let splitDown = Self.iconButton(
            symbol: "rectangle.split.1x2",
            label: "Split Pane Down",
            identifier: "seyal-split-down",
            target: self,
            action: #selector(splitDownPane)
        )
        splitDown.isHidden = !canSplit
        actionStack.addArrangedSubview(splitDown)
    }

    /// Only presentation values are realized here; terminal selection,
    /// identity, focus and close mutations remain Rust-owned.
    func apply(theme: NativeTheme) {
        layer?.backgroundColor = theme.utility.withAlphaComponent(focused ? 0.72 : 0.42).cgColor
        for (index, button) in tabButtons.enumerated() {
            let selected = tabs[index].selected
            button.attributedTitle = NSAttributedString(
                string: button.title,
                attributes: [
                    .font: NSFont.systemFont(ofSize: max(11, theme.uiFontSize - 1),
                                             weight: selected ? .semibold : .regular),
                    .foregroundColor: selected ? theme.text : theme.secondary,
                ]
            )
            tabUnderlines[index].layer?.backgroundColor = selected
                ? (focused ? theme.accent : theme.seam).cgColor
                : NSColor.clear.cgColor
            closeButtons[index].contentTintColor = theme.secondary
        }
    }

    override func menu(for event: NSEvent) -> NSMenu? {
        guard canClosePane else { return nil }
        let menu = NSMenu()
        let closePane = NSMenuItem(title: "Close Pane", action: #selector(closePaneRegion), keyEquivalent: "")
        closePane.target = self
        closePane.identifier = NSUserInterfaceItemIdentifier("seyal-close-pane")
        menu.addItem(closePane)
        return menu
    }

    override func layout() {
        super.layout()
        let toolbarHeight = min(24, max(0, bounds.height))
        let toolbarX = max(6, bounds.width - 6 - actionWidth)
        actionStack.frame = CGRect(
            x: toolbarX,
            y: (bounds.height - toolbarHeight) / 2,
            width: actionWidth,
            height: toolbarHeight
        )
        let viewportHeight = min(26, max(0, bounds.height - 4))
        let viewportX: CGFloat = 6
        let viewportWidth = max(0, toolbarX - 8 - viewportX)
        tabViewport.frame = CGRect(
            x: viewportX,
            y: (bounds.height - viewportHeight) / 2,
            width: viewportWidth,
            height: viewportHeight
        )
        guard
            !revealedSelectedTab,
            viewportWidth > 0,
            selectedTabButton != nil,
            let selectedTabIndex,
            terminalGroups.indices.contains(selectedTabIndex)
        else { return }
        tabStack.layoutSubtreeIfNeeded()
        var firstVisibleIndex = selectedTabIndex
        var visibleWidth = terminalGroups[firstVisibleIndex].frame.width
        while firstVisibleIndex > 0 {
            let previousWidth = terminalGroups[firstVisibleIndex - 1].frame.width + tabStack.spacing
            guard visibleWidth + previousWidth <= viewportWidth else { break }
            visibleWidth += previousWidth
            firstVisibleIndex -= 1
        }
        let origin = NSPoint(
            x: terminalGroups[firstVisibleIndex].frame.minX,
            y: tabViewport.contentView.bounds.origin.y
        )
        tabViewport.contentView.scroll(to: origin)
        tabViewport.reflectScrolledClipView(tabViewport.contentView)
        revealedSelectedTab = true
    }

    /// Distinct per-pane terminal-tab label. Single-tab leaves use the pane
    /// title alone; additional terminals in the same pane keep an index suffix.
    static func terminalTabTitle(paneTitle: String, index: Int, tabCount: Int) -> String {
        let trimmed = paneTitle.trimmingCharacters(in: .whitespacesAndNewlines)
        let base = trimmed.isEmpty ? "Terminal \(index + 1)" : trimmed
        if tabCount <= 1 { return base }
        return "\(base) · \(index + 1)"
    }

    private static func iconButton(
        symbol: String,
        label: String,
        identifier: String,
        target: AnyObject,
        action: Selector
    ) -> NSButton {
        let image = NSImage(systemSymbolName: symbol, accessibilityDescription: label)
            ?? NSImage(size: NSSize(width: 16, height: 16))
        let button = NSButton(image: image, target: target, action: action)
        button.bezelStyle = .inline
        button.isBordered = false
        button.imagePosition = .imageOnly
        button.setAccessibilityIdentifier(identifier)
        button.setAccessibilityLabel(label)
        button.toolTip = label
        button.translatesAutoresizingMaskIntoConstraints = false
        button.widthAnchor.constraint(equalToConstant: 24).isActive = true
        button.heightAnchor.constraint(equalToConstant: 24).isActive = true
        return button
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("PaneTerminalTabStrip is programmatic") }

    @objc private func selectTab(_ sender: NSButton) {
        guard tabs.indices.contains(sender.tag) else { return }
        onSelect?(tabs[sender.tag])
    }

    @objc private func closeTab(_ sender: NSButton) {
        guard tabs.indices.contains(sender.tag) else { return }
        onClose?(tabs[sender.tag])
    }

    @objc private func newTerminal() { onNew?() }

    @objc private func splitRightPane() { onSplit?(.right) }

    @objc private func splitDownPane() { onSplit?(.down) }

    @objc private func closePaneRegion() { onClosePane?() }
}

