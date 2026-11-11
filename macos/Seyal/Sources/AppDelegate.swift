import AppKit

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    private var host: MultiWindowHostController?

    init(host: MultiWindowHostController? = nil) {
        self.host = host
        super.init()
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        let host = MultiWindowHostController()
        self.host = host
        installMenus(targeting: host)
        host.bootstrapAfterLaunch()
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        // ADR-018 §3.3a: zero-Window composition stays running (Dock reopen /
        // ⌘⇧N re-enter). Closing the last window must not quit; Cmd-Q still
        // uses applicationShouldTerminate for confirm + bounded detach.
        false
    }

    func applicationShouldHandleReopen(
        _ sender: NSApplication,
        hasVisibleWindows flag: Bool
    ) -> Bool {
        // ADR-018 §3.3a: Rust window count is authority. AppKit's
        // hasVisibleWindows can stay true after the last product window is
        // destroyed (ordered-out / unreleased NSWindow), which previously
        // skipped zero-Window create on Dock click.
        guard let host else { return false }
        let productWindows = seyal_app_shell(host.appHandle).window_count
        if productWindows == 0 {
            host.handleDockReopen()
            return true
        }
        if !flag {
            // Miniaturized / occluded product windows: raise MRU, do not create.
            host.handleDockReopen()
            return true
        }
        // Visible product windows: AppKit brings them forward.
        return false
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        guard let host else {
            return .terminateNow
        }
        return host.applicationShouldTerminate()
    }

    static func makeMainMenu(targeting host: MultiWindowHostController) -> NSMenu {
        let mainMenu = NSMenu()

        let appItem = NSMenuItem()
        mainMenu.addItem(appItem)
        let appMenu = NSMenu()
        appMenu.addItem(
            withTitle: "Quit Seyal",
            action: #selector(NSApplication.terminate(_:)),
            keyEquivalent: "q"
        )
        appItem.submenu = appMenu

        let fileItem = NSMenuItem()
        mainMenu.addItem(fileItem)
        let fileMenu = NSMenu(title: "File")
        fileMenu.addItem(projectedItem(commandId: KeybindingShortcutRealization.windowNew, host: host))
        fileMenu.addItem(projectedItem(commandId: KeybindingShortcutRealization.terminalCreate, host: host))
        fileMenu.addItem(projectedItem(commandId: KeybindingShortcutRealization.tabCreate, host: host))
        fileMenu.addItem(projectedItem(commandId: KeybindingShortcutRealization.terminalCloseFocused, host: host))
        fileMenu.addItem(projectedItem(commandId: KeybindingShortcutRealization.tabCloseFocused, host: host))
        fileMenu.addItem(NSMenuItem.separator())
        fileMenu.addItem(projectedItem(commandId: KeybindingShortcutRealization.tabSelectPrevious, host: host))
        fileMenu.addItem(projectedItem(commandId: KeybindingShortcutRealization.tabSelectNext, host: host))
        fileItem.submenu = fileMenu

        let editItem = NSMenuItem()
        mainMenu.addItem(editItem)
        let editMenu = NSMenu(title: "Edit")
        editMenu.addItem(withTitle: "Cut", action: #selector(NSText.cut(_:)), keyEquivalent: "x")
        editMenu.addItem(withTitle: "Copy", action: #selector(NSText.copy(_:)), keyEquivalent: "c")
        editMenu.addItem(withTitle: "Paste", action: #selector(NSText.paste(_:)), keyEquivalent: "v")
        editItem.submenu = editMenu

        let viewItem = NSMenuItem()
        mainMenu.addItem(viewItem)
        let viewMenu = NSMenu(title: "View")
        viewMenu.addItem(projectedItem(commandId: KeybindingShortcutRealization.commandPaletteOpen, host: host))
        viewMenu.addItem(projectedItem(commandId: KeybindingShortcutRealization.gotoOpen, host: host))
        viewMenu.addItem(NSMenuItem.separator())
        // Toggle Raw / Set Flow are key-only (SPEC-024 is_menu_visible); Toggle TUI stays in View.
        viewMenu.addItem(projectedItem(commandId: KeybindingShortcutRealization.presentationToggleTui, host: host))
        viewMenu.addItem(projectedItem(commandId: KeybindingShortcutRealization.paneSplitRight, host: host))
        viewMenu.addItem(projectedItem(commandId: KeybindingShortcutRealization.paneSplitDown, host: host))
        viewMenu.addItem(NSMenuItem.separator())
        viewMenu.addItem(projectedItem(
            commandId: KeybindingShortcutRealization.chromeToggleLeftPanel,
            host: host
        ))
        viewMenu.addItem(projectedItem(
            commandId: KeybindingShortcutRealization.chromeToggleInspector,
            host: host
        ))
        viewItem.submenu = viewMenu

        let windowItem = NSMenuItem()
        mainMenu.addItem(windowItem)
        let windowMenu = NSMenu(title: "Window")
        windowMenu.addItem(projectedItem(commandId: KeybindingShortcutRealization.windowClose, host: host))
        windowMenu.addItem(projectedItem(commandId: KeybindingShortcutRealization.windowCycleNext, host: host))
        windowMenu.addItem(projectedItem(commandId: KeybindingShortcutRealization.windowCyclePrevious, host: host))
        windowMenu.addItem(NSMenuItem.separator())
        for ordinal in 1...9 {
            let item = NSMenuItem(
                title: "Select Window \(ordinal)",
                action: #selector(ProductChromeHostView.invokeProjectedWorkspaceCommand(_:)),
                keyEquivalent: ""
            )
            item.representedObject = KeybindingShortcutRealization.windowSelectOrdinal
            item.tag = ordinal
            item.target = host.liveHost
            windowMenu.addItem(item)
        }
        windowItem.submenu = windowMenu

        return mainMenu
    }

    private func installMenus(targeting host: MultiWindowHostController) {
        NSApp.mainMenu = Self.makeMainMenu(targeting: host)
    }

    private static func projectedItem(commandId: UInt16, host: MultiWindowHostController) -> NSMenuItem {
        let item = NSMenuItem(
            title: "",
            action: #selector(ProductChromeHostView.invokeProjectedWorkspaceCommand(_:)),
            keyEquivalent: ""
        )
        KeybindingShortcutRealization.realize(item, commandId: commandId)
        item.target = host.liveHost
        return item
    }
}
