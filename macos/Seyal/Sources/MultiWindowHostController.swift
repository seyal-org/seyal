import AppKit

/// Thin AppKit realization of Rust window effects. No writable product model.
@MainActor
final class MultiWindowHostController: NSObject, NSWindowDelegate {
    struct WindowKey: Hashable {
        let lo: UInt64
        let hi: UInt64
    }

    /// Derived realization map keyed by WindowId; order follows the snapshot.
    private var realizations: [WindowKey: NSWindow] = [:]
    /// Snapshot order of realized WindowIds (AppKit z-order is not authority).
    private(set) var orderedKeys: [WindowKey] = []

    let liveHost: ProductChromeHostView
    private let quitCoordinator = ApplicationQuitCoordinator()
    private var liveKey: WindowKey?

    var appHandle: UInt64 { liveHost.pane.appHandle }
    var quitReplyCount: Int { quitCoordinator.replyCount }

    override init() {
        liveHost = ProductChromeHostView(frame: NSRect(x: 0, y: 0, width: 1280, height: 800))
        super.init()
        // Chain after ProductChromeHostView's own pane hook so window effects
        // apply whenever Rust commits a product transition.
        liveHost.pane.onProductChanged = { [weak self] in
            guard let self else { return }
            self.liveHost.reconcileChrome()
            self.applyPendingEffects()
            self.syncDestroyAgainstSnapshot()
            self.refreshTitlesFromSnapshot()
        }
    }

    func bootstrapAfterLaunch() {
        applyPendingEffectsAndReconcile()
        liveHost.activateAfterWindowPresentation()
        NSApp.activate(ignoringOtherApps: true)
    }

    func applyPendingEffectsAndReconcile() {
        applyPendingEffects()
        syncDestroyAgainstSnapshot()
        refreshTitlesFromSnapshot()
        liveHost.reconcileChrome()
    }

    /// UI order equals snapshot order (ADR-018 §1.3 / W4a).
    func snapshotOrderedWindowKeys() -> [WindowKey] {
        let shell = seyal_app_shell(appHandle)
        var keys: [WindowKey] = []
        keys.reserveCapacity(Int(shell.window_count))
        for index in 0..<Int(shell.window_count) {
            let row = seyal_app_window(appHandle, UInt32(index))
            guard row.size != 0 else { continue }
            keys.append(WindowKey(lo: row.window_lo, hi: row.window_hi))
        }
        return keys
    }

    func realizedTabbingModes() -> [NSWindow.TabbingMode] {
        orderedKeys.compactMap { realizations[$0]?.tabbingMode }
    }

    /// Test/host inspection of the derived realization map (not a product model).
    func realizedWindow(for key: WindowKey) -> NSWindow? {
        realizations[key]
    }

    func applyPendingEffects() {
        // Drain window effects only. Quit effects stay queued for §4.
        while true {
            let effect = seyal_app_native_effect(appHandle, 0)
            guard effect.size != 0 else { break }
            if effect.kind == UInt16(SEYAL_APP_EFFECT_BOUNDED_DETACH_THEN_TERMINATE)
                || effect.kind == UInt16(SEYAL_APP_EFFECT_QUIT_CLEANUP_COMPLETE)
            {
                break
            }
            applyEffect(effect)
            ackOneEffect()
        }
        orderedKeys = snapshotOrderedWindowKeys().filter { realizations[$0] != nil }
    }

    private func applyEffect(_ effect: SeyalAppNativeEffect) {
        let key = WindowKey(lo: effect.window_lo, hi: effect.window_hi)
        switch effect.kind {
        case UInt16(SEYAL_APP_EFFECT_REALIZE_WINDOW):
            realize(key)
        case UInt16(SEYAL_APP_EFFECT_DESTROY_WINDOW_REALIZATION):
            destroyRealization(key)
        case UInt16(SEYAL_APP_EFFECT_ORDER_FRONT_MAKE_KEY):
            orderFrontMakeKey(key)
        default:
            break
        }
    }

    private func realize(_ key: WindowKey) {
        guard realizations[key] == nil else { return }
        let window = makeSeyalWindow(for: key)
        realizations[key] = window
        if !orderedKeys.contains(key) {
            orderedKeys.append(key)
        }
        // One live Metal leaf: install live host only for the product-active window.
        if isProductActive(key) {
            installLiveHost(in: window, key: key)
        } else {
            window.contentView = InactiveWindowPlaceholderView(frame: window.contentLayoutRect)
        }
        window.makeKeyAndOrderFront(nil)
    }

    private func destroyRealization(_ key: WindowKey) {
        guard let window = realizations.removeValue(forKey: key) else { return }
        orderedKeys.removeAll { $0 == key }
        if liveKey == key {
            liveHost.removeFromSuperview()
            liveKey = nil
            // Park only on Rust's product-active window (ADR-015). Otherwise wait
            // for OrderFrontMakeKey — never pick an arbitrary orderedKeys entry.
            if let next = orderedKeys.first(where: { isProductActive($0) }),
               let hostWindow = realizations[next] {
                installLiveHost(in: hostWindow, key: next)
            }
        }
        window.delegate = nil
        window.contentView = nil
        // orderOut (not close): XCTest hosts may lack AppDelegate, and closing
        // the last NSWindow would terminate the test process. Production quit
        // still proceeds via reply(toApplicationShouldTerminate:).
        window.orderOut(nil)
    }

    private func orderFrontMakeKey(_ key: WindowKey) {
        guard let window = realizations[key] else { return }
        if liveKey != key {
            installLiveHost(in: window, key: key)
        }
        window.makeKeyAndOrderFront(nil)
    }

    private func installLiveHost(in window: NSWindow, key: WindowKey) {
        if let previous = liveKey, previous != key, let prior = realizations[previous] {
            prior.contentView = InactiveWindowPlaceholderView(frame: prior.contentLayoutRect)
        }
        liveHost.removeFromSuperview()
        window.contentView = liveHost
        liveKey = key
    }

    private func syncDestroyAgainstSnapshot() {
        let listed = Set(snapshotOrderedWindowKeys())
        for key in Array(realizations.keys) where !listed.contains(key) {
            destroyRealization(key)
        }
        orderedKeys = snapshotOrderedWindowKeys().filter { realizations[$0] != nil }
    }

    private func refreshTitlesFromSnapshot() {
        let shell = seyal_app_shell(appHandle)
        for index in 0..<Int(shell.window_count) {
            let row = seyal_app_window(appHandle, UInt32(index))
            let key = WindowKey(lo: row.window_lo, hi: row.window_hi)
            guard let window = realizations[key] else { continue }
            if row.title_len > 0, let bytes = row.title {
                window.title = String(
                    decoding: UnsafeBufferPointer(start: bytes, count: Int(row.title_len)),
                    as: UTF8.self
                )
            } else {
                window.title = "Seyal"
            }
        }
    }

    private func isProductActive(_ key: WindowKey) -> Bool {
        let shell = seyal_app_shell(appHandle)
        return shell.active_window_lo == key.lo && shell.active_window_hi == key.hi
    }

    private func makeSeyalWindow(for key: WindowKey) -> NSWindow {
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 1280, height: 800),
            styleMask: [.titled, .closable, .miniaturizable, .resizable],
            backing: .buffered,
            defer: false
        )
        window.title = "Seyal"
        window.minSize = NSSize(width: 960, height: 640)
        window.tabbingMode = .disallowed
        window.identifier = NSUserInterfaceItemIdentifier("seyal-window-\(key.lo)-\(key.hi)")
        window.delegate = self
        let platform = NSApp.effectiveAppearance
        NativeThemeRealization.surfaceColdDiagnosticsOnce(for: platform)
        window.appearance = NativeThemeRealization.theme(for: platform).appearance
        window.center()
        return window
    }

    private func ackOneEffect() {
        var ack = SeyalAppAction()
        ack.version = UInt16(SEYAL_APP_ABI_VERSION)
        ack.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        ack.kind = UInt16(SEYAL_APP_ACTION_ACK_EFFECT.rawValue)
        _ = seyal_app_apply(appHandle, &ack)
    }

    // MARK: - Quit (ADR-018 §4)

    func applicationShouldTerminate() -> NSApplication.TerminateReply {
        // Drain any window effects so BoundedDetachThenTerminate is head.
        applyPendingEffects()
        return quitCoordinator.beginTerminateLater(
            forwardRequestQuit: { [weak self] in
                guard let self else {
                    return .failure(QuitForwardError.missingHost)
                }
                return self.forwardRequestQuit()
            },
            performNativeCleanup: { [weak self] in
                self?.performQuitCleanup()
            },
            ackUntilCleanupComplete: { [weak self] in
                self?.ackUntilQuitCleanupComplete() ?? false
            },
            reply: {
                NSApp.reply(toApplicationShouldTerminate: true)
            }
        )
    }

    /// Test access to the §4 coordinator without driving NSApp.reply.
    var quitCoordinatorForTests: ApplicationQuitCoordinator { quitCoordinator }

    func forwardRequestQuit() -> Result<UInt64, Error> {
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_QUIT.rawValue)
        let rc = seyal_app_apply(appHandle, &action)
        guard rc == 0 else {
            return .failure(QuitForwardError.applyFailed(rc))
        }
        let effect = seyal_app_native_effect(appHandle, 0)
        guard effect.kind == UInt16(SEYAL_APP_EFFECT_BOUNDED_DETACH_THEN_TERMINATE),
              effect.size != 0
        else {
            return .failure(QuitForwardError.missingDeadline)
        }
        return .success(effect.window_lo)
    }

    func performQuitCleanup() {
        liveHost.detachForTermination()
        for key in Array(realizations.keys) {
            destroyRealization(key)
        }
    }

    @discardableResult
    func ackUntilQuitCleanupComplete() -> Bool {
        // Ack BoundedDetachThenTerminate → QuitCleanupComplete, then ack that.
        for _ in 0..<8 {
            let effect = seyal_app_native_effect(appHandle, 0)
            guard effect.size != 0 else { return false }
            if effect.kind == UInt16(SEYAL_APP_EFFECT_QUIT_CLEANUP_COMPLETE) {
                ackOneEffect()
                return true
            }
            ackOneEffect()
        }
        return false
    }

    // MARK: - Typed actions (menus)

    /// File → New Window / ⌘N: always the target-free New Window intent.
    /// Rust resolves Workspace (ADR-018 §2.2 / §3.3a). Never ActivateWorkspace.
    @objc func createWindow(_: Any?) {
        // ADR-018 §2.2 / §3.3a: target-free New Window — Rust resolves Workspace.
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_CREATE_WINDOW.rawValue)
        _ = seyal_app_apply(appHandle, &action)
        applyPendingEffectsAndReconcile()
    }

    /// Dock reopen: always forward ActivateWorkspace{last_active_workspace}.
    /// Rust owns create-versus-raise; Swift must not inspect window_count.
    func handleDockReopen() {
        let shell = seyal_app_shell(appHandle)
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_SELECT_WORKSPACE.rawValue)
        action.target_execution_lo = shell.last_active_workspace_lo
        action.target_execution_hi = shell.last_active_workspace_hi
        _ = seyal_app_apply(appHandle, &action)
        applyPendingEffectsAndReconcile()
    }

    @objc func createTab(_: Any?) {
        liveHost.createTab()
        applyPendingEffectsAndReconcile()
    }

    /// ⌘W hierarchical close: always ClosePane on the focused Pane. Rust peels
    /// Pane → Tab → Window (ADR-018 §6 / M001 scaffold). No keyDown path.
    @objc func hierarchicalClose(_: Any?) {
        let shell = seyal_app_shell(appHandle)
        guard shell.window_count > 0 else { return }
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_CLOSE_PANE.rawValue)
        action.target_execution_lo = shell.focused_pane_lo
        action.target_execution_hi = shell.focused_pane_hi
        _ = seyal_app_apply(appHandle, &action)
        applyPendingEffectsAndReconcile()
    }

    /// Title-bar / system close: forward CloseWindow; never destroy locally.
    func forwardCloseWindow(_ key: WindowKey) {
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_CLOSE_WINDOW.rawValue)
        action.target_execution_lo = key.lo
        action.target_execution_hi = key.hi
        _ = seyal_app_apply(appHandle, &action)
        applyPendingEffectsAndReconcile()
    }

    @objc func cycleWindowNext(_: Any?) {
        cycleWindow(previous: false)
    }

    @objc func cycleWindowPrevious(_: Any?) {
        cycleWindow(previous: true)
    }

    @objc func selectWindowByTag(_ sender: Any?) {
        guard let item = sender as? NSMenuItem else { return }
        let keys = snapshotOrderedWindowKeys()
        let index = item.tag
        guard index >= 0, index < keys.count else { return }
        let key = keys[index]
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_SELECT_WINDOW.rawValue)
        action.target_execution_lo = key.lo
        action.target_execution_hi = key.hi
        _ = seyal_app_apply(appHandle, &action)
        applyPendingEffectsAndReconcile()
    }

    private func cycleWindow(previous: Bool) {
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_CYCLE_WINDOW.rawValue)
        action.reserved = previous ? 1 : 0
        _ = seyal_app_apply(appHandle, &action)
        applyPendingEffectsAndReconcile()
    }

    func reportWindowEvent(window: NSWindow, event: UInt32) {
        guard let key = key(for: window) else { return }
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_REPORT_WINDOW_EVENT.rawValue)
        action.target_execution_lo = key.lo
        action.target_execution_hi = key.hi
        action.reserved = event
        _ = seyal_app_apply(appHandle, &action)
    }

    private func key(for window: NSWindow) -> WindowKey? {
        realizations.first(where: { $0.value === window })?.key
    }

    // MARK: - NSWindowDelegate (forward only; no product decisions)

    func windowShouldClose(_ sender: NSWindow) -> Bool {
        // ADR-018 §2.5: refuse local destroy; Rust emits DestroyWindowRealization.
        if let key = key(for: sender) {
            forwardCloseWindow(key)
        }
        return false
    }

    func windowDidBecomeKey(_ notification: Notification) {
        guard let window = notification.object as? NSWindow else { return }
        reportWindowEvent(window: window, event: SEYAL_APP_WINDOW_EVENT_BECAME_KEY)
    }

    func windowDidResignKey(_ notification: Notification) {
        guard let window = notification.object as? NSWindow else { return }
        reportWindowEvent(window: window, event: SEYAL_APP_WINDOW_EVENT_RESIGNED_KEY)
    }

    func windowDidBecomeMain(_ notification: Notification) {
        guard let window = notification.object as? NSWindow else { return }
        reportWindowEvent(window: window, event: SEYAL_APP_WINDOW_EVENT_BECAME_MAIN)
    }

    func windowDidResignMain(_ notification: Notification) {
        guard let window = notification.object as? NSWindow else { return }
        reportWindowEvent(window: window, event: SEYAL_APP_WINDOW_EVENT_RESIGNED_MAIN)
    }

    func windowDidChangeOcclusionState(_ notification: Notification) {
        guard let window = notification.object as? NSWindow else { return }
        reportWindowEvent(window: window, event: SEYAL_APP_WINDOW_EVENT_OCCLUSION_CHANGED)
    }

    func windowDidMiniaturize(_ notification: Notification) {
        guard let window = notification.object as? NSWindow else { return }
        reportWindowEvent(window: window, event: SEYAL_APP_WINDOW_EVENT_MINIATURIZED)
    }

    func windowDidDeminiaturize(_ notification: Notification) {
        guard let window = notification.object as? NSWindow else { return }
        reportWindowEvent(window: window, event: SEYAL_APP_WINDOW_EVENT_DEMINIATURIZED)
    }

    func windowDidEnterFullScreen(_ notification: Notification) {
        guard let window = notification.object as? NSWindow else { return }
        reportWindowEvent(window: window, event: SEYAL_APP_WINDOW_EVENT_ENTERED_FULLSCREEN)
    }

    func windowDidExitFullScreen(_ notification: Notification) {
        guard let window = notification.object as? NSWindow else { return }
        reportWindowEvent(window: window, event: SEYAL_APP_WINDOW_EVENT_EXITED_FULLSCREEN)
    }

    func windowDidChangeScreen(_ notification: Notification) {
        guard let window = notification.object as? NSWindow else { return }
        reportWindowEvent(window: window, event: SEYAL_APP_WINDOW_EVENT_SCREEN_OR_SCALE_CHANGED)
    }

    func windowDidChangeBackingProperties(_ notification: Notification) {
        guard let window = notification.object as? NSWindow else { return }
        reportWindowEvent(window: window, event: SEYAL_APP_WINDOW_EVENT_SCREEN_OR_SCALE_CHANGED)
    }
}

enum QuitForwardError: Error {
    case missingHost
    case applyFailed(Int32)
    case missingDeadline
}

/// Non-live window body. No second Metal leaf (W4a non-goal #936).
@MainActor
final class InactiveWindowPlaceholderView: NSView {
    private let label = NSTextField(labelWithString: "Seyal")

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        wantsLayer = true
        layer?.backgroundColor = NSColor.windowBackgroundColor.cgColor
        label.translatesAutoresizingMaskIntoConstraints = false
        addSubview(label)
        NSLayoutConstraint.activate([
            label.centerXAnchor.constraint(equalTo: centerXAnchor),
            label.centerYAnchor.constraint(equalTo: centerYAnchor),
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("InactiveWindowPlaceholderView is programmatic")
    }
}
