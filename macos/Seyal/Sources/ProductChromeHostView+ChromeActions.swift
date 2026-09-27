import AppKit

@MainActor
extension ProductChromeHostView {
    @objc func showWorkspaces() {
        applyChromeKind(UInt16(SEYAL_APP_ACTION_SET_LEFT_PANEL.rawValue), reserved: 0)
    }

    @objc func showTabs() {
        applyChromeKind(UInt16(SEYAL_APP_ACTION_SET_LEFT_PANEL.rawValue), reserved: 1)
    }

    @objc func selectWorkspace(_ sender: NSButton) {
        applyIdentity(UInt16(SEYAL_APP_ACTION_SELECT_WORKSPACE.rawValue), button: sender)
    }

    @objc func selectTab(_ sender: NSButton) {
        applyIdentity(UInt16(SEYAL_APP_ACTION_SELECT_TAB.rawValue), button: sender)
    }

    @objc func focusPane(_ sender: NSButton) {
        applyIdentity(UInt16(SEYAL_APP_ACTION_FOCUS_PANE.rawValue), button: sender)
    }

    @objc func createTab() {
        applyChromeKind(UInt16(SEYAL_APP_ACTION_CREATE_TAB.rawValue), reserved: 0)
    }

    @objc func closeActiveTab(_ sender: NSButton) {
        applyIdentity(UInt16(SEYAL_APP_ACTION_CLOSE_TAB.rawValue), button: sender)
    }

    @objc func splitFocusedRight() {
        applyChromeKind(UInt16(SEYAL_APP_ACTION_SPLIT_FOCUSED.rawValue), reserved: 0)
    }

    @objc func splitFocusedDown() {
        applyChromeKind(UInt16(SEYAL_APP_ACTION_SPLIT_FOCUSED.rawValue), reserved: 1)
    }

    @objc func closeFocusedPane(_ sender: NSButton) {
        applyIdentity(UInt16(SEYAL_APP_ACTION_CLOSE_PANE.rawValue), button: sender)
    }

    @objc func openAttention(_ sender: NSButton) {
        applyPayload(UInt16(SEYAL_APP_ACTION_OPEN_ATTENTION.rawValue), text: sender.identifier?.rawValue ?? "")
    }

    @objc func selectAgent(_ sender: NSButton) {
        applyPayload(UInt16(SEYAL_APP_ACTION_SELECT_AGENT.rawValue), text: sender.identifier?.rawValue ?? "")
    }

    func applyChromeKind(_ kind: UInt16, reserved: UInt32) {
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = kind
        action.reserved = reserved
        _ = seyal_app_apply(pane.appHandle, &action)
        reconcileChrome()
    }

    func applyIdentity(_ kind: UInt16, button: NSButton) {
        guard let tagged = button as? IdentityButton else { return }
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = kind
        action.target_execution_lo = tagged.idLo
        action.target_execution_hi = tagged.idHi
        _ = seyal_app_apply(pane.appHandle, &action)
        reconcileChrome()
    }

    /// Block selection is Rust-owned (#935): the click only names the Block
    /// identity; Rust validates it against the focused Pane's Block list.
    func selectBlock(idLo: UInt64, idHi: UInt64, deselect: Bool) {
        let snapshot = seyal_app_snapshot(pane.appHandle)
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(
            deselect
                ? SEYAL_APP_ACTION_CLEAR_BLOCK_SELECTION.rawValue
                : SEYAL_APP_ACTION_SELECT_BLOCK.rawValue
        )
        action.applySnapshotFence(snapshot)
        action.target_execution_lo = idLo
        action.target_execution_hi = idHi
        guard seyal_app_apply(pane.appHandle, &action) == 0 else { return }
        // A successful apply bumps the snapshot generation; reconcile rebuilds
        // cards (selected flag), inspector rows and inspector visibility.
        reconcileChrome()
    }

    func applyPayload(_ kind: UInt16, text: String) {
        let snapshot = seyal_app_snapshot(pane.appHandle)
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = kind
        action.applySnapshotFence(snapshot)
        let utf8 = Array(text.utf8)
        utf8.withUnsafeBufferPointer { buffer in
            action.payload = buffer.baseAddress
            action.payload_len = UInt32(buffer.count)
            _ = seyal_app_apply(pane.appHandle, &action)
        }
        reconcileChrome()
    }

    func configureChromeButtons() {
        styleSwitcher(workspacesButton, identifier: "seyal-left-workspaces", action: #selector(showWorkspaces))
        styleSwitcher(tabsButton, identifier: "seyal-left-tabs", action: #selector(showTabs))
        newTabButton.setAccessibilityIdentifier("seyal-new-tab")
        newTabButton.target = self
        newTabButton.action = #selector(createTab)
        closeTabButton.setAccessibilityIdentifier("seyal-close-tab")
        closeTabButton.target = self
        closeTabButton.action = #selector(closeActiveTab(_:))
        splitRightButton.setAccessibilityIdentifier("seyal-split-right")
        splitRightButton.target = self
        splitRightButton.action = #selector(splitFocusedRight)
        splitDownButton.setAccessibilityIdentifier("seyal-split-down")
        splitDownButton.target = self
        splitDownButton.action = #selector(splitFocusedDown)
        closePaneButton.setAccessibilityIdentifier("seyal-close-pane")
        closePaneButton.target = self
        closePaneButton.action = #selector(closeFocusedPane(_:))
    }

    func styleSwitcher(_ button: NSButton, identifier: String, action: Selector) {
        button.setButtonType(.toggle)
        button.bezelStyle = .inline
        button.isBordered = false
        button.font = .systemFont(ofSize: 11, weight: .semibold)
        button.setAccessibilityIdentifier(identifier)
        button.target = self
        button.action = action
    }

    func borderlessButton(title: String, action: Selector) -> NSButton {
        let button = NSButton(title: title, target: self, action: action)
        button.bezelStyle = .inline
        button.isBordered = false
        button.font = .systemFont(ofSize: 12, weight: .regular)
        button.alignment = .left
        return button
    }

    func rowButton(
        title: String,
        detail: String?,
        identifier: String,
        selected: Bool,
        action: Selector,
        kind: UInt16,
        idLo: UInt64,
        idHi: UInt64
    ) -> IdentityButton {
        let label = detail.flatMap { $0.isEmpty ? nil : $0 }.map { "\(title)  \($0)" } ?? title
        let button = IdentityButton(title: label, target: self, action: action)
        button.idLo = idLo
        button.idHi = idHi
        button.kind = kind
        button.bezelStyle = .inline
        button.isBordered = false
        button.font = .systemFont(ofSize: 12, weight: selected ? .semibold : .regular)
        button.alignment = .left
        button.setAccessibilityIdentifier(identifier)
        button.state = selected ? .on : .off
        return button
    }

    func expose(_ view: NSView, identifier: String) {
        view.setAccessibilityElement(true)
        view.setAccessibilityRole(.group)
        view.setAccessibilityIdentifier(identifier)
    }
}
