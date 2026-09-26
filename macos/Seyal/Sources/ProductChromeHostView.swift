import AppKit

/// Thin AppKit projection of Rust shell/chrome/composer/recovery. No writable product model.
@MainActor
final class ProductChromeHostView: NSView {
    let pane: ThinPaneHostView
    private let material = NSVisualEffectView()
    private let tabStrip = NSView()
    private let tabTitle = NSTextField(labelWithString: "Terminal")
    private let left = NSView()
    private let inspector = NSStackView()
    private let attention = NSStackView()
    private let transcript = NSScrollView()
    private let blocks = NSStackView()
    private let composer: ComposerBridgeView
    /// Rust-owned history overlay (#933); internal for component tests.
    let historyOverlay: ComposerHistoryOverlayView
    /// Global command palette overlay (#932); internal for component tests.
    let commandPalette: CommandPaletteOverlayView
    private let workspacesButton = NSButton(title: "Workspaces", target: nil, action: nil)
    private let tabsButton = NSButton(title: "Tabs", target: nil, action: nil)
    private let recoveryLabel = NSTextField(labelWithString: "")
    private let leftItems = NSStackView()
    private let inspectorColumn = NSView()
    private let centerColumn = NSView()
    private var recoveryTimer: Timer?
    private var lastSnapshotGeneration: UInt64 = .max
    private var lastEligibility: UInt16 = .max
    private var lastProjectedExecution = (lo: UInt64(0), hi: UInt64(0))
    private var lastBlockCount: Int = 0
    /// Live-end follow for Flow transcript. New Blocks and async history-body
    /// growth keep the viewport pinned only while the user was already at the
    /// live end; intentional history scroll must not be yanked forward.
    private var followingLiveEnd = true
    private var isProgrammaticTranscriptScroll = false
    private var isReconcilingChrome = false
    /// Nested product/timeline pulses during a rebuild must not drop TUI.
    private var chromeNeedsReconcile = false
    private var blockCards: [UInt64: CommandBlockView] = [:]
    private var transcriptFrameRevision: UInt64 = 0
    private var paneFollowsTranscript: [NSLayoutConstraint] = []
    private var paneFillsCenter: [NSLayoutConstraint] = []
    private var centerLeadingHost: NSLayoutConstraint!
    private var centerTrailingHost: NSLayoutConstraint!
    private var centerTopHost: NSLayoutConstraint!
    private var centerLeadingLeft: NSLayoutConstraint!
    private var centerTrailingInspector: NSLayoutConstraint!
    private var centerTopTab: NSLayoutConstraint!

    override init(frame frameRect: NSRect) {
        pane = ThinPaneHostView(frame: frameRect)
        composer = ComposerBridgeView(appHandle: pane.appHandle)
        historyOverlay = ComposerHistoryOverlayView(appHandle: pane.appHandle)
        commandPalette = CommandPaletteOverlayView(appHandle: pane.appHandle)
        super.init(frame: frameRect)
        translatesAutoresizingMaskIntoConstraints = false
        setAccessibilityIdentifier("seyal-product-chrome")
        setAccessibilityElement(true)
        setAccessibilityRole(.group)
        wantsLayer = true

        material.translatesAutoresizingMaskIntoConstraints = false
        material.blendingMode = .behindWindow
        material.state = .active
        addSubview(material)

        configureChromeButtons()
        tabStrip.translatesAutoresizingMaskIntoConstraints = false
        tabStrip.wantsLayer = true
        tabStrip.setAccessibilityElement(true)
        tabStrip.setAccessibilityRole(.group)
        tabStrip.setAccessibilityIdentifier("seyal-tab-strip")
        tabTitle.font = .systemFont(ofSize: 13, weight: .semibold)
        tabTitle.translatesAutoresizingMaskIntoConstraints = false
        tabStrip.addSubview(tabTitle)

        left.translatesAutoresizingMaskIntoConstraints = false
        left.wantsLayer = true
        leftItems.orientation = .vertical
        leftItems.alignment = .leading
        leftItems.spacing = 2
        leftItems.translatesAutoresizingMaskIntoConstraints = false
        left.addSubview(workspacesButton)
        left.addSubview(tabsButton)
        left.addSubview(leftItems)
        workspacesButton.translatesAutoresizingMaskIntoConstraints = false
        tabsButton.translatesAutoresizingMaskIntoConstraints = false

        inspector.orientation = .vertical
        inspector.alignment = .leading
        inspector.spacing = 6
        inspector.translatesAutoresizingMaskIntoConstraints = false
        expose(inspector, identifier: "seyal-inspector")
        attention.orientation = .vertical
        attention.alignment = .leading
        expose(attention, identifier: "seyal-attention")
        recoveryLabel.font = .systemFont(ofSize: 11, weight: .regular)
        recoveryLabel.tag = 2
        recoveryLabel.setAccessibilityElement(true)
        recoveryLabel.setAccessibilityIdentifier("seyal-recovery")
        recoveryLabel.stringValue = "disconnected"
        recoveryLabel.translatesAutoresizingMaskIntoConstraints = false

        inspectorColumn.translatesAutoresizingMaskIntoConstraints = false
        inspectorColumn.wantsLayer = true
        inspectorColumn.addSubview(inspector)
        inspectorColumn.addSubview(attention)

        blocks.orientation = .vertical
        blocks.alignment = .width
        blocks.spacing = 22
        blocks.edgeInsets = NSEdgeInsets(top: 8, left: 0, bottom: 8, right: 0)
        blocks.translatesAutoresizingMaskIntoConstraints = false
        expose(blocks, identifier: "seyal-blocks")
        composer.setAccessibilityIdentifier("seyal-composer")

        let clip = TranscriptClipView()
        clip.drawsBackground = false
        clip.copiesOnScroll = false
        clip.postsBoundsChangedNotifications = true
        transcript.contentView = clip
        transcript.drawsBackground = false
        transcript.borderType = .noBorder
        transcript.hasVerticalScroller = true
        transcript.hasHorizontalScroller = false
        transcript.autohidesScrollers = true
        transcript.automaticallyAdjustsContentInsets = false
        transcript.translatesAutoresizingMaskIntoConstraints = false
        transcript.documentView = blocks
        transcript.setAccessibilityRole(.scrollArea)
        transcript.setAccessibilityIdentifier("seyal-blocks-scroll")

        centerColumn.translatesAutoresizingMaskIntoConstraints = false
        centerColumn.wantsLayer = true
        // Transcript chrome sits under the Pane Metal compositor. Flow clears
        // the drawable to transparent and paints only Block-body clips, so
        // command headers remain AppKit while output glyphs composite on top.
        centerColumn.addSubview(transcript)
        centerColumn.addSubview(pane)
        centerColumn.addSubview(composer)
        centerColumn.addSubview(historyOverlay)

        addSubview(tabStrip)
        addSubview(left)
        addSubview(centerColumn)
        addSubview(inspectorColumn)
        addSubview(recoveryLabel)
        recoveryLabel.alphaValue = 0
        recoveryLabel.setAccessibilityElement(true)

        // Topmost subview: the palette floats above every other region,
        // including the inspector, when Rust opens it.
        addSubview(commandPalette)
        NSLayoutConstraint.activate([
            commandPalette.leadingAnchor.constraint(equalTo: leadingAnchor),
            commandPalette.trailingAnchor.constraint(equalTo: trailingAnchor),
            commandPalette.topAnchor.constraint(equalTo: topAnchor),
            commandPalette.bottomAnchor.constraint(equalTo: bottomAnchor),
        ])
        commandPalette.onChanged = { [weak self] in
            self?.reconcileChrome()
        }
        commandPalette.onDismissed = { [weak self] in
            self?.routeFocus()
        }

        pane.setContentHuggingPriority(.defaultLow, for: .vertical)
        pane.setContentCompressionResistancePriority(.defaultLow, for: .vertical)
        composer.setContentHuggingPriority(.required, for: .vertical)
        transcript.setContentHuggingPriority(NSLayoutConstraint.Priority(1), for: .vertical)
        transcript.setContentCompressionResistancePriority(.defaultLow, for: .vertical)

        NSLayoutConstraint.activate([
            material.leadingAnchor.constraint(equalTo: leadingAnchor),
            material.trailingAnchor.constraint(equalTo: trailingAnchor),
            material.topAnchor.constraint(equalTo: topAnchor),
            material.bottomAnchor.constraint(equalTo: bottomAnchor),

            tabStrip.leadingAnchor.constraint(equalTo: leadingAnchor),
            tabStrip.trailingAnchor.constraint(equalTo: trailingAnchor),
            tabStrip.topAnchor.constraint(equalTo: topAnchor),
            tabStrip.heightAnchor.constraint(equalToConstant: 48),
            tabTitle.leadingAnchor.constraint(equalTo: tabStrip.leadingAnchor, constant: 236),
            tabTitle.centerYAnchor.constraint(equalTo: tabStrip.centerYAnchor),

            left.leadingAnchor.constraint(equalTo: leadingAnchor),
            left.topAnchor.constraint(equalTo: tabStrip.bottomAnchor),
            left.bottomAnchor.constraint(equalTo: bottomAnchor),
            left.widthAnchor.constraint(equalToConstant: 220),
            workspacesButton.leadingAnchor.constraint(equalTo: left.leadingAnchor, constant: 10),
            workspacesButton.topAnchor.constraint(equalTo: left.topAnchor, constant: 10),
            tabsButton.leadingAnchor.constraint(equalTo: workspacesButton.trailingAnchor, constant: 8),
            tabsButton.centerYAnchor.constraint(equalTo: workspacesButton.centerYAnchor),
            leftItems.leadingAnchor.constraint(equalTo: left.leadingAnchor, constant: 10),
            leftItems.trailingAnchor.constraint(equalTo: left.trailingAnchor, constant: -10),
            leftItems.topAnchor.constraint(equalTo: workspacesButton.bottomAnchor, constant: 12),
            leftItems.bottomAnchor.constraint(lessThanOrEqualTo: left.bottomAnchor, constant: -10),

            inspectorColumn.trailingAnchor.constraint(equalTo: trailingAnchor),
            inspectorColumn.topAnchor.constraint(equalTo: tabStrip.bottomAnchor),
            inspectorColumn.bottomAnchor.constraint(equalTo: bottomAnchor),
            inspectorColumn.widthAnchor.constraint(equalToConstant: 248),
            inspector.leadingAnchor.constraint(equalTo: inspectorColumn.leadingAnchor, constant: 10),
            inspector.trailingAnchor.constraint(equalTo: inspectorColumn.trailingAnchor, constant: -10),
            inspector.topAnchor.constraint(equalTo: inspectorColumn.topAnchor, constant: 10),
            attention.leadingAnchor.constraint(equalTo: inspector.leadingAnchor),
            attention.trailingAnchor.constraint(equalTo: inspector.trailingAnchor),
            attention.topAnchor.constraint(equalTo: inspector.bottomAnchor, constant: 12),
            recoveryLabel.leadingAnchor.constraint(equalTo: leadingAnchor),
            recoveryLabel.topAnchor.constraint(equalTo: topAnchor),
            recoveryLabel.widthAnchor.constraint(equalToConstant: 1),
            recoveryLabel.heightAnchor.constraint(equalToConstant: 1),

            centerColumn.bottomAnchor.constraint(equalTo: bottomAnchor),
            transcript.leadingAnchor.constraint(equalTo: centerColumn.leadingAnchor, constant: 20),
            transcript.trailingAnchor.constraint(equalTo: centerColumn.trailingAnchor, constant: -20),
            transcript.topAnchor.constraint(equalTo: centerColumn.topAnchor, constant: 16),
            transcript.heightAnchor.constraint(greaterThanOrEqualToConstant: 240),
            composer.leadingAnchor.constraint(equalTo: centerColumn.leadingAnchor, constant: 24),
            composer.trailingAnchor.constraint(equalTo: centerColumn.trailingAnchor, constant: -24),
            composer.topAnchor.constraint(equalTo: transcript.bottomAnchor, constant: 12),
            composer.bottomAnchor.constraint(equalTo: centerColumn.bottomAnchor, constant: -16),
            historyOverlay.leadingAnchor.constraint(equalTo: composer.leadingAnchor),
            historyOverlay.trailingAnchor.constraint(equalTo: composer.trailingAnchor),
            historyOverlay.bottomAnchor.constraint(equalTo: composer.topAnchor, constant: -8),
            blocks.topAnchor.constraint(equalTo: transcript.contentView.topAnchor),
            blocks.leadingAnchor.constraint(equalTo: transcript.contentView.leadingAnchor),
            blocks.widthAnchor.constraint(equalTo: transcript.contentView.widthAnchor),
        ])
        centerLeadingHost = centerColumn.leadingAnchor.constraint(equalTo: leadingAnchor)
        centerTrailingHost = centerColumn.trailingAnchor.constraint(equalTo: trailingAnchor)
        centerTopHost = centerColumn.topAnchor.constraint(equalTo: topAnchor)
        centerLeadingLeft = centerColumn.leadingAnchor.constraint(equalTo: left.trailingAnchor)
        centerTrailingInspector = centerColumn.trailingAnchor.constraint(
            equalTo: inspectorColumn.leadingAnchor
        )
        centerTopTab = centerColumn.topAnchor.constraint(equalTo: tabStrip.bottomAnchor)
        paneFollowsTranscript = [
            pane.leadingAnchor.constraint(equalTo: transcript.leadingAnchor),
            pane.trailingAnchor.constraint(equalTo: transcript.trailingAnchor),
            pane.topAnchor.constraint(equalTo: transcript.topAnchor),
            pane.bottomAnchor.constraint(equalTo: transcript.bottomAnchor),
        ]
        paneFillsCenter = [
            pane.leadingAnchor.constraint(equalTo: centerColumn.leadingAnchor),
            pane.trailingAnchor.constraint(equalTo: centerColumn.trailingAnchor),
            pane.topAnchor.constraint(equalTo: centerColumn.topAnchor),
            pane.bottomAnchor.constraint(equalTo: centerColumn.bottomAnchor),
        ]
        NSLayoutConstraint.activate(paneFollowsTranscript)
        applyShellChrome(seyal_app_chrome(pane.appHandle))

        composer.onSubmitComposer = { [weak self] command in
            self?.pane.inputSurface.terminalSubmitComposerCommand(command) ?? -10
        }
        composer.onSubmitRaw = { [weak self] command in
            self?.pane.inputSurface.terminalSubmitCommittedText(command) ?? -10
        }
        pane.inputSurface.onRequestComposerFocus = { [weak self] in
            self?.composer.focusEditor()
        }
        composer.onHistoryOpened = { [weak self] in
            self?.reconcileChrome()
        }
        historyOverlay.onChanged = { [weak self] in
            self?.reconcileChrome()
        }
        historyOverlay.onDismissed = { [weak self] in
            self?.composer.focusEditor()
        }
        pane.inputSurface.onTimelineChanged = { [weak self] in
            self?.projectRuntimeBlocks()
            self?.reconcileChrome()
            self?.refreshRunningBlockOutput()
        }
        pane.inputSurface.onHistoryRangeChanged = { [weak self] range in
            self?.applyHistoryRange(range)
        }
        pane.inputSurface.onComposerResultChanged = { [weak self] result in
            let accepted = result.code == .accepted || result.code == .unsupported
            self?.composer.applyComposerResult(requestID: result.requestID, accepted: accepted)
            self?.reconcileChrome()
        }
        pane.inputSurface.onComposerStatusChanged = { [weak self] status in
            self?.relayComposerStatus(status)
            self?.reconcileChrome()
        }
        pane.onProductChanged = { [weak self] in
            self?.reconcileChrome()
            // Frame advances (and ViewportLineIds remaps) arrive here via
            // ThinPaneHostView.onFrameChanged — keep running PRIMARY_CLIP
            // membership and card height in sync with each prepared generation.
            self?.refreshRunningBlockOutput()
        }
        NotificationCenter.default.addObserver(
            self,
            selector: #selector(transcriptDidScroll),
            name: NSView.boundsDidChangeNotification,
            object: clip
        )
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("ProductChromeHostView is programmatic")
    }

    @objc private func transcriptDidScroll() {
        if !isProgrammaticTranscriptScroll {
            followingLiveEnd = isNearLiveEnd()
        }
        publishBlockOutputFrame()
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        applyTheme()
    }

    var inputSurface: InteractiveMetalSurfaceView { pane.inputSurface }

    func activateAfterWindowPresentation() {
        beginRecovery()
        pane.activateAfterWindowPresentation()
        reconcileChrome()
        applyTheme()
        routeFocus()
    }

    func requestQuit() { pane.requestQuit() }
    func detachForTermination() { pane.detachForTermination() }

    func reconcileChrome() {
        if isReconcilingChrome {
            chromeNeedsReconcile = true
            return
        }
        isReconcilingChrome = true
        defer { isReconcilingChrome = false }
        var turns = 0
        repeat {
            chromeNeedsReconcile = false
            performChromeReconcile()
            turns += 1
        } while chromeNeedsReconcile && turns < 8
    }

    private func performChromeReconcile() {
        var snapshot = seyal_app_snapshot(pane.appHandle)
        let bound = (lo: snapshot.execution_lo, hi: snapshot.execution_hi)
        if snapshot.flags & UInt16(SEYAL_APP_SNAP_HAS_EXECUTION) != 0,
           bound != lastProjectedExecution
        {
            lastProjectedExecution = bound
            projectRuntimeBlocks()
            snapshot = seyal_app_snapshot(pane.appHandle)
        }
        let eligibilityChanged = snapshot.eligibility != lastEligibility
        if snapshot.generation == lastSnapshotGeneration && !eligibilityChanged {
            composer.reconcile()
            historyOverlay.reconcile()
            commandPalette.reconcile()
            driveRecovery()
            return
        }
        lastSnapshotGeneration = snapshot.generation
        // Stamp eligibility before Block history requests. Those calls notify
        // bridge status, which used to re-enter here with eligibilityChanged
        // still true and overflow the main-thread stack (nvim TUI takeover).
        if eligibilityChanged {
            lastEligibility = snapshot.eligibility
        }
        let chrome = seyal_app_chrome(pane.appHandle)
        applyShellChrome(chrome)
        let shell = seyal_app_shell(pane.appHandle)
        workspacesButton.state = chrome.left_panel == 0 ? .on : .off
        tabsButton.state = chrome.left_panel == 1 ? .on : .off
        rebuildLeft(shell: shell, leftPanel: chrome.left_panel)
        rebuildInspector(chrome)
        rebuildTabStrip(shell: shell)
        let direct = snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue)
            || snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue)
        if !direct {
            rebuildBlocks()
        }
        applyTranscriptPresentation(snapshot)
        recoveryLabel.stringValue = recoveryText(snapshot)
        composer.reconcile()
        historyOverlay.reconcile()
        commandPalette.reconcile()
        driveRecovery()
        if eligibilityChanged {
            routeFocus()
        }
        applyTheme()
    }

    /// Global keyboard-first command palette (#932): the menu action target.
    /// Opening is Rust-owned; a rejected open leaves focus untouched.
    @objc func openCommandPalette() {
        commandPalette.requestOpen()
    }

    func routeFocus() {
        // An open palette owns focus; eligibility-driven routing resumes
        // only after it closes (see `onDismissed`).
        guard !commandPalette.isOpen else { return }
        let snapshot = seyal_app_snapshot(pane.appHandle)
        let composerSnap = seyal_app_composer(pane.appHandle)
        if snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_FLOW.rawValue),
           composerSnap.mode != UInt16(SEYAL_APP_COMPOSER_HIDDEN.rawValue)
        {
            composer.focusEditor()
        } else if snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue)
            || snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue)
        {
            window?.makeFirstResponder(pane.inputSurface)
        }
    }

    private func rebuildTabStrip(shell: SeyalAppShell) {
        if shell.tab_count > 0 {
            let row = seyal_app_shell_row(pane.appHandle, UInt16(SEYAL_APP_ROW_TAB), 0)
            tabTitle.stringValue = copyUTF8(row.title, row.title_len) ?? "Terminal"
        }
    }

    private func rebuildLeft(shell: SeyalAppShell, leftPanel: UInt16) {
        leftItems.arrangedSubviews.forEach { $0.removeFromSuperview() }
        if leftPanel == 0 {
            for index in 0..<Int(shell.workspace_count) {
                let row = seyal_app_shell_row(pane.appHandle, UInt16(SEYAL_APP_ROW_WORKSPACE), UInt32(index))
                leftItems.addArrangedSubview(
                    rowButton(
                        title: copyUTF8(row.title, row.title_len) ?? "Workspace",
                        detail: copyUTF8(row.detail, row.detail_len),
                        identifier: "seyal-workspace-\(index)",
                        selected: row.flags & UInt16(SEYAL_APP_ROW_SELECTED) != 0,
                        action: #selector(selectWorkspace(_:)),
                        kind: UInt16(SEYAL_APP_ACTION_SELECT_WORKSPACE.rawValue),
                        idLo: row.id_lo,
                        idHi: row.id_hi
                    )
                )
            }
        } else {
            for index in 0..<Int(shell.tab_count) {
                let row = seyal_app_shell_row(pane.appHandle, UInt16(SEYAL_APP_ROW_TAB), UInt32(index))
                leftItems.addArrangedSubview(
                    rowButton(
                        title: copyUTF8(row.title, row.title_len) ?? "Tab",
                        detail: copyUTF8(row.detail, row.detail_len),
                        identifier: "seyal-tab-\(index)",
                        selected: row.flags & UInt16(SEYAL_APP_ROW_SELECTED) != 0,
                        action: #selector(selectTab(_:)),
                        kind: UInt16(SEYAL_APP_ACTION_SELECT_TAB.rawValue),
                        idLo: row.id_lo,
                        idHi: row.id_hi
                    )
                )
            }
        }
        for index in 0..<Int(shell.pane_count) {
            let row = seyal_app_shell_row(pane.appHandle, UInt16(SEYAL_APP_ROW_PANE), UInt32(index))
            leftItems.addArrangedSubview(
                rowButton(
                    title: copyUTF8(row.title, row.title_len) ?? "Pane",
                    detail: nil,
                    identifier: "seyal-pane-\(index)",
                    selected: row.flags & UInt16(SEYAL_APP_ROW_SELECTED) != 0,
                    action: #selector(focusPane(_:)),
                    kind: UInt16(SEYAL_APP_ACTION_FOCUS_PANE.rawValue),
                    idLo: row.id_lo,
                    idHi: row.id_hi
                )
            )
        }
    }

    private func rebuildInspector(_ chrome: SeyalAppChrome) {
        inspector.arrangedSubviews.forEach { $0.removeFromSuperview() }
        attention.arrangedSubviews.forEach { $0.removeFromSuperview() }
        for index in 0..<Int(chrome.inspector_row_count) {
            let row = seyal_app_chrome_row(pane.appHandle, UInt16(SEYAL_APP_ROW_INSPECTOR), UInt32(index))
            let title = copyUTF8(row.title, row.title_len) ?? ""
            let value = copyUTF8(row.detail, row.detail_len) ?? ""
            let caption = NSTextField(labelWithString: title)
            caption.font = .systemFont(ofSize: 10, weight: .medium)
            caption.tag = 2
            let body = NSTextField(labelWithString: value)
            body.font = .systemFont(ofSize: 12, weight: .regular)
            body.maximumNumberOfLines = 2
            inspector.addArrangedSubview(caption)
            inspector.addArrangedSubview(body)
        }
        for index in 0..<Int(chrome.attention_count) {
            let row = seyal_app_chrome_row(pane.appHandle, UInt16(SEYAL_APP_ROW_ATTENTION), UInt32(index))
            let identity = copyUTF8(row.title, row.title_len) ?? ""
            let title = copyUTF8(row.detail, row.detail_len) ?? identity
            let button = borderlessButton(title: title, action: #selector(openAttention(_:)))
            button.setAccessibilityIdentifier("seyal-attention-\(index)")
            button.identifier = NSUserInterfaceItemIdentifier(identity)
            attention.addArrangedSubview(button)
        }
        for index in 0..<Int(chrome.agent_count) {
            let row = seyal_app_chrome_row(pane.appHandle, UInt16(SEYAL_APP_ROW_AGENT), UInt32(index))
            let identity = copyUTF8(row.title, row.title_len) ?? ""
            let name = copyUTF8(row.detail, row.detail_len) ?? identity
            let button = borderlessButton(title: name, action: #selector(selectAgent(_:)))
            button.setAccessibilityIdentifier("seyal-agent-\(index)")
            button.identifier = NSUserInterfaceItemIdentifier(identity)
            button.state = row.flags & UInt16(SEYAL_APP_ROW_SELECTED) != 0 ? .on : .off
            attention.addArrangedSubview(button)
        }
    }

    private func applyShellChrome(_ chrome: SeyalAppChrome) {
        let leftOn = chrome.reserved & UInt32(SEYAL_APP_CHROME_LEFT_VISIBLE) != 0
        let inspectorOn = chrome.reserved & UInt32(SEYAL_APP_CHROME_INSPECTOR_VISIBLE) != 0
        let tabOn = chrome.reserved & UInt32(SEYAL_APP_CHROME_TAB_STRIP_VISIBLE) != 0
        left.isHidden = !leftOn
        left.setAccessibilityElement(leftOn)
        inspectorColumn.isHidden = !inspectorOn
        inspectorColumn.setAccessibilityElement(inspectorOn)
        inspector.setAccessibilityElement(inspectorOn)
        tabStrip.isHidden = !tabOn
        tabStrip.setAccessibilityElement(tabOn)
        centerLeadingHost.isActive = !leftOn
        centerLeadingLeft.isActive = leftOn
        centerTrailingHost.isActive = !inspectorOn
        centerTrailingInspector.isActive = inspectorOn
        centerTopHost.isActive = !tabOn
        centerTopTab.isActive = tabOn
    }

    private func projectRuntimeBlocks() {
        let snapshot = seyal_app_snapshot(pane.appHandle)
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_APPLY_RUNTIME_BLOCKS.rawValue)
        action.applySnapshotFence(snapshot)
        _ = seyal_app_apply(pane.appHandle, &action)
    }

    /// Relay Runtime's composer eligibility into the Rust root unchanged
    /// (#978). Rust decides what the composer shows; this only carries it.
    private func relayComposerStatus(_ status: NativeComposerStatus) {
        let snapshot = seyal_app_snapshot(pane.appHandle)
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_APPLY_COMPOSER_STATUS.rawValue)
        action.applySnapshotFence(snapshot)
        action.reserved = UInt32(status.eligibility)
        action.target_execution_lo = status.revision
        _ = seyal_app_apply(pane.appHandle, &action)
    }

    private func applyTranscriptPresentation(_ snapshot: SeyalAppSnapshot) {
        let direct = snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue)
            || snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue)
        transcript.isHidden = direct
        if direct {
            NSLayoutConstraint.deactivate(paneFollowsTranscript)
            NSLayoutConstraint.activate(paneFillsCenter)
            let mode: TerminalPresentationMode =
                snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue) ? .tui : .raw
            pane.inputSurface.applyRendererPresentation(.fullPane(mode))
            pane.inputSurface.setLiveTailBlocks([:])
            pane.inputSurface.removeTranscriptRegions(except: [])
            layoutSubtreeIfNeeded()
        } else {
            NSLayoutConstraint.deactivate(paneFillsCenter)
            NSLayoutConstraint.activate(paneFollowsTranscript)
            pane.inputSurface.applyRendererPresentation(.flow())
            layoutSubtreeIfNeeded()
            publishBlockOutputFrame()
        }
    }

    private func rebuildBlocks() {
        blocks.arrangedSubviews.forEach { $0.removeFromSuperview() }
        blockCards.removeAll()
        let composer = seyal_app_composer(pane.appHandle)
        let count = Int(composer.block_count)
        let cellHeight = pane.inputSurface.terminalPresentationCellSize().height
        var retained = Set<UInt64>()
        for index in 0..<count {
            let row = seyal_app_block_row(pane.appHandle, UInt32(index))
            let projection = seyal_app_block_projection(pane.appHandle, UInt32(index))
            let title = copyUTF8(row.title, row.title_len) ?? "command"
            let detail = copyUTF8(row.detail, row.detail_len) ?? ""
            let promptRow = seyal_app_copy(pane.appHandle, UInt16(SEYAL_APP_COPY_BLOCK_PROMPT))
            let prompt = copyUTF8(promptRow.title, promptRow.title_len) ?? "$"
            let blockID = row.id_lo
            let lines = outputLineCount(projection: projection)
            let card = CommandBlockView(
                prompt: prompt,
                title: title,
                detail: detail,
                state: row.flags & UInt16(SEYAL_APP_BLOCK_STATE_MASK),
                cellHeight: cellHeight,
                lines: lines
            )
            card.setAccessibilityIdentifier("seyal-block-\(index)")
            card.body.setAccessibilityIdentifier("seyal-block-\(index)-body")
            card.isSelected = row.flags & UInt16(SEYAL_APP_BLOCK_SELECTED) != 0
            let idLo = row.id_lo
            let idHi = row.id_hi
            card.onSelect = { [weak self] selected in
                self?.selectBlock(idLo: idLo, idHi: idHi, deselect: selected)
            }
            blocks.addArrangedSubview(card)
            if blockID != 0 {
                blockCards[blockID] = card
                retained.insert(blockID)
                applyBlockOutputProjection(blockID: blockID, index: UInt32(index))
            }
        }
        pane.inputSurface.discardHistoryRequests(except: retained)
        layoutSubtreeIfNeeded()
        publishBlockOutputFrame()
        publishLiveTailBlocks()
        if count > lastBlockCount, followingLiveEnd {
            scrollTranscriptToLiveEnd()
        }
        lastBlockCount = count
    }

    private func outputLineCount(projection: SeyalAppBlockProjection) -> Int {
        switch projection.kind {
        case UInt16(SEYAL_APP_BLOCK_PROJECTION_HISTORY):
            guard projection.start_line > 0, projection.end_line >= projection.start_line else {
                return 1
            }
            return Int(min(projection.end_line - projection.start_line + 1, 512))
        case UInt16(SEYAL_APP_BLOCK_PROJECTION_PRIMARY_CLIP):
            // Rust-owned row slice height (not the full prepared viewport).
            let rows = Int(projection.reserved1)
            return rows > 0 ? rows : 1
        default:
            return 1
        }
    }

    private func refreshRunningBlockOutput() {
        let snapshot = seyal_app_snapshot(pane.appHandle)
        if snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue)
            || snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue)
        {
            return
        }
        publishLiveTailBlocks()
        // Damage-driven primary clips refresh from Candidate-D frame updates.
        // Re-publish geometry so Block body height tracks the prepared rows.
        let composer = seyal_app_composer(pane.appHandle)
        let cellHeight = pane.inputSurface.terminalPresentationCellSize().height
        for index in 0..<Int(composer.block_count) {
            let row = seyal_app_block_row(pane.appHandle, UInt32(index))
            let projection = seyal_app_block_projection(pane.appHandle, UInt32(index))
            guard row.id_lo != 0,
                  projection.kind == UInt16(SEYAL_APP_BLOCK_PROJECTION_PRIMARY_CLIP)
            else { continue }
            if let card = blockCards[row.id_lo] {
                card.setOutputLines(outputLineCount(projection: projection), cellHeight: cellHeight)
            }
        }
        layoutSubtreeIfNeeded()
        publishBlockOutputFrame()
    }

    private func applyBlockOutputProjection(blockID: UInt64, index: UInt32) {
        let projection = seyal_app_block_projection(pane.appHandle, index)
        switch projection.kind {
        case UInt16(SEYAL_APP_BLOCK_PROJECTION_HISTORY):
            guard projection.start_line > 0, projection.end_line >= projection.start_line else {
                return
            }
            _ = pane.inputSurface.requestHistoryRange(
                startLine: projection.start_line,
                endLine: projection.end_line,
                blockID: blockID
            )
        case UInt16(SEYAL_APP_BLOCK_PROJECTION_PRIMARY_CLIP):
            // Live-tail uses the prepared primary frame; do not invent history.
            break
        default:
            break
        }
    }

    private func publishLiveTailBlocks() {
        let snapshot = seyal_app_snapshot(pane.appHandle)
        if snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue)
            || snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue)
        {
            pane.inputSurface.setLiveTailBlocks([:])
            return
        }
        let composer = seyal_app_composer(pane.appHandle)
        var live: [UInt64: LiveTailClip] = [:]
        for index in 0..<Int(composer.block_count) {
            let row = seyal_app_block_row(pane.appHandle, UInt32(index))
            let projection = seyal_app_block_projection(pane.appHandle, UInt32(index))
            guard row.id_lo != 0,
                  projection.kind == UInt16(SEYAL_APP_BLOCK_PROJECTION_PRIMARY_CLIP),
                  projection.start_line > 0,
                  projection.reserved1 > 0
            else { continue }
            let rowCount = UInt16(min(projection.reserved1, UInt32(UInt16.max)))
            live[row.id_lo] = LiveTailClip(
                startLine: projection.start_line,
                firstRow: projection.reserved0,
                rowCount: rowCount
            )
        }
        pane.inputSurface.setLiveTailBlocks(live)
    }

    private func applyHistoryRange(_ range: NativeHistoryRange) {
        pane.inputSurface.retainHistoryRange(range)
        let cellHeight = pane.inputSurface.terminalPresentationCellSize().height
        if let card = blockCards[range.blockID] {
            card.setOutputLines(max(range.rows.count, 1), cellHeight: cellHeight)
        }
        layoutSubtreeIfNeeded()
        publishBlockOutputFrame()
        // History replies arrive after the initial live-end scroll and can grow
        // earlier cards. Keep following only when the user was already at the
        // live end so the newly submitted Block stays hittable.
        if followingLiveEnd {
            scrollTranscriptToLiveEnd()
        }
    }

    private func publishBlockOutputFrame() {
        let snapshot = seyal_app_snapshot(pane.appHandle)
        let direct = snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue)
            || snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue)
        guard !direct else { return }
        let surface = pane.inputSurface
        var regions: [NativeTranscriptRegion] = []
        for (blockID, card) in blockCards {
            let clip = card.body.convert(card.body.bounds, to: surface)
            guard clip.width > 0, clip.height > 0 else { continue }
            regions.append(NativeTranscriptRegion(id: blockID, origin: clip.origin, clip: clip))
        }
        regions.sort { $0.id < $1.id }
        transcriptFrameRevision &+= 1
        surface.setTranscriptFrame(
            NativeTranscriptFrame(
                revision: transcriptFrameRevision,
                regions: regions,
                surfaceIdentity: ObjectIdentifier(surface)
            )
        )
    }

    private func isNearLiveEnd(tolerance: CGFloat = 24) -> Bool {
        let document = transcript.documentView ?? blocks
        let visible = transcript.contentView.bounds
        let height = document.fittingSize.height
        let maxY = max(height - visible.height, 0)
        return visible.origin.y >= maxY - tolerance
    }

    private func scrollTranscriptToLiveEnd() {
        isProgrammaticTranscriptScroll = true
        defer { isProgrammaticTranscriptScroll = false }
        let document = transcript.documentView ?? blocks
        let visible = transcript.contentView.bounds.height
        let height = document.fittingSize.height
        let y = max(height - visible, 0)
        transcript.contentView.scroll(to: NSPoint(x: 0, y: y))
        transcript.reflectScrolledClipView(transcript.contentView)
        followingLiveEnd = true
    }

    private func applyTheme() {
        let theme = NativeThemeRealization.theme(for: effectiveAppearance)
        NativeThemeRealization.apply(
            to: self,
            material: material,
            appearance: effectiveAppearance
        )
        left.layer?.backgroundColor = theme.utility.cgColor
        inspectorColumn.layer?.backgroundColor = theme.utility.cgColor
        tabStrip.layer?.backgroundColor = theme.container.cgColor
        centerColumn.layer?.backgroundColor = theme.canvas.cgColor
        transcript.backgroundColor = .clear
        left.layer?.borderWidth = 0
        composer.apply(theme: theme)
        historyOverlay.apply(theme: theme)
        commandPalette.apply(theme: theme)
        for view in blocks.arrangedSubviews {
            (view as? CommandBlockView)?.apply(theme: theme)
        }
    }

    private func recoveryText(_ snapshot: SeyalAppSnapshot) -> String {
        let stage: String
        switch snapshot.recovery_stage {
        case 6: stage = "connected"
        case 7: stage = "recovery exhausted"
        case 8: stage = "blocked"
        case 4, 5: stage = "restoring"
        case 0: stage = "disconnected"
        default: stage = "connecting"
        }
        if snapshot.recovery_stage == 6 {
            return stage
        }
        return "\(stage) · attempts \(snapshot.recovery_attempts)"
    }

    private func beginRecovery() {
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_BEGIN_RECOVERY.rawValue)
        action.target_pty_generation = UInt64(Date().timeIntervalSince1970 * 1000)
        _ = seyal_app_apply(pane.appHandle, &action)
        driveRecovery()
    }

    private func driveRecovery() {
        let snapshot = seyal_app_snapshot(pane.appHandle)
        switch snapshot.recovery_effect {
        case UInt32(SEYAL_APP_RECOVERY_EFFECT_PERFORM_ATTEMPT.rawValue):
            // CompleteRecovery replaces the effect queue. Acking here would
            // drop LaunchHelper without spawning Runtime.
            completeRecovery(connected: pane.inputSurface.terminalBridgeIsConnected)
            driveRecovery()
        case UInt32(SEYAL_APP_RECOVERY_EFFECT_SCHEDULE.rawValue):
            let delayMs = max(seyal_app_recovery_param(pane.appHandle), 10)
            recoveryTimer?.invalidate()
            let generation = snapshot.recovery_generation
            recoveryTimer = Timer.scheduledTimer(
                withTimeInterval: TimeInterval(delayMs) / 1000,
                repeats: false
            ) { [weak self] _ in
                DispatchQueue.main.async {
                    self?.fireRecovery(generation: generation)
                }
            }
            ackRecovery()
        case UInt32(SEYAL_APP_RECOVERY_EFFECT_LAUNCH_HELPER.rawValue):
            _ = BundledRuntimeLauncher().launch()
            ackRecovery()
            driveRecovery()
        case UInt32(SEYAL_APP_RECOVERY_EFFECT_DISPOSE_HANDLE.rawValue):
            seyal_bridge_disconnect_handle(seyal_app_recovery_param(pane.appHandle))
            ackRecovery()
            driveRecovery()
        default:
            break
        }
    }

    private func completeRecovery(connected: Bool, helperMissing: Bool = false) {
        let snapshot = seyal_app_snapshot(pane.appHandle)
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_COMPLETE_RECOVERY.rawValue)
        action.target_execution_lo = snapshot.recovery_generation
        action.target_pty_generation = UInt64(Date().timeIntervalSince1970 * 1000)
        if connected {
            action.reserved = UInt32(SEYAL_APP_RECOVERY_CONNECTED.rawValue)
        } else if helperMissing {
            action.reserved = UInt32(SEYAL_APP_RECOVERY_ENDPOINT_MISSING.rawValue)
                | (UInt32(SEYAL_APP_RECOVERY_LAUNCH_HELPER_MISSING.rawValue) << 8)
        } else {
            action.reserved = UInt32(SEYAL_APP_RECOVERY_ENDPOINT_MISSING.rawValue)
                | (UInt32(SEYAL_APP_RECOVERY_LAUNCH_STARTED.rawValue) << 8)
        }
        _ = seyal_app_apply(pane.appHandle, &action)
    }

    private func fireRecovery(generation: UInt64) {
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_FIRE_RECOVERY.rawValue)
        action.target_execution_lo = generation
        action.target_pty_generation = UInt64(Date().timeIntervalSince1970 * 1000)
        _ = seyal_app_apply(pane.appHandle, &action)
        driveRecovery()
    }

    private func ackRecovery() {
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_ACK_RECOVERY.rawValue)
        _ = seyal_app_apply(pane.appHandle, &action)
    }

    @objc private func showWorkspaces() {
        applyChromeKind(UInt16(SEYAL_APP_ACTION_SET_LEFT_PANEL.rawValue), reserved: 0)
    }

    @objc private func showTabs() {
        applyChromeKind(UInt16(SEYAL_APP_ACTION_SET_LEFT_PANEL.rawValue), reserved: 1)
    }

    @objc private func selectWorkspace(_ sender: NSButton) {
        applyIdentity(UInt16(SEYAL_APP_ACTION_SELECT_WORKSPACE.rawValue), button: sender)
    }

    @objc private func selectTab(_ sender: NSButton) {
        applyIdentity(UInt16(SEYAL_APP_ACTION_SELECT_TAB.rawValue), button: sender)
    }

    @objc private func focusPane(_ sender: NSButton) {
        applyIdentity(UInt16(SEYAL_APP_ACTION_FOCUS_PANE.rawValue), button: sender)
    }

    @objc private func openAttention(_ sender: NSButton) {
        applyPayload(UInt16(SEYAL_APP_ACTION_OPEN_ATTENTION.rawValue), text: sender.identifier?.rawValue ?? "")
    }

    @objc private func selectAgent(_ sender: NSButton) {
        applyPayload(UInt16(SEYAL_APP_ACTION_SELECT_AGENT.rawValue), text: sender.identifier?.rawValue ?? "")
    }

    private func applyChromeKind(_ kind: UInt16, reserved: UInt32) {
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = kind
        action.reserved = reserved
        _ = seyal_app_apply(pane.appHandle, &action)
        reconcileChrome()
    }

    private func applyIdentity(_ kind: UInt16, button: NSButton) {
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
    private func selectBlock(idLo: UInt64, idHi: UInt64, deselect: Bool) {
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

    private func applyPayload(_ kind: UInt16, text: String) {
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

    private func configureChromeButtons() {
        styleSwitcher(workspacesButton, identifier: "seyal-left-workspaces", action: #selector(showWorkspaces))
        styleSwitcher(tabsButton, identifier: "seyal-left-tabs", action: #selector(showTabs))
    }

    private func styleSwitcher(_ button: NSButton, identifier: String, action: Selector) {
        button.setButtonType(.toggle)
        button.bezelStyle = .inline
        button.isBordered = false
        button.font = .systemFont(ofSize: 11, weight: .semibold)
        button.setAccessibilityIdentifier(identifier)
        button.target = self
        button.action = action
    }

    private func borderlessButton(title: String, action: Selector) -> NSButton {
        let button = NSButton(title: title, target: self, action: action)
        button.bezelStyle = .inline
        button.isBordered = false
        button.font = .systemFont(ofSize: 12, weight: .regular)
        button.alignment = .left
        return button
    }

    private func rowButton(
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

    private func expose(_ view: NSView, identifier: String) {
        view.setAccessibilityElement(true)
        view.setAccessibilityRole(.group)
        view.setAccessibilityIdentifier(identifier)
    }
}

private final class TranscriptClipView: NSClipView {
    override var isFlipped: Bool { true }
}

private final class IdentityButton: NSButton {
    var kind: UInt16 = 0
    var idLo: UInt64 = 0
    var idHi: UInt64 = 0
}

private final class CommandBlockView: NSView {
    let body = NSView()
    /// Host click on the header; `true` when the card is already selected.
    var onSelect: ((Bool) -> Void)?
    /// Projected from the Rust block row's SEYAL_APP_BLOCK_SELECTED flag.
    var isSelected = false {
        didSet {
            setAccessibilityValue(isSelected ? "selected" : "")
            if let theme { apply(theme: theme) }
        }
    }
    private let header = NSView()
    private let prompt = NSTextField(labelWithString: "")
    private let command = NSTextField(labelWithString: "")
    private let status = NSTextField(labelWithString: "")
    private let seam = NSView()
    private let state: UInt16
    private var bodyHeight: NSLayoutConstraint!
    private var theme: NativeTheme?

    init(
        prompt: String,
        title: String,
        detail: String,
        state: UInt16,
        cellHeight: CGFloat,
        lines: Int
    ) {
        self.state = state
        super.init(frame: .zero)
        translatesAutoresizingMaskIntoConstraints = false
        wantsLayer = false
        setAccessibilityElement(true)
        setAccessibilityRole(.group)
        self.prompt.stringValue = prompt
        self.prompt.font = .monospacedSystemFont(ofSize: 13, weight: .medium)
        self.prompt.setContentHuggingPriority(.required, for: .horizontal)
        self.prompt.translatesAutoresizingMaskIntoConstraints = false
        command.stringValue = title.isEmpty ? "command" : title
        setAccessibilityLabel(command.stringValue)
        command.font = .monospacedSystemFont(ofSize: 13, weight: .medium)
        command.lineBreakMode = .byTruncatingTail
        command.translatesAutoresizingMaskIntoConstraints = false
        status.stringValue = detail
        status.isHidden = detail.isEmpty
        status.font = .monospacedSystemFont(ofSize: 11, weight: .regular)
        status.tag = 2
        status.setContentHuggingPriority(.required, for: .horizontal)
        status.translatesAutoresizingMaskIntoConstraints = false
        seam.translatesAutoresizingMaskIntoConstraints = false
        seam.wantsLayer = true
        header.translatesAutoresizingMaskIntoConstraints = false
        body.translatesAutoresizingMaskIntoConstraints = false
        body.wantsLayer = true
        body.layer?.isOpaque = false
        body.layer?.backgroundColor = NSColor.clear.cgColor
        body.setAccessibilityElement(true)
        body.setAccessibilityRole(.group)
        // The card is itself an accessibility element. Without an explicit
        // child list, XCUI cannot see the body identifier.
        header.addSubview(self.prompt)
        header.addSubview(command)
        header.addSubview(status)
        header.wantsLayer = true
        header.layer?.cornerRadius = 6
        addSubview(header)
        addSubview(body)
        addSubview(seam)
        bodyHeight = body.heightAnchor.constraint(
            equalToConstant: max(cellHeight, 1) * CGFloat(max(lines, 1))
        )
        NSLayoutConstraint.activate([
            header.leadingAnchor.constraint(equalTo: leadingAnchor),
            header.trailingAnchor.constraint(equalTo: trailingAnchor),
            header.topAnchor.constraint(equalTo: topAnchor),
            header.heightAnchor.constraint(greaterThanOrEqualToConstant: 22),
            self.prompt.leadingAnchor.constraint(equalTo: header.leadingAnchor),
            self.prompt.centerYAnchor.constraint(equalTo: header.centerYAnchor),
            command.leadingAnchor.constraint(equalTo: self.prompt.trailingAnchor, constant: 8),
            command.centerYAnchor.constraint(equalTo: header.centerYAnchor),
            status.trailingAnchor.constraint(equalTo: header.trailingAnchor),
            status.centerYAnchor.constraint(equalTo: header.centerYAnchor),
            command.trailingAnchor.constraint(lessThanOrEqualTo: status.leadingAnchor, constant: -12),
            body.leadingAnchor.constraint(equalTo: command.leadingAnchor),
            body.trailingAnchor.constraint(equalTo: trailingAnchor),
            body.topAnchor.constraint(equalTo: header.bottomAnchor, constant: 4),
            bodyHeight,
            seam.leadingAnchor.constraint(equalTo: leadingAnchor),
            seam.trailingAnchor.constraint(equalTo: trailingAnchor),
            seam.topAnchor.constraint(equalTo: body.bottomAnchor, constant: 12),
            seam.bottomAnchor.constraint(equalTo: bottomAnchor),
            seam.heightAnchor.constraint(equalToConstant: 1),
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("CommandBlockView is programmatic")
    }

    func setOutputLines(_ lines: Int, cellHeight: CGFloat) {
        bodyHeight.constant = max(cellHeight, 1) * CGFloat(max(lines, 1))
    }

    override func accessibilityChildren() -> [Any]? {
        // Body is the XCUI live-tail target. Header labels stay in the
        // accessibility tree so VoiceOver still hears prompt, command, and status.
        [prompt, command, status, body]
    }

    /// Flow's Metal surface returns `nil` from `hitTest`, so Block chrome must
    /// own the click. XCUI (and a user) hit the card center, which is the body
    /// once output exists — a header-only gesture never sees that click.
    override func hitTest(_ point: NSPoint) -> NSView? {
        super.hitTest(point) == nil ? nil : self
    }

    override func mouseDown(with event: NSEvent) {
        onSelect?(isSelected)
    }

    func apply(theme: NativeTheme) {
        self.theme = theme
        body.layer?.isOpaque = false
        body.layer?.backgroundColor = NSColor.clear.cgColor
        prompt.textColor = theme.accent
        command.textColor = theme.accent
        header.layer?.backgroundColor = isSelected
            ? theme.accent.withAlphaComponent(0.14).cgColor
            : NSColor.clear.cgColor
        if state == UInt16(SEYAL_APP_BLOCK_STATE_FAILED) {
            status.textColor = theme.danger
            seam.layer?.backgroundColor = theme.danger.withAlphaComponent(0.45).cgColor
        } else {
            status.textColor = theme.muted
            seam.layer?.backgroundColor = theme.seam.cgColor
        }
    }
}

private func copyUTF8(_ pointer: UnsafePointer<UInt8>?, _ length: UInt32) -> String? {
    guard length > 0, let pointer else { return nil }
    return String(decoding: UnsafeBufferPointer(start: pointer, count: Int(length)), as: UTF8.self)
}
