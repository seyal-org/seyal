import AppKit

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    private var window: NSWindow?
    private var host: ProductChromeHostView?

    func applicationDidFinishLaunching(_ notification: Notification) {
        let host = ProductChromeHostView(frame: NSRect(x: 0, y: 0, width: 1280, height: 800))
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

        installMenus()
        host.activateAfterWindowPresentation()
        NSApp.activate(ignoringOtherApps: true)
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        true
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        host?.requestQuit()
        host?.detachForTermination()
        return .terminateNow
    }

    private func installMenus() {
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

        let viewItem = NSMenuItem()
        mainMenu.addItem(viewItem)
        let viewMenu = NSMenu(title: "View")
        let paletteItem = NSMenuItem(
            title: "Command Palette",
            action: #selector(ProductChromeHostView.openCommandPalette),
            keyEquivalent: "k"
        )
        // Global command palette (#932). Target-less: AppKit walks the
        // responder chain, but `host` is the content view and not always
        // first responder (e.g. the terminal surface is), so target it
        // explicitly at the chrome host that owns the Rust-backed overlay.
        paletteItem.target = host
        viewMenu.addItem(paletteItem)
        viewItem.submenu = viewMenu

        NSApp.mainMenu = mainMenu
    }
}
