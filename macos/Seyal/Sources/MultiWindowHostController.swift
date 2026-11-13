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
    /// Close requests whose `windowShouldClose` veto is still on the stack.
    private var appKitCloseRequestsInFlight: Set<WindowKey> = []
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
        // Preserve ProductChromeHostView's per-frame LiveTail/plan refresh, then
        // drain window effects. Full chrome reconcile is NOT on this path —
        // Candidate-D frames must not pay performChromeReconcile (#1484 P0).
        // Timeline/status handlers and applyPendingEffectsAndReconcile own
        // reconcileChrome; ChromeActions that need chrome after a command call
        // it explicitly.
        let refreshProductHost = liveHost.pane.onProductChanged
        liveHost.pane.onProductChanged = { [weak self] in
            refreshProductHost?()
            guard let self else { return }
            // Candidate-D: drain window effects only. Shell title/destroy
            // snapshots every frame made simple echo laggy (#1484 P0).
            self.applyPendingEffects()
        }
    }

    func bootstrapAfterLaunch() {
        applyPendingEffectsAndReconcile()
        liveHost.activateAfterWindowPresentation()
        NSApp.activate(ignoringOtherApps: true)
        #if DEBUG
        // Hosted resize acceptance needs enough room for both real Pane targets
        // on smaller CI displays, even when AppKit restores a previous frame.
        if ProcessInfo.processInfo.environment["SEYAL_UI_TEST_COMPACT_RESIZE_WINDOW"] == "1" {
            DispatchQueue.main.async { [weak self] in
                guard let self,
                      let window = self.realizations.values.first(where: { $0.contentView === self.liveHost })
                else { return }
                self.placeCompactResizeWindow(window)
            }
        }
        #endif
    }

    func applyPendingEffectsAndReconcile() {
        applyPendingEffects()
        syncDestroyAgainstSnapshot()
        refreshTitlesFromSnapshot()
        liveHost.reconcileChrome()
    }

    #if DEBUG
    /// Once the hosted test drags the window, leave that size alone.
    private var compactClampArmed = true
    /// Re-entrancy fence for the hosted resize fixture's on-screen clamp.
    private var applyingCompactFrame = false
    #endif

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

    func realizedWindowForTests(_ key: WindowKey) -> NSWindow? {
        realizations[key]
    }

    func applyPendingEffects() {
        // Drain window effects only. Quit effects stay queued for §4.
        var drained = false
        while true {
            let effect = seyal_app_native_effect(appHandle, 0)
            guard effect.size != 0 else { break }
            if effect.kind == UInt16(SEYAL_APP_EFFECT_BOUNDED_DETACH_THEN_TERMINATE)
                || effect.kind == UInt16(SEYAL_APP_EFFECT_QUIT_CLEANUP_COMPLETE)
            {
                break
            }
            if !orderFrontIsSupersededByActivation(effect) {
                applyEffect(effect)
            }
            ackOneEffect()
            drained = true
        }
        guard drained else { return }
        orderedKeys = snapshotOrderedWindowKeys().filter { realizations[$0] != nil }
        // Title/destroy only after real window effects — not every Candidate-D.
        syncDestroyAgainstSnapshot()
        refreshTitlesFromSnapshot()
    }

    /// Cross-window navigation publishes product focus and an OrderFront
    /// projection before the retryable activation effect. Defer that raise
    /// until activation succeeds so a bounded failure leaves AppKit on the
    /// previous live host while Rust's committed focus remains authoritative.
    private func orderFrontIsSupersededByActivation(_ effect: SeyalAppNativeEffect) -> Bool {
        guard effect.kind == UInt16(SEYAL_APP_EFFECT_ORDER_FRONT_MAKE_KEY) else { return false }
        let next = seyal_app_native_effect(appHandle, 1)
        return next.kind == UInt16(SEYAL_APP_EFFECT_WINDOW_ACTIVATION)
            && next.window_lo == effect.window_lo
            && next.window_hi == effect.window_hi
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
        case UInt16(SEYAL_APP_EFFECT_WINDOW_ACTIVATION):
            realizeWindowActivation(key)
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
        // Install the live Pane surfaces only in Rust's product-active window.
        if isProductActive(key) {
            installLiveHost(in: window, key: key)
            // Zero-Window Dock/⌘⇧N re-entry must re-arm the Runtime bridge.
            // bootstrapAfterLaunch only activates once; after the last window
            // was destroyed (especially after multi-terminal close) the prior
            // Controller is gone and a fresh activate is required (#1402).
            liveHost.activateAfterWindowPresentation()
            liveHost.reconcileChrome()
        } else {
            window.contentView = InactiveWindowPlaceholderView(frame: window.contentLayoutRect)
        }
        if window.isMiniaturized {
            window.deminiaturize(nil)
        }
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    private func destroyRealization(_ key: WindowKey) {
        if pendingActivationKey == key {
            cancelPendingActivation()
        }
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
        window.contentView = nil
        // `windowShouldClose` vetoes AppKit's synchronous user request.
        // Defer `close()` until that call returns, or the veto swallows it.
        if appKitCloseRequestsInFlight.contains(key) {
            DispatchQueue.main.async { [window] in
                Self.finishAppKitClose(window)
            }
        } else {
            Self.finishAppKitClose(window)
        }
    }

    /// Drop the native realization from AppKit's window list.
    ///
    /// `isReleasedWhenClosed = false` orders the window out but leaves it in
    /// `NSApp.windows`. Releasing on close removes it. The extra retain
    /// balances AppKit's release so ARC stays the owner.
    private static func finishAppKitClose(_ window: NSWindow) {
        window.delegate = nil
        window.isReleasedWhenClosed = true
        _ = Unmanaged.passRetained(window)
        window.close()
    }

    private func orderFrontMakeKey(_ key: WindowKey) {
        guard let window = realizations[key] else { return }
        if liveKey != key {
            installLiveHost(in: window, key: key)
        }
        // Dock reopen with a miniaturized product window must deminiaturize
        // before makeKeyAndOrderFront (ADR-018 §3.3a raise path).
        if window.isMiniaturized {
            window.deminiaturize(nil)
        }
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    /// Remaining injected failures for the next `WindowActivation` (tests).
    var activationFailBudget: Int = 0
    /// Replaces AppKit's activation request to exercise OS refusal in tests.
    var activationRequestForTests: ((NSWindow) -> Void)?
    /// Provides AppKit's key-state observation when tests need deterministic success.
    var keyWindowStatusForTests: ((NSWindow) -> Bool)?
    /// Observes completion without depending on XCTest's foreground activation state.
    var activationCompletionForTests: ((Bool) -> Void)?
    /// Attempts spent on the last `WindowActivation` effect (tests).
    private(set) var lastActivationAttempts: Int = 0
    /// Captures the native failure report at its synchronous FFI boundary (tests).
    private(set) var lastActivationFailureReportForTests: (result: Int32, error: Int32)?
    private static let activationAcknowledgementDelay: TimeInterval = 0.05
    private var activationGeneration: UInt64 = 0
    private var activationWorkItem: DispatchWorkItem?
    private var pendingActivationKey: WindowKey?

    /// Realize SPEC-022 WindowActivation: only the named window, bounded asynchronous retry.
    private func realizeWindowActivation(_ key: WindowKey) {
        cancelPendingActivation()
        let generation = activationGeneration
        lastActivationAttempts = 0
        lastActivationFailureReportForTests = nil
        pendingActivationKey = key
        scheduleActivationAttempt(key, generation: generation)
    }

    private func scheduleActivationAttempt(_ key: WindowKey, generation: UInt64, after delay: TimeInterval = 0) {
        guard pendingActivationKey == key, activationGeneration == generation else { return }
        let work = DispatchWorkItem { [weak self] in
            self?.attemptNamedWindowActivation(key, generation: generation)
        }
        activationWorkItem = work
        DispatchQueue.main.asyncAfter(deadline: .now() + delay, execute: work)
    }

    private func attemptNamedWindowActivation(_ key: WindowKey, generation: UInt64) {
        guard pendingActivationKey == key, activationGeneration == generation else { return }
        guard let window = realizations[key] else {
            completeWindowActivation(key, generation: generation, succeeded: false)
            return
        }

        lastActivationAttempts += 1
        if activationFailBudget > 0 {
            activationFailBudget -= 1
            retryOrFailWindowActivation(key, generation: generation)
            return
        }

        // Host must not pick a different window than the effect names.
        if let activationRequestForTests {
            activationRequestForTests(window)
        } else {
            NSApp.activate(ignoringOtherApps: true)
            window.makeKeyAndOrderFront(nil)
        }

        // AppKit can complete key-window activation on a later event turn.
        // A bounded delayed acknowledgement avoids both a false immediate
        // failure and a nested event loop that could reenter effect draining.
        let acknowledgement = DispatchWorkItem { [weak self] in
            guard let self,
                  self.pendingActivationKey == key,
                  self.activationGeneration == generation
            else { return }
            guard let target = self.realizations[key] else {
                self.completeWindowActivation(key, generation: generation, succeeded: false)
                return
            }
            if self.isKeyWindow(target) {
                self.completeWindowActivation(key, generation: generation, succeeded: true)
            } else {
                self.retryOrFailWindowActivation(key, generation: generation)
            }
        }
        activationWorkItem = acknowledgement
        DispatchQueue.main.asyncAfter(
            deadline: .now() + Self.activationAcknowledgementDelay,
            execute: acknowledgement
        )
    }

    private func retryOrFailWindowActivation(_ key: WindowKey, generation: UInt64) {
        guard pendingActivationKey == key, activationGeneration == generation else { return }
        if lastActivationAttempts >= Int(SEYAL_APP_WINDOW_ACTIVATION_MAX_ATTEMPTS) {
            completeWindowActivation(key, generation: generation, succeeded: false)
        } else {
            scheduleActivationAttempt(
                key,
                generation: generation,
                after: Self.activationAcknowledgementDelay
            )
        }
    }

    private func completeWindowActivation(_ key: WindowKey, generation: UInt64, succeeded: Bool) {
        guard pendingActivationKey == key, activationGeneration == generation else { return }
        activationWorkItem?.cancel()
        activationWorkItem = nil
        pendingActivationKey = nil

        if succeeded, let window = realizations[key], liveKey != key {
            installLiveHost(in: window, key: key)
        } else if !succeeded {
            reportActivationFailed(key)
        }
        activationCompletionForTests?(succeeded)
    }

    private func cancelPendingActivation() {
        activationGeneration &+= 1
        activationWorkItem?.cancel()
        activationWorkItem = nil
        pendingActivationKey = nil
    }

    private func isKeyWindow(_ window: NSWindow) -> Bool {
        if let keyWindowStatusForTests {
            return keyWindowStatusForTests(window)
        }
        return window.isKeyWindow && NSApp.keyWindow === window
    }

    private func reportActivationFailed(_ key: WindowKey) {
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_REPORT_WINDOW_EVENT.rawValue)
        action.target_execution_lo = key.lo
        action.target_execution_hi = key.hi
        action.reserved = SEYAL_APP_WINDOW_EVENT_ACTIVATION_FAILED
        let result = seyal_app_apply(appHandle, &action)
        lastActivationFailureReportForTests = (result, seyal_app_last_error(appHandle))
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

    private var initialWindowSize: NSSize {
        #if DEBUG
        if ProcessInfo.processInfo.environment["SEYAL_UI_TEST_COMPACT_RESIZE_WINDOW"] == "1" {
            return NSSize(width: 1100, height: 760)
        }
        #endif
        return NSSize(width: 1280, height: 800)
    }

    private func makeSeyalWindow(for key: WindowKey) -> NSWindow {
        let window = NSWindow(
            contentRect: NSRect(origin: .zero, size: initialWindowSize),
            styleMask: [.titled, .closable, .miniaturizable, .resizable],
            backing: .buffered,
            defer: false
        )
        window.isReleasedWhenClosed = false
        window.title = "Seyal"
        window.minSize = NSSize(width: 960, height: 640)
        window.standardWindowButton(.closeButton)?
            .setAccessibilityIdentifier("seyal-window-close")
        #if DEBUG
        // A restored frame wider than the display puts the resize corner
        // off-screen, so the hosted two-pane resize drag never starts.
        if ProcessInfo.processInfo.environment["SEYAL_UI_TEST_COMPACT_RESIZE_WINDOW"] == "1" {
            window.isRestorable = false
        }
        #endif
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

    /// Test seam for ADR-018 §4.0 confirm. Production uses `NSAlert.runModal`.
    var quitConfirmationHandlerForTests: (() -> Bool)?
    /// Test seam for §4.1 `reply(true)` without driving `NSApp.reply`.
    var quitTerminateReplyHandlerForTests: (() -> Void)?

    func applicationShouldTerminate() -> NSApplication.TerminateReply {
        if seyal_app_should_confirm_quit(appHandle) != 0 {
            if !presentQuitConfirmation() {
                return .terminateCancel
            }
        }
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
            reply: { [weak self] in
                if let handler = self?.quitTerminateReplyHandlerForTests {
                    handler()
                } else {
                    NSApp.reply(toApplicationShouldTerminate: true)
                }
            }
        )
    }

    /// ADR-018 §4.0 native adapter: Rust owns should-confirm; host only presents.
    func presentQuitConfirmation() -> Bool {
        if let handler = quitConfirmationHandlerForTests {
            return handler()
        }
        let alert = NSAlert()
        alert.messageText = "Quit Seyal?"
        alert.informativeText =
            "You have multiple windows or tabs open. Do you want to quit Seyal?"
        alert.alertStyle = .warning
        alert.addButton(withTitle: "Quit")
        alert.addButton(withTitle: "Cancel")
        return alert.runModal() == .alertFirstButtonReturn
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
        cancelPendingActivation()
        liveHost.detachForTermination()
        for key in Array(realizations.keys) {
            destroyRealization(key)
        }
        // ADR-018 §4.0 / §13: intentional headed Quit must clear Dock. A
        // GUI-spawned helper left alive keeps Seyal as (exited-with-subordinates).
        BundledRuntimeLauncher.terminateLaunchedHelperForHeadedQuit()
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

    @objc func createWindow(_: Any?) {
        // ADR-018 §2.2 / §3.3a: target-free New Window — Rust resolves Workspace.
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_CREATE_WINDOW.rawValue)
        _ = seyal_app_apply(appHandle, &action)
        applyPendingEffectsAndReconcile()
    }

    @objc func createTab(_: Any?) {
        liveHost.createTab()
        applyPendingEffectsAndReconcile()
    }

    /// Dock reopen: forward ActivateWorkspace for the last active workspace.
    func handleDockReopen() {
        let before = seyal_app_shell(appHandle)
        if before.window_count == 0 {
            var action = SeyalAppAction()
            action.version = UInt16(SEYAL_APP_ABI_VERSION)
            action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
            action.kind = UInt16(SEYAL_APP_ACTION_SELECT_WORKSPACE.rawValue)
            action.target_execution_lo = before.last_active_workspace_lo
            action.target_execution_hi = before.last_active_workspace_hi
            let code = seyal_app_apply(appHandle, &action)
            applyPendingEffectsAndReconcile()
            // Fall back to CreateWindow if ActivateWorkspace did not materialize
            // a Window (stale workspace id / provisioning rollback).
            if code != 0 || seyal_app_shell(appHandle).window_count == 0 {
                createWindow(nil)
            }
            ensureProductWindowActivated()
            return
        }
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_SELECT_WORKSPACE.rawValue)
        action.target_execution_lo = before.last_active_workspace_lo
        action.target_execution_hi = before.last_active_workspace_hi
        _ = seyal_app_apply(appHandle, &action)
        applyPendingEffectsAndReconcile()
        // Rust may still list Windows while AppKit realizations were cleared.
        realizeMissingSnapshotWindows()
        ensureProductWindowActivated()
    }

    /// Re-create AppKit realizations for any Rust Windows missing a host window.
    private func realizeMissingSnapshotWindows() {
        for key in snapshotOrderedWindowKeys() where realizations[key] == nil {
            realize(key)
        }
        orderedKeys = snapshotOrderedWindowKeys().filter { realizations[$0] != nil }
    }

    private func ensureProductWindowActivated() {
        guard let key = snapshotOrderedWindowKeys().first(where: { isProductActive($0) })
            ?? orderedKeys.first
        else { return }
        if realizations[key] == nil {
            realize(key)
        }
        orderFrontMakeKey(key)
        liveHost.activateAfterWindowPresentation()
        liveHost.reconcileChrome()
    }

    /// ⌘W hierarchical close: ClosePane on the focused Pane. Rust peels
    /// Pane → Tab → Window.
    @objc func hierarchicalClose(_: Any?) {
        let shell = seyal_app_shell(appHandle)
        guard shell.window_count > 0 else { return }
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_CLOSE_PANE.rawValue)
        action.target_execution_lo = shell.focused_pane_lo
        action.target_execution_hi = shell.focused_pane_hi
        action.target_generation = shell.containment_generation
        _ = seyal_app_apply(appHandle, &action)
        applyPendingEffectsAndReconcile()
    }

    /// Title-bar close: forward CloseWindow. Destruction comes from Rust.
    func forwardCloseWindow(_ key: WindowKey) {
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_CLOSE_WINDOW.rawValue)
        action.target_execution_lo = key.lo
        action.target_execution_hi = key.hi
        action.target_generation = seyal_app_shell(appHandle).containment_generation
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
        if let key = key(for: sender) {
            appKitCloseRequestsInFlight.insert(key)
            defer { appKitCloseRequestsInFlight.remove(key) }
            forwardCloseWindow(key)
        }
        return false
    }

    func windowDidBecomeKey(_ notification: Notification) {
        guard let window = notification.object as? NSWindow else { return }
        if let key = key(for: window), pendingActivationKey == key, isKeyWindow(window) {
            completeWindowActivation(key, generation: activationGeneration, succeeded: true)
        }
        reportWindowEvent(window: window, event: SEYAL_APP_WINDOW_EVENT_BECAME_KEY)
        if let key = key(for: window), isProductActive(key), liveKey != key {
            installLiveHost(in: window, key: key)
        }
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
        var event = SEYAL_APP_WINDOW_EVENT_OCCLUSION_CHANGED
        if !window.occlusionState.contains(.visible) {
            event |= SEYAL_APP_WINDOW_EVENT_OCCLUSION_HIDDEN
        }
        reportWindowEvent(window: window, event: event)
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

    func windowWillStartLiveResize(_ notification: Notification) {
        #if DEBUG
        if ProcessInfo.processInfo.environment["SEYAL_UI_TEST_COMPACT_RESIZE_WINDOW"] == "1" {
            compactClampArmed = false
        }
        #endif
    }

    func windowDidResize(_ notification: Notification) {
        #if DEBUG
        guard compactClampArmed,
              !applyingCompactFrame,
              ProcessInfo.processInfo.environment["SEYAL_UI_TEST_COMPACT_RESIZE_WINDOW"] == "1",
              let window = notification.object as? NSWindow,
              !window.inLiveResize,
              let screen = window.screen ?? NSScreen.main,
              window.frame.width > screen.visibleFrame.width - 40
        else { return }
        // AppKit forbids setFrame inside the resize notification that layout
        // is still posting. Clamp on the next turn, and only while the frame
        // is too wide for the resize corner to stay on screen.
        applyingCompactFrame = true
        DispatchQueue.main.async { [weak self, weak window] in
            guard let self, let window else { return }
            self.placeCompactResizeWindow(window)
        }
        #endif
    }

    #if DEBUG
    private func placeCompactResizeWindow(_ window: NSWindow) {
        applyingCompactFrame = true
        defer { applyingCompactFrame = false; compactClampArmed = false }
        if let content = window.contentView {
            relaxWideConstraints(in: content)
        }
        let visible = (window.screen ?? NSScreen.main)?.visibleFrame
        let width = min(1100, max(960, (visible?.width ?? 1100) - 220))
        let height = min(760, max(640, (visible?.height ?? 760) - 80))
        var frame = window.frameRect(
            forContentRect: NSRect(x: 0, y: 0, width: width, height: height))
        guard let visible else {
            window.setFrame(frame, display: true)
            window.center()
            return
        }
        frame.origin.x = visible.minX + 8
        frame.origin.y = max(visible.minY + 8, visible.maxY - frame.height - 8)
        window.setFrame(frame, display: true)
    }

    /// Autoresizing masks inside the pane layout become required widths.
    /// A stale wide one keeps a restored frame from accepting the compact size.
    private func relaxWideConstraints(in view: NSView) {
        for constraint in view.constraints where constraint.firstAttribute == .width && constraint.constant > 700 {
            constraint.priority = .defaultLow
        }
        for child in view.subviews {
            relaxWideConstraints(in: child)
        }
    }
    #endif
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
