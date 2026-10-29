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
        tabs: [PaneLayoutView.TerminalTab],
        focused: Bool,
        canSplit: Bool,
        canClosePane: Bool
    ) {
        self.tabs = tabs
        self.canClosePane = canClosePane
        super.init(frame: .zero)
        translatesAutoresizingMaskIntoConstraints = false
        setAccessibilityElement(true)
        setAccessibilityRole(.group)
        setAccessibilityIdentifier("seyal-pane-\(paneHi)-\(paneLo)-terminal-tabs")
        setAccessibilityValue(focused ? "focused" : "")
        tabStack.orientation = .horizontal
        tabStack.alignment = .centerY
        tabStack.spacing = 4
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
        actionStack.spacing = 4
        actionStack.translatesAutoresizingMaskIntoConstraints = true
        let actionCount = 1 + (canSplit ? 2 : 0)
        actionWidth = CGFloat(actionCount) * 24 + CGFloat(actionCount - 1) * 4
        addSubview(tabViewport)
        addSubview(actionStack)
        actionStack.frame = CGRect(x: 0, y: 0, width: actionWidth, height: 24)
        setAccessibilityChildren([tabViewport, actionStack])
        for (index, tab) in tabs.enumerated() {
            let group = NSStackView()
            group.orientation = .horizontal
            group.spacing = 0
            let button = NSButton(title: "Terminal \(index + 1)", target: self, action: #selector(selectTab(_:)))
            button.bezelStyle = .recessed
            button.identifier = NSUserInterfaceItemIdentifier("seyal-terminal-tab-\(tab.hi)-\(tab.lo)")
            button.setAccessibilityIdentifier(button.identifier!.rawValue)
            button.setAccessibilityValue(tab.selected ? "selected" : "")
            button.state = tab.selected ? .on : .off
            button.tag = index
            if tab.selected {
                selectedTabButton = button
                selectedTabIndex = index
            }
            group.addArrangedSubview(button)
            let close = NSButton(title: "×", target: self, action: #selector(closeTab(_:)))
            close.bezelStyle = .texturedRounded
            close.setAccessibilityIdentifier("seyal-terminal-tab-\(tab.hi)-\(tab.lo)-close")
            close.setAccessibilityLabel("Close Terminal \(index + 1)")
            close.tag = index
            group.addArrangedSubview(close)
            tabStack.addArrangedSubview(group)
            terminalGroups.append(group)
        }
        let add = NSButton(title: "+", target: self, action: #selector(newTerminal))
        add.bezelStyle = .texturedRounded
        add.isBordered = false
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
        button.bezelStyle = .texturedRounded
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

