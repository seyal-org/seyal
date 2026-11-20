import AppKit

/// ADR-028 / ADR-029 workspace chrome: leave offer, CreateWorkspace, Folder
/// picker, and SetWorkspaceRoot. Kept beside ChromeActions so workspace root
/// admission stays one AppKit responsibility (#1490).
@MainActor
extension ProductChromeHostView {
    /// Thin host confirm for ADR-028 leave offer. Rust owns create/jump; No is dismiss-only.
    func presentLeaveOfferIfNeeded() {
        let offer = seyal_app_leave_offer(pane.appHandle)
        guard offer.kind != UInt16(SEYAL_APP_LEAVE_OFFER_NONE.rawValue) else {
            presentedLeaveOfferPath = nil
            return
        }
        let path = productChromeCopyUTF8(offer.path, UInt32(offer.path_len)) ?? ""
        guard !path.isEmpty, presentedLeaveOfferPath != path else { return }
        presentedLeaveOfferPath = path
        let isJump = offer.kind == UInt16(SEYAL_APP_LEAVE_OFFER_JUMP.rawValue)
        let alert = NSAlert()
        alert.messageText = isJump ? "Open existing workspace?" : "Create workspace?"
        alert.informativeText = isJump
            ? "Current directory left this workspace root.\nJump to the workspace for:\n\(path)"
            : "Current directory left this workspace root.\nCreate a workspace for:\n\(path)"
        alert.addButton(withTitle: "Yes")
        alert.addButton(withTitle: "No")
        let response = alert.runModal()
        if response == .alertFirstButtonReturn {
            applyChromeKind(UInt16(SEYAL_APP_ACTION_CONFIRM_LEAVE_OFFER.rawValue), reserved: 0)
        } else {
            applyChromeKind(UInt16(SEYAL_APP_ACTION_DISMISS_LEAVE_OFFER.rawValue), reserved: 0)
        }
        presentedLeaveOfferPath = nil
        reconcileChrome()
    }

    /// CreateWorkspace: `+` inherits selected root (or HOME); Folder picks a directory.
    @objc func createWorkspace() {
        commitCreateWorkspace(root: inheritedWorkspaceRootPath())
    }

    @objc func createWorkspaceFromFolder() {
        pickFolderAndCreateWorkspace()
    }

    func pickFolderAndCreateWorkspace() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        panel.prompt = "Create Workspace"
        panel.message = "Choose a folder to bind as the new workspace root."
        panel.directoryURL = URL(fileURLWithPath: inheritedWorkspaceRootPath(), isDirectory: true)
        guard panel.runModal() == .OK, let url = panel.url else { return }
        let root = url.standardizedFileURL.path
        commitCreateWorkspace(root: root)
    }

    func commitCreateWorkspace(root: String) {
        let rootPath = (root as NSString).standardizingPath
        let code = applyPayloadReturning(
            UInt16(SEYAL_APP_ACTION_CREATE_WORKSPACE.rawValue),
            text: rootPath
        )
        if code != 0 {
            let error = seyal_app_last_error(pane.appHandle)
            let alert = NSAlert()
            alert.messageText = "Could not create workspace"
            alert.informativeText =
                "CreateWorkspace failed (apply=\(code), last_error=\(error)). "
                + "13=NoLiveClient, 35=ProvisioningRejected. Quit Seyal fully (stop any "
                + "leftover seyal-runtime), relaunch so the bundled Runtime attaches, "
                + "then retry +. A live terminal alone is not enough if mint cannot "
                + "reach Runtime CAP_EXECUTION_PROVISIONING."
            alert.alertStyle = .warning
            alert.runModal()
            routeFocus()
            return
        }
        ensureSelectedWorkspaceRoot(rootPath)
        routeFocus()
    }

    /// After create, force the selected workspace Path to the picked folder.
    func ensureSelectedWorkspaceRoot(_ rootPath: String) {
        guard let selected = selectedWorkspaceRow() else { return }
        let shown = productChromeCopyUTF8(selected.detail, selected.detail_len) ?? ""
        let shownPath = (shown as NSString).standardizingPath
        let wanted = (rootPath as NSString).standardizingPath
        if !shownPath.isEmpty, shownPath == wanted { return }
        _ = applyWorkspacePayloadReturning(
            UInt16(SEYAL_APP_ACTION_SET_WORKSPACE_ROOT.rawValue),
            text: wanted,
            targetLo: selected.id_lo,
            targetHi: selected.id_hi
        )
    }

    /// ADR-028 SetWorkspaceRoot on the selected open-workspace row.
    @discardableResult
    func commitSetWorkspaceRootOnSelected(root: String) -> Int32 {
        guard let selected = selectedWorkspaceRow() else {
            let alert = NSAlert()
            alert.messageText = "Could not set workspace folder"
            alert.informativeText = "No workspace is selected."
            alert.alertStyle = .warning
            alert.runModal()
            routeFocus()
            return -1
        }
        let code = applyWorkspacePayloadReturning(
            UInt16(SEYAL_APP_ACTION_SET_WORKSPACE_ROOT.rawValue),
            text: root,
            targetLo: selected.id_lo,
            targetHi: selected.id_hi
        )
        if code != 0 {
            let error = seyal_app_last_error(pane.appHandle)
            let alert = NSAlert()
            alert.messageText = "Could not set workspace folder"
            alert.informativeText =
                "SetWorkspaceRoot failed (apply=\(code), last_error=\(error))."
            alert.alertStyle = .warning
            alert.runModal()
        }
        routeFocus()
        return code
    }

    /// Prefer the selected open-workspace detail (bound root); else HOME.
    func inheritedWorkspaceRootPath() -> String {
        if let selected = selectedWorkspaceRow(),
           let detail = productChromeCopyUTF8(selected.detail, selected.detail_len)
        {
            let trimmed = detail.trimmingCharacters(in: .whitespacesAndNewlines)
            if !trimmed.isEmpty, trimmed.hasPrefix("/") {
                return trimmed
            }
        }
        return NSHomeDirectory()
    }

    func selectedWorkspaceRow() -> SeyalAppRow? {
        let shell = seyal_app_shell(pane.appHandle)
        for index in 0..<Int(shell.workspace_count) {
            let row = seyal_app_shell_row(
                pane.appHandle,
                UInt16(SEYAL_APP_ROW_WORKSPACE),
                UInt32(index)
            )
            if row.flags & UInt16(SEYAL_APP_ROW_SELECTED) != 0 {
                return row
            }
        }
        return nil
    }
}
