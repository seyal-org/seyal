import AppKit

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    private var window: NSWindow?
    private var host: ProductChromeHostView?

    func applicationDidFinishLaunching(_ notification: Notification) {
        let initialSize = initialWindowSize
        let host = ProductChromeHostView(
            frame: NSRect(origin: .zero, size: initialSize)
        )
        self.host = host

        let window = NSWindow(
            contentRect: host.bounds,
            styleMask: [.titled, .closable, .miniaturizable, .resizable],
            backing: .buffered,
            defer: false
        )
        window.title = "Seyal"
        window.minSize = NSSize(width: 960, height: 640)
        // Appearance comes from Rust-resolved visual preference at host apply.
        let platform = NSApp.effectiveAppearance
        // Bounded non-secret diagnostics once per cold load — not on every chrome reconcile.
        NativeThemeRealization.surfaceColdDiagnosticsOnce(for: platform)
        let resolved = NativeThemeRealization.theme(for: platform)
        window.appearance = resolved.appearance
        window.contentView = host
        window.center()
        window.makeKeyAndOrderFront(nil)
        self.window = window

        installMenus(host: host)
        host.activateAfterWindowPresentation()
        NSApp.activate(ignoringOtherApps: true)

        #if DEBUG
        // AppKit may restore/zoom the previous window frame after launch. The
        // multi-Pane UI test opts into a compact frame so both real Pane hit
        // targets fit on the test display before it starts interacting.
        if ProcessInfo.processInfo.environment["SEYAL_UI_TEST_COMPACT_RESIZE_WINDOW"] == "1" {
            DispatchQueue.main.async {
                window.setContentSize(initialSize)
                window.center()
            }
        }
        #endif
    }

    private var initialWindowSize: NSSize {
        #if DEBUG
        if ProcessInfo.processInfo.environment["SEYAL_UI_TEST_COMPACT_RESIZE_WINDOW"] == "1" {
            return NSSize(width: 1100, height: 760)
        }
        #endif
        return NSSize(width: 1280, height: 800)
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        true
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        host?.requestQuit()
        host?.detachForTermination()
        return .terminateNow
    }

    /// Reserved §4.2 Edit/AppKit items keep hardcoded equivalents; product items
    /// realize Rust `KeybindingShortcutProjection` once at startup (R11.2–R11.3).
    private func installMenus(host: ProductChromeHostView) {
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

        let editItem = NSMenuItem()
        mainMenu.addItem(editItem)
        let editMenu = NSMenu(title: "Edit")
        editMenu.addItem(withTitle: "Cut", action: #selector(NSText.cut(_:)), keyEquivalent: "x")
        editMenu.addItem(withTitle: "Copy", action: #selector(NSText.copy(_:)), keyEquivalent: "c")
        editMenu.addItem(withTitle: "Paste", action: #selector(NSText.paste(_:)), keyEquivalent: "v")
        editItem.submenu = editMenu

        let fileItem = NSMenuItem()
        mainMenu.addItem(fileItem)
        let fileMenu = NSMenu(title: "File")
        fileMenu.addItem(projectedItem(
            commandId: KeybindingShortcutRealization.tabCreate,
            host: host
        ))
        fileMenu.addItem(projectedItem(
            commandId: KeybindingShortcutRealization.tabCloseFocused,
            host: host
        ))
        fileItem.submenu = fileMenu

        let viewItem = NSMenuItem()
        mainMenu.addItem(viewItem)
        let viewMenu = NSMenu(title: "View")
        viewMenu.addItem(projectedItem(
            commandId: KeybindingShortcutRealization.commandPaletteOpen,
            host: host
        ))
        viewMenu.addItem(projectedItem(
            commandId: KeybindingShortcutRealization.paneSplitRight,
            host: host
        ))
        viewMenu.addItem(projectedItem(
            commandId: KeybindingShortcutRealization.paneSplitDown,
            host: host
        ))
        viewMenu.addItem(projectedItem(
            commandId: KeybindingShortcutRealization.presentationToggleRaw,
            host: host
        ))
        viewMenu.addItem(projectedItem(
            commandId: KeybindingShortcutRealization.presentationToggleTui,
            host: host
        ))
        // R11.2 / R6.4.1: goto.open is a normal projected WorkspaceCommand —
        // title, optional equivalent, enablement, and invoke all go through Rust.
        viewMenu.addItem(projectedItem(
            commandId: KeybindingShortcutRealization.gotoOpen,
            host: host
        ))
        viewItem.submenu = viewMenu

        let windowItem = NSMenuItem()
        mainMenu.addItem(windowItem)
        let windowMenu = NSMenu(title: "Window")
        windowMenu.addItem(projectedItem(
            commandId: KeybindingShortcutRealization.tabSelectPrevious,
            host: host
        ))
        windowMenu.addItem(projectedItem(
            commandId: KeybindingShortcutRealization.tabSelectNext,
            host: host
        ))
        windowItem.submenu = windowMenu

        NSApp.mainMenu = mainMenu
    }

    private func projectedItem(commandId: UInt16, host: ProductChromeHostView) -> NSMenuItem {
        let item = NSMenuItem(
            title: "",
            action: #selector(ProductChromeHostView.invokeProjectedWorkspaceCommand(_:)),
            keyEquivalent: ""
        )
        item.target = host
        KeybindingShortcutRealization.realize(item, commandId: commandId)
        return item
    }
}
