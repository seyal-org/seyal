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
        // ADR-018 §2.5: last-window-close never quits in M003 (W4b).
        false
    }

    func applicationShouldHandleReopen(
        _ sender: NSApplication,
        hasVisibleWindows flag: Bool
    ) -> Bool {
        // ADR-018 §3.3a: Dock reopen with no visible windows forwards ActivateWorkspace.
        if !flag {
            host?.handleDockReopen()
            return true
        }
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
        fileMenu.addItem(projectedItem(commandId: KeybindingShortcutRealization.appCloseFocused, host: host))
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
        viewMenu.addItem(projectedItem(commandId: KeybindingShortcutRealization.presentationToggleRaw, host: host))
        viewMenu.addItem(projectedItem(commandId: KeybindingShortcutRealization.presentationToggleTui, host: host))
        viewMenu.addItem(projectedItem(commandId: KeybindingShortcutRealization.paneSplitRight, host: host))
        viewMenu.addItem(projectedItem(commandId: KeybindingShortcutRealization.paneSplitDown, host: host))
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
