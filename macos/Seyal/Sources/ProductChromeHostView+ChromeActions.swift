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

    /// Pane-region click (#923): Rust validates the PaneId and owns focus.
    func focusPaneRegion(lo: UInt64, hi: UInt64) {
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_FOCUS_PANE.rawValue)
        action.target_execution_lo = lo
        action.target_execution_hi = hi
        _ = seyal_app_apply(pane.appHandle, &action)
        reconcileChrome()
        routeFocus()
    }

    @objc func createTab() {
        applyChromeKind(UInt16(SEYAL_APP_ACTION_CREATE_TAB.rawValue), reserved: 0)
    }

    /// SPEC-024 §11 / R6.4.1: menu or key-equivalent dispatch of a projected
    /// WorkspaceCommand. Rust re-validates against the current route context.
    @objc func invokeProjectedWorkspaceCommand(_ sender: NSMenuItem) {
        guard let commandId = sender.representedObject as? UInt16 else { return }
        let ordinal = commandId == KeybindingShortcutRealization.windowSelectOrdinal
            ? UInt8(sender.tag)
            : 0
        let code = seyal_app_invoke_workspace_command(pane.appHandle, commandId, ordinal, 0)
        if code == 0 {
            // This callback also lets the owning window host realize native
            // effects (including a Rust-owned window close) produced by the
            // command. Calling only reconcileChrome leaves the AppKit window
            // map stale until some later frame happens to arrive.
            pane.onProductChanged?()
            routeFocus()
        } else {
            NSApp.presentError(
                NSError(
                    domain: "Seyal.WorkspaceCommand",
                    code: Int(code),
                    userInfo: [
                        NSLocalizedDescriptionKey: "Seyal could not perform “\(sender.title)”. Try again."
                    ]
                )
            )
        }
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
        if kind == UInt16(SEYAL_APP_ACTION_CLOSE_TAB.rawValue)
            || kind == UInt16(SEYAL_APP_ACTION_CLOSE_PANE.rawValue)
        {
            action.target_generation = tagged.containmentGeneration
        }
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
        // C05: compact layout chrome — short glyphs, full names stay in accessibility labels.
        styleCompactChrome(newTabButton, title: "+", label: "New Tab", id: "seyal-new-tab", action: #selector(createTab))
        styleCompactChrome(closeTabButton, title: "×", label: "Close Tab", id: "seyal-close-tab", action: #selector(closeActiveTab(_:)))
        styleCompactChrome(splitRightButton, title: "❘", label: "Split Right", id: "seyal-split-right", action: #selector(splitFocusedRight))
        styleCompactChrome(splitDownButton, title: "⎯", label: "Split Down", id: "seyal-split-down", action: #selector(splitFocusedDown))
        styleCompactChrome(closePaneButton, title: "✕", label: "Close Pane", id: "seyal-close-pane", action: #selector(closeFocusedPane(_:)))
    }

    func styleCompactChrome(_ button: NSButton, title: String, label: String, id: String, action: Selector) {
        button.title = title
        button.toolTip = label
        button.setAccessibilityLabel(label)
        button.setAccessibilityIdentifier(id)
        button.target = self
        button.action = action
        button.bezelStyle = .inline
        button.isBordered = false
        button.font = .systemFont(ofSize: 13, weight: .medium)
    }

    func configureCloseButtons(shell: SeyalAppShell) {
        closeTabButton.idLo = shell.active_tab_lo
        closeTabButton.idHi = shell.active_tab_hi
        closeTabButton.containmentGeneration = shell.containment_generation
        closePaneButton.idLo = shell.focused_pane_lo
        closePaneButton.idHi = shell.focused_pane_hi
        closePaneButton.containmentGeneration = shell.containment_generation
    }

    /// PT5: name ids only. Rust owns zoom/layout; stale ids fail closed.
    func zoomPane(lo: UInt64, hi: UInt64) {
        applyPaneTreeKind(UInt16(SEYAL_APP_ACTION_ZOOM_PANE.rawValue), lo: lo, hi: hi)
    }

    func unzoomPane() {
        applyChromeKind(UInt16(SEYAL_APP_ACTION_UNZOOM.rawValue), reserved: 0)
    }

    func swapPanes(aLo: UInt64, aHi: UInt64, bLo: UInt64, bHi: UInt64) {
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_SWAP_PANES.rawValue)
        action.target_execution_lo = aLo
        action.target_execution_hi = aHi
        action.target_attachment_lo = bLo
        action.target_attachment_hi = bHi
        _ = seyal_app_apply(pane.appHandle, &action)
        reconcileChrome()
    }

    func movePaneBeside(paneLo: UInt64, paneHi: UInt64, neighborLo: UInt64, neighborHi: UInt64, side: UInt32) {
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_MOVE_PANE_BESIDE.rawValue)
        action.target_execution_lo = paneLo
        action.target_execution_hi = paneHi
        action.target_attachment_lo = neighborLo
        action.target_attachment_hi = neighborHi
        action.reserved = side
        _ = seyal_app_apply(pane.appHandle, &action)
        reconcileChrome()
    }

    func focusDirection(_ reserved: UInt32) {
        applyChromeKind(UInt16(SEYAL_APP_ACTION_FOCUS_DIRECTION.rawValue), reserved: reserved)
    }

    func equalizeFocused() {
        applyChromeKind(UInt16(SEYAL_APP_ACTION_EQUALIZE_FOCUSED.rawValue), reserved: 0)
    }

    func equalizeTab() {
        applyChromeKind(UInt16(SEYAL_APP_ACTION_EQUALIZE_TAB.rawValue), reserved: 0)
    }

    private func applyPaneTreeKind(_ kind: UInt16, lo: UInt64, hi: UInt64) {
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = kind
        action.target_execution_lo = lo
        action.target_execution_hi = hi
        _ = seyal_app_apply(pane.appHandle, &action)
        reconcileChrome()
    }

    func styleSwitcher(_ button: NSButton, identifier: String, action: Selector) {
        // Typography-only mode switcher (C03); no AppKit toggle chrome.
        button.setButtonType(.momentaryChange)
        button.bezelStyle = .inline
        button.isBordered = false
        button.font = .systemFont(ofSize: 11, weight: .semibold)
        button.setAccessibilityIdentifier(identifier)
        button.target = self
        button.action = action
    }

    func applySwitcherAppearance(leftPanel: UInt16, theme: NativeTheme) {
        let workspacesOn = leftPanel == 0
        workspacesButton.font = .systemFont(ofSize: theme.uiFontSize - 1, weight: workspacesOn ? .semibold : .regular)
        tabsButton.font = .systemFont(ofSize: theme.uiFontSize - 1, weight: workspacesOn ? .regular : .semibold)
        workspacesButton.contentTintColor = workspacesOn ? theme.text : theme.muted
        tabsButton.contentTintColor = workspacesOn ? theme.muted : theme.text
        workspacesButton.attributedTitle = NSAttributedString(
            string: "Workspaces",
            attributes: [
                .font: workspacesButton.font as Any,
                .foregroundColor: workspacesOn ? theme.text : theme.muted,
            ]
        )
        tabsButton.attributedTitle = NSAttributedString(
            string: "Tabs",
            attributes: [
                .font: tabsButton.font as Any,
                .foregroundColor: workspacesOn ? theme.muted : theme.text,
            ]
        )
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
        showsAccent: Bool = true,
        action: Selector,
        kind: UInt16,
        idLo: UInt64,
        idHi: UInt64
    ) -> ChromeContextRowView {
        let row = ChromeContextRowView(
            title: title,
            detail: detail,
            identifier: identifier,
            selected: selected,
            showsAccent: showsAccent,
            target: self,
            action: action,
            kind: kind,
            idLo: idLo,
            idHi: idHi
        )
        let theme = NativeThemeRealization.theme(for: effectiveAppearance)
        row.apply(theme: theme)
        return row
    }

    func expose(_ view: NSView, identifier: String) {
        view.setAccessibilityElement(true)
        view.setAccessibilityRole(.group)
        view.setAccessibilityIdentifier(identifier)
    }

    /// Host GPU follows Rust-authored tier; never invents Hidden/Visible.
    func applyPresentationTierFromSnapshot() {
        let snapshot = seyal_app_snapshot(pane.appHandle)
        let shell = seyal_app_shell(pane.appHandle)
        var matched = false
        for windowIndex in 0..<Int(shell.window_count) {
            let window = seyal_app_window(pane.appHandle, UInt32(windowIndex))
            for tabIndex in 0..<Int(window.tab_count) {
                let tab = seyal_app_tab(pane.appHandle, UInt32(windowIndex), UInt32(tabIndex))
                for paneIndex in 0..<Int(tab.pane_count) {
                    let leaf = seyal_app_pane_leaf(
                        pane.appHandle,
                        UInt32(windowIndex),
                        UInt32(tabIndex),
                        UInt32(paneIndex)
                    )
                    if leaf.pane_lo == snapshot.pane_lo, leaf.pane_hi == snapshot.pane_hi {
                        pane.inputSurface.applyProductPresentationTier(leaf.presentation_tier)
                        matched = true
                    }
                }
            }
        }
        if !matched {
            pane.inputSurface.applyProductPresentationTier(UInt16(SEYAL_APP_TIER_UNPRESENTED))
        }
    }
}

extension ProductChromeHostView: NSMenuItemValidation {
    /// R6.4.2: enabled state comes only from the Rust shortcut projection.
    func validateMenuItem(_ menuItem: NSMenuItem) -> Bool {
        guard let commandId = menuItem.representedObject as? UInt16 else {
            return true
        }
        return KeybindingShortcutRealization.isEnabled(
            appHandle: pane.appHandle,
            commandId: commandId,
            composerFocused: false
        )
    }
}
