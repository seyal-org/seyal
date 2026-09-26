import AppKit

/// Thin AppKit projection of Rust shell/chrome/composer/recovery. No writable product model.
@MainActor
final class ProductChromeHostView: NSView {
    let pane: ThinPaneHostView
    let material = NSVisualEffectView()
    /// Headed/component probe for cold visual snapshot. Identifier-encoded so
    /// VoiceOver does not hear a pipe-delimited token string as the chrome value.
    let coldVisualProbe = NSView()
    let tabStrip = NSView()
    let tabTitle = NSTextField(labelWithString: "Terminal")
    /// Layout-chrome cluster (#922): apply to the active Tab/focused Pane,
    /// never duplicated per-Pane (M001-CORE-TERMINAL-REFERENCE-SCREEN.md §4.3).
    let newTabButton = NSButton(title: "+", target: nil, action: nil)
    let closeTabButton = IdentityButton(title: "Close Tab", target: nil, action: nil)
    let splitRightButton = NSButton(title: "Split Right", target: nil, action: nil)
    let splitDownButton = NSButton(title: "Split Down", target: nil, action: nil)
    let closePaneButton = IdentityButton(title: "Close Pane", target: nil, action: nil)
    let left = NSView()
    let inspector = NSStackView()
    let attention = NSStackView()
    let transcript = NSScrollView()
    let blocks = NSStackView()
    let composer: ComposerBridgeView
    /// Rust-owned history overlay (#933); internal for component tests.
    let historyOverlay: ComposerHistoryOverlayView
    /// Global command palette overlay (#932); internal for component tests.
    let commandPalette: CommandPaletteOverlayView
    let workspacesButton = NSButton(title: "Workspaces", target: nil, action: nil)
    let tabsButton = NSButton(title: "Tabs", target: nil, action: nil)
    let recoveryLabel = NSTextField(labelWithString: "")
    let leftItems = NSStackView()
    let inspectorColumn = NSView()
    let centerColumn = NSView()
    var recoveryTimer: Timer?
    var lastSnapshotGeneration: UInt64 = .max
    var lastEligibility: UInt16 = .max
    var lastProjectedExecution = (lo: UInt64(0), hi: UInt64(0))
    var lastBlockCount: Int = 0
    /// Live-end follow for Flow transcript. New Blocks and async history-body
    /// growth keep the viewport pinned only while the user was already at the
    /// live end; intentional history scroll must not be yanked forward.
    var followingLiveEnd = true
    var isProgrammaticTranscriptScroll = false
    var isReconcilingChrome = false
    /// Nested product/timeline pulses during a rebuild must not drop TUI.
    var chromeNeedsReconcile = false
    var blockCards: [UInt64: CommandBlockView] = [:]
    var transcriptFrameRevision: UInt64 = 0
    var paneFollowsTranscript: [NSLayoutConstraint] = []
    var paneFillsCenter: [NSLayoutConstraint] = []
    var centerLeadingHost: NSLayoutConstraint!
    var centerTrailingHost: NSLayoutConstraint!
    var centerTopHost: NSLayoutConstraint!
    var centerLeadingLeft: NSLayoutConstraint!
    var centerTrailingInspector: NSLayoutConstraint!
    var centerTopTab: NSLayoutConstraint!

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

        coldVisualProbe.translatesAutoresizingMaskIntoConstraints = false
        coldVisualProbe.setAccessibilityElement(true)
        coldVisualProbe.setAccessibilityRole(.group)
        coldVisualProbe.setAccessibilityLabel("Cold-start visual configuration")
        coldVisualProbe.setAccessibilityIdentifier("seyal-cold-visual-probe")
        addSubview(coldVisualProbe)

        configureChromeButtons()
        tabStrip.translatesAutoresizingMaskIntoConstraints = false
        tabStrip.wantsLayer = true
        tabStrip.setAccessibilityElement(true)
        tabStrip.setAccessibilityRole(.group)
        tabStrip.setAccessibilityIdentifier("seyal-tab-strip")
        tabTitle.font = .systemFont(ofSize: 13, weight: .semibold)
        tabTitle.translatesAutoresizingMaskIntoConstraints = false
        tabStrip.addSubview(tabTitle)
        for button in [newTabButton, closeTabButton, splitRightButton, splitDownButton, closePaneButton] {
            button.translatesAutoresizingMaskIntoConstraints = false
            button.bezelStyle = .inline
            button.isBordered = false
            button.font = .systemFont(ofSize: 11, weight: .regular)
            tabStrip.addSubview(button)
        }

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

            coldVisualProbe.widthAnchor.constraint(equalToConstant: 1),
            coldVisualProbe.heightAnchor.constraint(equalToConstant: 1),
            coldVisualProbe.leadingAnchor.constraint(equalTo: leadingAnchor),
            coldVisualProbe.topAnchor.constraint(equalTo: topAnchor),

            tabStrip.leadingAnchor.constraint(equalTo: leadingAnchor),
            tabStrip.trailingAnchor.constraint(equalTo: trailingAnchor),
            tabStrip.topAnchor.constraint(equalTo: topAnchor),
            tabStrip.heightAnchor.constraint(equalToConstant: 48),
            tabTitle.leadingAnchor.constraint(equalTo: tabStrip.leadingAnchor, constant: 236),
            tabTitle.centerYAnchor.constraint(equalTo: tabStrip.centerYAnchor),

            newTabButton.leadingAnchor.constraint(equalTo: tabTitle.trailingAnchor, constant: 12),
            newTabButton.centerYAnchor.constraint(equalTo: tabStrip.centerYAnchor),
            closeTabButton.leadingAnchor.constraint(equalTo: newTabButton.trailingAnchor, constant: 8),
            closeTabButton.centerYAnchor.constraint(equalTo: tabStrip.centerYAnchor),
            closePaneButton.trailingAnchor.constraint(equalTo: tabStrip.trailingAnchor, constant: -12),
            closePaneButton.centerYAnchor.constraint(equalTo: tabStrip.centerYAnchor),
            splitDownButton.trailingAnchor.constraint(equalTo: closePaneButton.leadingAnchor, constant: -8),
            splitDownButton.centerYAnchor.constraint(equalTo: tabStrip.centerYAnchor),
            splitRightButton.trailingAnchor.constraint(equalTo: splitDownButton.leadingAnchor, constant: -8),
            splitRightButton.centerYAnchor.constraint(equalTo: tabStrip.centerYAnchor),

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

    @objc func transcriptDidScroll() {
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

    func performChromeReconcile() {
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

    func rebuildTabStrip(shell: SeyalAppShell) {
        if shell.tab_count > 0 {
            let row = seyal_app_shell_row(pane.appHandle, UInt16(SEYAL_APP_ROW_TAB), 0)
            tabTitle.stringValue = productChromeCopyUTF8(row.title, row.title_len) ?? "Terminal"
        }
        // Omit rather than disable (mirrors the command palette's own
        // omission of "New Tab"/"Split" when the M001 shell policy
        // disallows them; see build_commands).
        newTabButton.isHidden = shell.flags & UInt16(SEYAL_APP_SHELL_ALLOWS_TAB_CREATION) == 0
        splitRightButton.isHidden = shell.flags & UInt16(SEYAL_APP_SHELL_ALLOWS_PANE_SPLITTING) == 0
        splitDownButton.isHidden = splitRightButton.isHidden
        closeTabButton.isHidden = shell.flags & UInt16(SEYAL_APP_SHELL_ALLOWS_TAB_CLOSE) == 0
        closeTabButton.idLo = shell.active_tab_lo
        closeTabButton.idHi = shell.active_tab_hi
        closePaneButton.isHidden = shell.flags & UInt16(SEYAL_APP_SHELL_ALLOWS_PANE_CLOSE) == 0
        closePaneButton.idLo = shell.focused_pane_lo
        closePaneButton.idHi = shell.focused_pane_hi
    }

    func rebuildLeft(shell: SeyalAppShell, leftPanel: UInt16) {
        leftItems.arrangedSubviews.forEach { $0.removeFromSuperview() }
        if leftPanel == 0 {
            for index in 0..<Int(shell.workspace_count) {
                let row = seyal_app_shell_row(pane.appHandle, UInt16(SEYAL_APP_ROW_WORKSPACE), UInt32(index))
                leftItems.addArrangedSubview(
                    rowButton(
                        title: productChromeCopyUTF8(row.title, row.title_len) ?? "Workspace",
                        detail: productChromeCopyUTF8(row.detail, row.detail_len),
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
                        title: productChromeCopyUTF8(row.title, row.title_len) ?? "Tab",
                        detail: productChromeCopyUTF8(row.detail, row.detail_len),
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
                    title: productChromeCopyUTF8(row.title, row.title_len) ?? "Pane",
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

    func rebuildInspector(_ chrome: SeyalAppChrome) {
        inspector.arrangedSubviews.forEach { $0.removeFromSuperview() }
        attention.arrangedSubviews.forEach { $0.removeFromSuperview() }
        for index in 0..<Int(chrome.inspector_row_count) {
            let row = seyal_app_chrome_row(pane.appHandle, UInt16(SEYAL_APP_ROW_INSPECTOR), UInt32(index))
            let title = productChromeCopyUTF8(row.title, row.title_len) ?? ""
            let value = productChromeCopyUTF8(row.detail, row.detail_len) ?? ""
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
            let identity = productChromeCopyUTF8(row.title, row.title_len) ?? ""
            let title = productChromeCopyUTF8(row.detail, row.detail_len) ?? identity
            let button = borderlessButton(title: title, action: #selector(openAttention(_:)))
            button.setAccessibilityIdentifier("seyal-attention-\(index)")
            button.identifier = NSUserInterfaceItemIdentifier(identity)
            attention.addArrangedSubview(button)
        }
        for index in 0..<Int(chrome.agent_count) {
            let row = seyal_app_chrome_row(pane.appHandle, UInt16(SEYAL_APP_ROW_AGENT), UInt32(index))
            let identity = productChromeCopyUTF8(row.title, row.title_len) ?? ""
            let name = productChromeCopyUTF8(row.detail, row.detail_len) ?? identity
            let button = borderlessButton(title: name, action: #selector(selectAgent(_:)))
            button.setAccessibilityIdentifier("seyal-agent-\(index)")
            button.identifier = NSUserInterfaceItemIdentifier(identity)
            button.state = row.flags & UInt16(SEYAL_APP_ROW_SELECTED) != 0 ? .on : .off
            attention.addArrangedSubview(button)
        }
    }

    func applyShellChrome(_ chrome: SeyalAppChrome) {
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

    func applyTheme() {
        let theme = NativeThemeRealization.apply(
            to: self,
            material: material,
            appearance: effectiveAppearance
        )
        // Frosted utility chrome lets the window material show through; tonal /
        // opaque / reduced-material keep solid fills so content stays readable.
        if theme.usesFrostedUtilityMaterial {
            left.layer?.backgroundColor = NSColor.clear.cgColor
            inspectorColumn.layer?.backgroundColor = NSColor.clear.cgColor
            tabStrip.layer?.backgroundColor = NSColor.clear.cgColor
        } else {
            left.layer?.backgroundColor = theme.utility.cgColor
            inspectorColumn.layer?.backgroundColor = theme.utility.cgColor
            tabStrip.layer?.backgroundColor = theme.container.cgColor
        }
        centerColumn.layer?.backgroundColor = theme.canvas.cgColor
        transcript.backgroundColor = .clear
        left.layer?.borderWidth = 0
        tabTitle.font = .systemFont(ofSize: theme.uiFontSize, weight: .semibold)
        recoveryLabel.font = .systemFont(ofSize: max(theme.uiFontSize - 1, 10), weight: .regular)
        blocks.edgeInsets = NSEdgeInsets(
            top: theme.terminalPadding,
            left: theme.windowPadding,
            bottom: theme.terminalPadding,
            right: theme.windowPadding
        )
        updateColdVisualProbe(theme)
        composer.apply(theme: theme)
        historyOverlay.apply(theme: theme)
        commandPalette.apply(theme: theme)
        for view in blocks.arrangedSubviews {
            (view as? CommandBlockView)?.apply(theme: theme)
        }
    }

    func updateColdVisualProbe(_ theme: NativeTheme) {
        let appearanceToken = theme.appearance.bestMatch(from: [.aqua, .darkAqua]) == .aqua
            ? "light"
            : "dark"
        let materialToken = theme.usesFrostedUtilityMaterial ? "frosted" : "solid"
        // Machine probe lives only in the identifier; label stays human-readable.
        coldVisualProbe.setAccessibilityIdentifier(
            "seyal-cold-visual-probe.\(appearanceToken).\(Int(theme.uiFontSize)).\(Int(theme.terminalFontSize)).\(Int(theme.windowPadding)).\(Int(theme.terminalPadding)).\(materialToken)"
        )
        coldVisualProbe.setAccessibilityLabel(
            "Cold-start visual configuration: \(appearanceToken) appearance, UI font \(Int(theme.uiFontSize)), terminal font \(Int(theme.terminalFontSize)), window padding \(Int(theme.windowPadding)), terminal padding \(Int(theme.terminalPadding)), \(materialToken) utility material"
        )
    }
}
