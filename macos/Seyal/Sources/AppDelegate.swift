import AppKit

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    private var host: MultiWindowHostController?

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

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        guard let host else {
            return .terminateNow
        }
        return host.applicationShouldTerminate()
    }

    private func installMenus(targeting host: MultiWindowHostController) {
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
        let newWindow = NSMenuItem(
            title: "New Window",
            action: #selector(MultiWindowHostController.createWindow(_:)),
            keyEquivalent: "n"
        )
        newWindow.target = host
        fileMenu.addItem(newWindow)
        let newTab = NSMenuItem(
            title: "New Tab",
            action: #selector(MultiWindowHostController.createTab(_:)),
            keyEquivalent: "t"
        )
        newTab.target = host
        fileMenu.addItem(newTab)
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
        let paletteItem = NSMenuItem(
            title: "Command Palette",
            action: #selector(ProductChromeHostView.openCommandPalette),
            keyEquivalent: "k"
        )
        paletteItem.target = host.liveHost
        viewMenu.addItem(paletteItem)
        viewItem.submenu = viewMenu

        let windowItem = NSMenuItem()
        mainMenu.addItem(windowItem)
        let windowMenu = NSMenu(title: "Window")
        let cycleNext = NSMenuItem(
            title: "Cycle Next Window",
            action: #selector(MultiWindowHostController.cycleWindowNext(_:)),
            keyEquivalent: "`"
        )
        cycleNext.keyEquivalentModifierMask = [.command]
        cycleNext.target = host
        windowMenu.addItem(cycleNext)
        let cyclePrev = NSMenuItem(
            title: "Cycle Previous Window",
            action: #selector(MultiWindowHostController.cycleWindowPrevious(_:)),
            keyEquivalent: "`"
        )
        cyclePrev.keyEquivalentModifierMask = [.command, .shift]
        cyclePrev.target = host
        windowMenu.addItem(cyclePrev)
        windowMenu.addItem(NSMenuItem.separator())
        // ⌥⌘1…9 select by snapshot order — NSMenuItem key equivalents only.
        for index in 1...9 {
            let item = NSMenuItem(
                title: "Select Window \(index)",
                action: #selector(MultiWindowHostController.selectWindowByTag(_:)),
                keyEquivalent: "\(index)"
            )
            item.keyEquivalentModifierMask = [.command, .option]
            item.tag = index - 1
            item.target = host
            windowMenu.addItem(item)
        }
        windowItem.submenu = windowMenu

        NSApp.mainMenu = mainMenu
    }
}
