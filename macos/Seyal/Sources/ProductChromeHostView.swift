import AppKit

/// Thin AppKit projection of Rust shell/chrome/composer/recovery. No writable product model.
@MainActor
final class ProductChromeHostView: NSView {
    let pane: ThinPaneHostView
    let material = NSVisualEffectView()
    /// Headed/component probe for cold visual snapshot. Identifier-encoded so
    /// VoiceOver does not hear a pipe-delimited token string as the chrome value.
    let coldVisualProbe = NSView()
    /// Outer Tab create/close stay with the left C03 mode row. Pane terminal
    /// tabs stay pane-local in `PaneLayoutView` (do not add a second strip).
    let newTabButton = NSButton(title: "+", target: nil, action: nil)
    let closeTabButton = IdentityButton(title: "×", target: nil, action: nil)
    let leftModeRow = NSStackView()
    let left = NSView()
    let inspector = NSStackView()
    let attention = NSStackView()
    let transcript = NSScrollView()
    let blocks = NSStackView()
    /// Leading filler so sparse Flow content hugs the bottom, just above the
    /// composer, instead of top-packing with a dead zone below it (#1426
    /// follow-up / Warp-like Block stacking). Height is driven explicitly by
    /// `syncTranscriptDocumentSize`, never by NSStackView distribution.
    let transcriptLeadingSpacer = NSView()
    var transcriptLeadingSpacerHeight: NSLayoutConstraint!
    var lastTranscriptViewportSize = CGSize.zero
    var hasPendingTranscriptViewportSync = false
    let composer: ComposerBridgeView
    let sessionBootstrap = SessionBootstrapBanner()
    /// Rust-owned history overlay (#933); internal for component tests.
    let historyOverlay: ComposerHistoryOverlayView
    /// Global command palette overlay (#932); internal for component tests.
    let commandPalette: CommandPaletteOverlayView
    let workspacesButton = NSButton(title: "Workspaces", target: nil, action: nil)
    let createWorkspaceButton = NSButton(title: "+", target: nil, action: nil)
    let tabsButton = NSButton(title: "Tabs", target: nil, action: nil)
    let recoveryLabel = NSTextField(labelWithString: "")
    /// #1379 Defect B: single clear recovery action after primary shell exit.
    let newShellButton = NSButton(title: "New shell", target: nil, action: nil)
    let leftItems = NSStackView()
    let inspectorColumn = NSView()
    /// Rust-projected Pane regions (#923); the live surface sits in
    /// `centerColumn.liveContent`.
    let centerColumn = PaneLayoutView()
    var liveContent: NSView { centerColumn.liveContent }
    var recoveryTimer: Timer?
    var recoveryTimerGeneration: UInt64 = 0
    var paneRecoveryTimer: Timer?
    var paneRecoveryTimerPin: PaneRecoveryPin?
    /// Rust recovery generation whose PerformAttempt is open on the lifecycle
    /// queue or still completing on MainActor (adopt + CompleteRecovery).
    /// Held across adopt so frame → reconcile → `driveRecovery` cannot start a
    /// second open while PerformAttempt remains the pending Rust effect.
    var recoveryAttemptInFlight: RecoveryAttemptIdentity?
    /// ADR-028: avoid re-presenting the same leave-offer path until Rust clears it.
    var presentedLeaveOfferPath: String?
    /// Blocking hello/attach runs here so the AppKit actor never becomes the
    /// lifecycle I/O executor.
    let recoveryLifecycleQueue = DispatchQueue(
        label: "com.seyal.runtime.lifecycle-recovery",
        qos: .userInitiated
    )
    var lastSnapshotGeneration: UInt64 = .max
    var lastEligibility: UInt16 = .max
    var lastProjectedExecution = (lo: UInt64(0), hi: UInt64(0))
    var lastBlockCount: Int = 0
    /// Follow live end only while the user was already pinned there.
    var followingLiveEnd = true
    /// Mid-history document-height floor so rebuilds cannot collapse scroll.
    var preservedDocumentHeightConstraint: NSLayoutConstraint?
    var isProgrammaticTranscriptScroll = false
    var isReconcilingChrome = false
    /// Nested product/timeline pulses during a rebuild must not drop TUI.
    var chromeNeedsReconcile = false
    var blockCards: [UInt64: CommandBlockView] = [:]
    var composerDockConstraints: [NSLayoutConstraint] = []
    var transcriptRelayBottomConstraint: NSLayoutConstraint?
    let transcriptTopSpacer = NSView(frame: .zero)
    var transcriptTopSpacerHeight: NSLayoutConstraint!
    var relayTarget: (
        blockID: UInt64,
        generation: UInt64,
        revision: UInt64,
        targetClass: UInt8,
        mode: UInt8
    ) = (0, 0, 0, 0, 0)
    var composerInlineBlockID: UInt64 = 0
    var transcriptFrameRevision: UInt64 = 0
    /// Bounds zero-clip async republish so layout stalls cannot spin forever (#1379).
    var blockOutputZeroClipRetries: Int = 0
    static let blockOutputZeroClipRetryLimit = 8
    var paneFollowsTranscript: [NSLayoutConstraint] = []
    var paneFillsCenter: [NSLayoutConstraint] = []
    var centerLeadingHost: NSLayoutConstraint!
    var centerTrailingHost: NSLayoutConstraint!
    var centerTopHost: NSLayoutConstraint!
    var centerLeadingLeft: NSLayoutConstraint!
    var centerTrailingInspector: NSLayoutConstraint!

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
        for button in [newTabButton, closeTabButton] {
            button.translatesAutoresizingMaskIntoConstraints = false
            button.bezelStyle = .inline
            button.isBordered = false
            button.font = .systemFont(ofSize: 11, weight: .regular)
            button.widthAnchor.constraint(equalToConstant: 20).isActive = true
            button.heightAnchor.constraint(equalToConstant: 22).isActive = true
        }

        left.translatesAutoresizingMaskIntoConstraints = false
        left.wantsLayer = true
        leftModeRow.orientation = .horizontal
        leftModeRow.alignment = .centerY
        // Fit the full Workspace/Terminal mode switcher and backed actions in
        // the 220 pt context rail without button compression or overlap.
        leftModeRow.spacing = 4
        leftModeRow.translatesAutoresizingMaskIntoConstraints = false
        let modeSpacer = NSView()
        modeSpacer.translatesAutoresizingMaskIntoConstraints = false
        modeSpacer.setContentHuggingPriority(.defaultLow, for: .horizontal)
        leftModeRow.addArrangedSubview(workspacesButton)
        leftModeRow.addArrangedSubview(createWorkspaceButton)
        // Pane tabs are visible in their own strips; advanced inventory
        // remains available from the command palette, not a second toggle.
        leftModeRow.addArrangedSubview(modeSpacer)
        leftModeRow.addArrangedSubview(newTabButton)
        leftModeRow.addArrangedSubview(closeTabButton)
        leftItems.orientation = .vertical
        leftItems.alignment = .width
        leftItems.distribution = .fill
        leftItems.spacing = 2
        leftItems.translatesAutoresizingMaskIntoConstraints = false
        // Keep the inventory column at panel width so C04 rows cannot hug
        // label intrinsic size and pin to the trailing seam (same trap as
        // Flow card NSTextField intrinsic width).
        leftItems.setHuggingPriority(.defaultHigh, for: .horizontal)
        left.addSubview(leftModeRow)
        left.addSubview(leftItems)

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
        newShellButton.translatesAutoresizingMaskIntoConstraints = false
        newShellButton.bezelStyle = .rounded
        newShellButton.font = .systemFont(ofSize: 13, weight: .medium)
        newShellButton.setAccessibilityIdentifier("seyal-new-shell")
        newShellButton.target = self
        newShellButton.action = #selector(requestNewShellAfterExit)
        newShellButton.isHidden = true

        inspectorColumn.translatesAutoresizingMaskIntoConstraints = false
        inspectorColumn.wantsLayer = true
        inspectorColumn.addSubview(inspector)
        inspectorColumn.addSubview(attention)

        blocks.orientation = .vertical
        blocks.alignment = .width
        blocks.spacing = 22
        blocks.edgeInsets = NSEdgeInsets(top: 8, left: 0, bottom: 8, right: 0)
        blocks.translatesAutoresizingMaskIntoConstraints = false
        setupTranscriptTopSpacer()
        expose(blocks, identifier: "seyal-blocks")
        configureTranscriptLeadingSpacer()
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
        liveContent.addSubview(transcript)
        liveContent.addSubview(pane)
        liveContent.addSubview(sessionBootstrap)
        liveContent.addSubview(composer)
        liveContent.addSubview(newShellButton)
        liveContent.addSubview(historyOverlay)
        centerColumn.onFocusPane = { [weak self] lo, hi in
            self?.focusPaneRegion(lo: lo, hi: hi)
        }
        centerColumn.onSelectTerminalTab = { [weak self] paneLo, paneHi, terminalLo, terminalHi in
            guard let self, self.focusPaneForPaneAction(paneLo: paneLo, paneHi: paneHi) else { return }
            self.selectTerminalTab(paneLo: paneLo, paneHi: paneHi, terminalLo: terminalLo, terminalHi: terminalHi)
        }
        centerColumn.onCloseTerminalTab = { [weak self] paneLo, paneHi, terminalLo, terminalHi in
            self?.closeTerminalTab(paneLo: paneLo, paneHi: paneHi, terminalLo: terminalLo, terminalHi: terminalHi)
        }
        centerColumn.onNewTerminal = { [weak self] paneLo, paneHi in
            guard let self, self.focusPaneForPaneAction(paneLo: paneLo, paneHi: paneHi) else { return }
            self.createTerminal()
        }
        centerColumn.onSplitPane = { [weak self] paneLo, paneHi, direction in
            guard let self, self.focusPaneForPaneAction(paneLo: paneLo, paneHi: paneHi) else { return }
            switch direction {
            case .right: self.splitFocusedRight()
            case .down: self.splitFocusedDown()
            }
        }
        centerColumn.onClosePane = { [weak self] paneLo, paneHi in
            self?.closePane(paneLo: paneLo, paneHi: paneHi)
        }
        centerColumn.appHandle = pane.appHandle

        addSubview(left)
        addSubview(centerColumn)
        addSubview(inspectorColumn)
        addSubview(recoveryLabel)
        recoveryLabel.alphaValue = 0
        recoveryLabel.setAccessibilityElement(true)

        // Topmost subview: the palette floats above every other region,
        // including the inspector, when Rust opens it.
        addSubview(commandPalette)
        setupComposerDockConstraints()
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
            self?.commandPaletteDidDismiss()
        }

        pane.setContentHuggingPriority(.defaultLow, for: .vertical)
        pane.setContentCompressionResistancePriority(.defaultLow, for: .vertical)
        composer.setContentHuggingPriority(.required, for: .vertical)
        // SessionBootstrapBanner owns its vertical hugging: required only while
        // offered, fittingSizeCompression while collapsed so it cannot steal
        // transcript/composer space or fight early liveContent height ≈ 1.
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

            left.leadingAnchor.constraint(equalTo: leadingAnchor),
            left.topAnchor.constraint(equalTo: topAnchor),
            left.bottomAnchor.constraint(equalTo: bottomAnchor),
            left.widthAnchor.constraint(equalToConstant: 220),
            leftModeRow.leadingAnchor.constraint(equalTo: left.leadingAnchor, constant: 12),
            leftModeRow.trailingAnchor.constraint(equalTo: left.trailingAnchor, constant: -12),
            leftModeRow.topAnchor.constraint(equalTo: left.topAnchor, constant: 12),
            // Rows bleed to the panel seam so C04 selection fill is edge-to-edge;
            // section labels keep the same 12 pt content inset via their own padding.
            leftItems.leadingAnchor.constraint(equalTo: left.leadingAnchor),
            leftItems.trailingAnchor.constraint(equalTo: left.trailingAnchor),
            leftItems.topAnchor.constraint(equalTo: leftModeRow.bottomAnchor, constant: 14),
            leftItems.bottomAnchor.constraint(lessThanOrEqualTo: left.bottomAnchor, constant: -12),

            inspectorColumn.trailingAnchor.constraint(equalTo: trailingAnchor),
            inspectorColumn.topAnchor.constraint(equalTo: topAnchor),
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
            transcript.leadingAnchor.constraint(equalTo: liveContent.leadingAnchor, constant: 20),
            transcript.trailingAnchor.constraint(equalTo: liveContent.trailingAnchor, constant: -20),
            transcript.topAnchor.constraint(equalTo: liveContent.topAnchor, constant: 16),
            transcript.heightAnchor.constraint(greaterThanOrEqualToConstant: 240),
            sessionBootstrap.leadingAnchor.constraint(equalTo: liveContent.leadingAnchor, constant: 24),
            sessionBootstrap.trailingAnchor.constraint(equalTo: liveContent.trailingAnchor, constant: -24),
            // When collapsed (height 0), this matches master: composer sits
            // 12pt below transcript. When the banner is visible it grows in
            // place without shifting Metal/PTY geometry for the idle path.
            sessionBootstrap.topAnchor.constraint(equalTo: transcript.bottomAnchor, constant: 12),
            composerDockConstraints[0], composerDockConstraints[1],
            composerDockConstraints[2], composerDockConstraints[3],
            newShellButton.centerXAnchor.constraint(equalTo: composer.centerXAnchor),
            newShellButton.centerYAnchor.constraint(equalTo: composer.centerYAnchor),
            newShellButton.widthAnchor.constraint(greaterThanOrEqualToConstant: 120),
            historyOverlay.leadingAnchor.constraint(equalTo: composer.leadingAnchor),
            historyOverlay.trailingAnchor.constraint(equalTo: composer.trailingAnchor),
            historyOverlay.bottomAnchor.constraint(equalTo: composer.topAnchor, constant: -8),
            blocks.topAnchor.constraint(equalTo: transcript.contentView.topAnchor),
            blocks.leadingAnchor.constraint(equalTo: transcript.contentView.leadingAnchor),
            blocks.widthAnchor.constraint(equalTo: transcript.contentView.widthAnchor),
        ])
        pinTranscriptToComposer()
        centerLeadingHost = centerColumn.leadingAnchor.constraint(equalTo: leadingAnchor)
        centerTrailingHost = centerColumn.trailingAnchor.constraint(equalTo: trailingAnchor)
        centerTopHost = centerColumn.topAnchor.constraint(equalTo: topAnchor)
        centerLeadingLeft = centerColumn.leadingAnchor.constraint(equalTo: left.trailingAnchor)
        centerTrailingInspector = centerColumn.trailingAnchor.constraint(
            equalTo: inspectorColumn.leadingAnchor
        )
        paneFollowsTranscript = [
            pane.leadingAnchor.constraint(equalTo: transcript.leadingAnchor),
            pane.trailingAnchor.constraint(equalTo: transcript.trailingAnchor),
            pane.topAnchor.constraint(equalTo: transcript.topAnchor),
            pane.bottomAnchor.constraint(equalTo: transcript.bottomAnchor),
        ]
        paneFillsCenter = [
            pane.leadingAnchor.constraint(equalTo: liveContent.leadingAnchor),
            pane.trailingAnchor.constraint(equalTo: liveContent.trailingAnchor),
            pane.topAnchor.constraint(equalTo: liveContent.topAnchor),
            pane.bottomAnchor.constraint(equalTo: liveContent.bottomAnchor),
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
            self?.requestComposerFocusIfPaletteAllows() ?? false
        }
        composer.onFocusEligibilityChanged = { [weak self] in
            guard let self, self.composer.isFocusEligibleForRecovery else { return }
            self.pane.inputSurface.composerFocusEligibilityDidChange()
            // Busy/DirectInput → Available (REPL exit): reclaim the dock like Warp.
            self.routeFocus()
        }
        pane.inputSurface.onRecoveryEffectsPending = { [weak self] in
            self?.reconcileChrome()
        }
        // Product shortcuts (⌘D split, pane focus, …) mutate Rust then
        // reconcile. Always re-route first responder: Flow→Flow split does
        // not change eligibility, so reconcile alone leaves the hidden
        // liveContent composer focused (#1464).
        composer.onCommandConsumed = { [weak self] in
            self?.reconcileChrome()
            self?.routeFocus()
        }
        pane.inputSurface.onCommandConsumed = { [weak self] in
            self?.reconcileChrome()
            self?.routeFocus()
        }
        centerColumn.onPaneCommandConsumed = { [weak self] in
            self?.reconcileChrome()
            self?.routeFocus()
        }
        composer.onHistoryOpened = { [weak self] in
            self?.reconcileChrome()
        }
        historyOverlay.onChanged = { [weak self] in
            self?.reconcileChrome()
        }
        historyOverlay.onDismissed = { [weak self] in
            self?.routeFocus()
        }
        installTimelineChangeHandler()
        pane.inputSurface.onHistoryRangeChanged = { [weak self] range in
            guard let self else { return }
            if self.pane.inputSurface.renderRawScrollbackHistoryIfNeeded(range) {
                return
            }
            self.applyHistoryRange(range)
        }
        pane.inputSurface.onComposerResultChanged = { [weak self] result in
            guard let self else { return }
            let accepted = result.code == .accepted || result.code == .unsupported
            // Multipane leaf composers own SUBMIT pending after Execute. Draining
            // only the legacy dock left pane hosts Busy with no Block AX after
            // optimistic accept was removed (#1474 / #1424).
            self.distributeComposerResult(
                requestID: result.requestID,
                accepted: accepted,
                failureCode: accepted ? nil : result.code
            )
            self.reconcileChrome()
        }
        installComposerStatusHandler()
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

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        applyTheme()
    }

    var inputSurface: InteractiveMetalSurfaceView { pane.inputSurface }

    func activateAfterWindowPresentation() {
        // The surface is the single BeginRecovery requester; its effects
        // reach `driveRecovery` through `onRecoveryEffectsPending`.
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
        // Focus, split, and attach reconcile while the primary PTY is idle.
        // Arm a newly registered Controller fd and relay eligibility already
        // cached for the focused attachment; composer mode stays Busy until
        // that fact is applied.
        pane.inputSurface.bridge?.armAuxiliaryReadSources()
        pane.inputSurface.bridge?.publishComposerStatus()
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
            applyComposerPlacement()
            sessionBootstrap.reconcile()
            historyOverlay.reconcile()
            commandPalette.reconcile()
            driveRecovery()
            let recoverySnap = seyal_app_snapshot(pane.appHandle)
            recoveryLabel.stringValue = recoveryText(recoverySnap)
            updateNewShellRecoveryAction()
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
        applyLeftModeSwitcher(leftPanel: chrome.left_panel)
        createWorkspaceButton.isHidden = chrome.left_panel != 0
        rebuildLeft(shell: shell, leftPanel: chrome.left_panel)
        presentLeaveOfferIfNeeded()
        rebuildInspector(chrome)
        rebuildLayoutActions(shell: shell, leftPanel: chrome.left_panel)
        reconcilePaneHosts(paneCount: Int(shell.pane_count))
        let direct = snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue)
            || snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue)
        if shell.pane_count > 1 {
            // Every visible Pane, including the focused one, has its own
            // durable presentation host. The legacy single-pane transcript
            // must not publish a second AX copy of the focused Pane's Blocks.
            clearLegacyTranscriptProjectionForMultipane()
        } else if !direct {
            rebuildBlocks()
        }
        applyTranscriptPresentation(snapshot)
        composer.reconcile()
        applyComposerPlacement()
        sessionBootstrap.reconcile()
        historyOverlay.reconcile()
        commandPalette.reconcile()
        driveRecovery()
        let recoverySnap = seyal_app_snapshot(pane.appHandle)
        recoveryLabel.stringValue = recoveryText(recoverySnap)
        updateNewShellRecoveryAction()
        if eligibilityChanged {
            routeFocus()
        }
        applyTheme()
        applyPresentationTierFromSnapshot()
    }

    /// Global keyboard-first command palette (#932): the menu action target.
    /// Opening is Rust-owned; a rejected open leaves focus untouched.
    @objc func openCommandPalette() {
        commandPalette.requestOpen()
    }


    /// R6.2.1: route Command through Rust before AppKit menu key-equivalents.
    /// Matched / cross-context projected steals consume; §6.2 step 2c miss and
    /// reserved §4.2 stay native (composer/palette ⌘←/⌘⌫ editing).
    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        guard flags.contains(.command) else {
            return super.performKeyEquivalent(with: event)
        }
        let responder = window?.firstResponder as? NSView
        let composerFocused = responder.map { $0 === composer || $0.isDescendant(of: composer) } ?? false
        // R8.4: report IME/marked-text the same way keyDown paths do so a
        // Command chord prefix cannot activate during composition.
        let compositionActive = Self.compositionActiveForKeyRouting(
            responder: responder,
            composer: composer,
            inputSurface: inputSurface
        )
        switch KeybindingStrokeNormalizer.route(
            appHandle: pane.appHandle,
            event: event,
            composerFocused: composerFocused,
            compositionActive: compositionActive
        ) {
        case .consumed:
            // Apply product changes as well as refreshing this Pane. Window
            // commands enqueue native realization effects in Rust; a direct
            // chrome refresh can leave the AppKit Window map stale.
            pane.onProductChanged?()
            routeFocus()
            return true
        case .nativeCommand:
            return super.performKeyEquivalent(with: event)
        case .fallsThrough:
            return super.performKeyEquivalent(with: event)
        }
    }

    /// Shared probe for R6.2.1 / R8.4: marked text on the focused input
    /// surface — composer, metal terminal, or palette/goto/history field
    /// editors (`NSTextView`). Package-visible for host tests.
    static func compositionActiveForKeyRouting(
        responder: NSView?,
        composer: ComposerBridgeView,
        inputSurface: InteractiveMetalSurfaceView
    ) -> Bool {
        guard let responder else { return false }
        if responder === composer || responder.isDescendant(of: composer) {
            return composer.hasMarkedComposition
        }
        if responder === inputSurface || responder.isDescendant(of: inputSurface) {
            return inputSurface.hasMarkedText()
        }
        if let metal = responder as? InteractiveMetalSurfaceView {
            return metal.hasMarkedText()
        }
        // Palette / Go to… / composer-history queries use NSTextField; AppKit
        // makes the field editor (NSTextView) first responder during IME.
        if let textView = responder as? NSTextView {
            return textView.hasMarkedText()
        }
        if let field = responder as? NSTextField,
           let editor = field.currentEditor() as? NSTextView
        {
            return editor.hasMarkedText()
        }
        return false
    }

    /// Navigation-only goto surface (SPEC-022 N4 / `goto.open`). Reuses the
    /// command-palette overlay; default scope is Panes.

    /// Navigation-only goto surface (SPEC-022 N4 / `goto.open`). Reuses the
    /// command-palette overlay; default scope is Panes.
    @objc func openGoto() {
        commandPalette.requestOpenGoto()
    }

    /// Flow first-responder target: the focused multipane leaf composer when
    /// pane hosts exist, otherwise the original single-pane composer (#1464).
    var activeComposer: ComposerBridgeView {
        let shell = seyal_app_shell(pane.appHandle)
        if !centerColumn.secondaryHosts.isEmpty,
           let host = centerColumn.presentationHost(
               paneLo: shell.focused_pane_lo,
               paneHi: shell.focused_pane_hi
           ),
           !host.composer.isHidden
        {
            return host.composer
        }
        if let owned = centerColumn.secondaryHosts.first(where: \.inputOwned),
           !owned.composer.isHidden
        {
            return owned.composer
        }
        return composer
    }

    /// Apply a Runtime ComposerResult to the Pane that owns the matching
    /// pending request. Multipane must not fan-out through focused fences —
    /// that stole focus via `prepareFocusedMutation` after the first sibling
    /// submit and dropped pane-local markers (#1474 / #1424).
    func distributeComposerResult(
        requestID: UInt64,
        accepted: Bool,
        failureCode: NativeComposerResult.Code?
    ) {
        if centerColumn.secondaryHosts.isEmpty {
            composer.applyComposerResult(requestID: requestID, accepted: accepted)
            if let failureCode {
                composer.presentAdmissionFailure(failureCode)
            }
            return
        }
        // Retire the legacy dock from AX while pane hosts own Flow so XCUI and
        // hit-testing cannot submit through a hidden competing `seyal-composer`.
        composer.retirePrimaryAccessibilityIdentity()
        let appHandle = pane.appHandle
        for host in centerColumn.secondaryHosts {
            let snap = seyal_app_composer_for_pane(appHandle, host.paneLo, host.paneHi)
            guard snap.request_id == requestID, requestID != 0 else { continue }
            let applied = seyal_app_apply_composer_result_for_pane(
                appHandle,
                host.paneLo,
                host.paneHi,
                requestID,
                accepted ? 1 : 0
            )
            guard applied == 0 else { continue }
            host.composer.reconcile()
            if let failureCode {
                host.composer.presentAdmissionFailure(failureCode)
            }
            return
        }
    }

    /// Restore single-pane dock AX when multipane hosts collapse.
    func restoreLegacyComposerAccessibilityIfNeeded() {
        guard centerColumn.secondaryHosts.isEmpty else {
            composer.retirePrimaryAccessibilityIdentity()
            return
        }
        composer.restorePrimaryAccessibilityIdentity()
    }

    /// Focused Pane Metal surface for Flow DirectInput / Raw / TUI keys.
    /// Multipane Raw/TUI covers the host with liveContent (#867 / #1474).
    func focusedInputSurface() -> InteractiveMetalSurfaceView {
        let shell = seyal_app_shell(pane.appHandle)
        if !centerColumn.secondaryHosts.isEmpty,
            centerColumn.liveContent.isHidden,
            let host = centerColumn.presentationHost(
                paneLo: shell.focused_pane_lo,
                paneHi: shell.focused_pane_hi
            )
        {
            return host.surface
        }
        return pane.inputSurface
    }

    /// ADR-009 #1466: Flow + Block-scoped DirectInput owns keys on Metal, not
    /// the Busy composer dock.
    func snapshotPublishesBlockScopedDirectInput(_ snapshot: SeyalAppSnapshot) -> Bool {
        snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_FLOW.rawValue)
            && snapshot.flags & UInt16(SEYAL_APP_SNAP_BLOCK_SCOPED_DIRECT_INPUT) != 0
    }

    /// Multipane: prefer the focused Pane's composer DirectInput flag / handle
    /// target over the shared ApplicationRoot snapshot (sibling Available can
    /// clear that shared target).
    func focusedPanePublishesBlockScopedDirectInput(_ snapshot: SeyalAppSnapshot) -> Bool {
        if snapshotPublishesBlockScopedDirectInput(snapshot) {
            return true
        }
        let shell = seyal_app_shell(pane.appHandle)
        let paneComposer = seyal_app_composer_for_pane(
            pane.appHandle,
            shell.focused_pane_lo,
            shell.focused_pane_hi
        )
        let eligibility = seyal_app_eligibility_for_pane(
            pane.appHandle,
            shell.focused_pane_lo,
            shell.focused_pane_hi
        )
        guard eligibility == UInt16(SEYAL_APP_ELIGIBILITY_FLOW.rawValue) else { return false }
        if let host = centerColumn.presentationHost(
            paneLo: shell.focused_pane_lo,
            paneHi: shell.focused_pane_hi
        ), host.displayHandle != 0 {
            return seyalPublishesBlockScopedDirectInput(
                flags: paneComposer.flags,
                target: seyal_bridge_composer_target_for_handle(host.displayHandle)
            )
        }
        return seyalPublishesBlockScopedDirectInput(
            flags: paneComposer.flags,
            target: seyal_bridge_composer_target()
        )
    }

    @discardableResult
    func focusBlockScopedDirectInputSurface() -> Bool {
        let surface = focusedInputSurface()
        let snapshot = seyal_app_snapshot(pane.appHandle)
        let direct = focusedPanePublishesBlockScopedDirectInput(snapshot)
        surface.panePublishesBlockScopedDirectInput = direct
        // Primary ApplicationRoot surface keeps its own bridge; secondary hosts
        // borrow in focusBlockScopedDirectInputIfEligible before routeFocus.
        surface.syncInputRoutePresentation()
        return window?.makeFirstResponder(surface) ?? false
    }

    func routeFocus() {
        // An open palette owns focus; eligibility-driven routing resumes
        // only after it closes (see `onDismissed`).
        guard !commandPalette.isOpen else { return }
        // Sync Metal IME/AX with Rust eligibility before choosing a responder
        // (#866 / #1468). DirectInput must keep Metal, not Busy composer.
        let snapshot = seyal_app_snapshot(pane.appHandle)
        let direct = focusedPanePublishesBlockScopedDirectInput(snapshot)
        // Primary ApplicationRoot surface only; secondary hosts publish via
        // SecondaryLivePaneHost.focusBlockScopedDirectInputIfEligible.
        pane.inputSurface.panePublishesBlockScopedDirectInput =
            direct && focusedInputSurface() === pane.inputSurface
        pane.inputSurface.syncInputRoutePresentation()
        if direct {
            _ = focusBlockScopedDirectInputSurface()
            return
        }
        let shell = seyal_app_shell(pane.appHandle)
        if !centerColumn.secondaryHosts.isEmpty {
            let eligibility = seyal_app_eligibility_for_pane(
                pane.appHandle,
                shell.focused_pane_lo,
                shell.focused_pane_hi
            )
            if eligibility == UInt16(SEYAL_APP_ELIGIBILITY_FLOW.rawValue) {
                let paneComposer = seyal_app_composer_for_pane(
                    pane.appHandle,
                    shell.focused_pane_lo,
                    shell.focused_pane_hi
                )
                if paneComposer.mode != UInt16(SEYAL_APP_COMPOSER_HIDDEN.rawValue) {
                    activeComposer.focusEditor()
                }
                return
            }
        }
        let composerSnap = seyal_app_composer(pane.appHandle)
        if snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_FLOW.rawValue),
           composerSnap.mode != UInt16(SEYAL_APP_COMPOSER_HIDDEN.rawValue)
        {
            // Multipane Flow has pane-owned editors above this hidden legacy
            // liveContent. If its focused host is not ready, never direct
            // keyboard input to the invisible root composer.
            if centerColumn.secondaryHosts.isEmpty {
                composer.focusEditor()
            }
        } else if snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue)
            || snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue)
        {
            window?.makeFirstResponder(focusedInputSurface())
        }
    }

    func rebuildLayoutActions(shell: SeyalAppShell, leftPanel: UInt16) {
        // Workspaces mode: + creates an outer Workspace Tab. Tabs mode: + creates
        // a pane-local TerminalTab on the focused Pane (#1412 / ADR-025 §2.8).
        if leftPanel == 1 {
            newTabButton.isHidden = false
            newTabButton.action = #selector(createTerminal)
            newTabButton.setAccessibilityLabel("New Terminal")
            newTabButton.toolTip = "New Terminal"
            closeTabButton.isHidden = true
        } else {
            newTabButton.isHidden = shell.flags & UInt16(SEYAL_APP_SHELL_ALLOWS_TAB_CREATION) == 0
            newTabButton.action = #selector(createTab)
            newTabButton.setAccessibilityLabel("New Layout")
            newTabButton.toolTip = "New Layout"
            closeTabButton.isHidden = shell.flags & UInt16(SEYAL_APP_SHELL_ALLOWS_TAB_CLOSE) == 0
        }
        configureCloseButtons(shell: shell)
    }

    func rebuildLeft(shell: SeyalAppShell, leftPanel: UInt16) {
        leftItems.arrangedSubviews.forEach { $0.removeFromSuperview() }
        if leftPanel == 0 {
            appendLeftSection(
                title: "WORKSPACES",
                identifier: "seyal-workspaces-heading",
                topGap: false
            ) {
                for index in 0..<Int(shell.workspace_count) {
                    let row = seyal_app_shell_row(pane.appHandle, UInt16(SEYAL_APP_ROW_WORKSPACE), UInt32(index))
                    let title = productChromeCopyUTF8(row.title, row.title_len) ?? "Workspace"
                    let path = productChromeCopyUTF8(row.detail, row.detail_len)
                    let selected = row.flags & UInt16(SEYAL_APP_ROW_SELECTED) != 0
                    // C04 secondary: helpful path; on the active Workspace also
                    // keep outer Tab count when a root is bound (#1413 + #1412).
                    let pathSecondary = Self.helpfulWorkspaceDetail(title: title, detail: path)
                    let tabSecondary = selected
                        ? "\(shell.tab_count) tab\(shell.tab_count == 1 ? "" : "s")"
                        : nil
                    let secondary: String?
                    if let pathSecondary, let tabSecondary {
                        secondary = "\(pathSecondary) · \(tabSecondary)"
                    } else {
                        secondary = pathSecondary ?? tabSecondary
                    }
                    leftItems.addArrangedSubview(
                        contextRow(
                            primary: title,
                            secondary: secondary,
                            identifier: "seyal-workspace-\(index)",
                            selected: selected,
                            action: #selector(selectWorkspace(_:)),
                            kind: UInt16(SEYAL_APP_ACTION_SELECT_WORKSPACE.rawValue),
                            idLo: row.id_lo,
                            idHi: row.id_hi
                        )
                    )
                }
            }
            appendWorkspaceLayoutsIfNeeded(shell: shell)
            let chrome = seyal_app_chrome(pane.appHandle)
            if chrome.agent_count > 0 {
                appendLeftSection(
                    title: "AGENTS",
                    identifier: "seyal-agents-heading",
                    topGap: true
                ) {
                    for index in 0..<Int(chrome.agent_count) {
                        let row = seyal_app_chrome_row(
                            pane.appHandle,
                            UInt16(SEYAL_APP_ROW_AGENT),
                            UInt32(index)
                        )
                        let identity = productChromeCopyUTF8(row.title, row.title_len) ?? ""
                        let name = productChromeCopyUTF8(row.detail, row.detail_len) ?? identity
                        let button = borderlessButton(title: name, action: #selector(selectAgent(_:)))
                        button.identifier = NSUserInterfaceItemIdentifier(identity)
                        button.setAccessibilityIdentifier("seyal-agent-\(index)")
                        leftItems.addArrangedSubview(button)
                    }
                }
            }
        } else {
            // Pane-local TerminalTab navigation map (Rust-authored titles/order).
            appendLeftSection(
                title: "TABS",
                identifier: "seyal-tabs-heading",
                topGap: false
            ) {
                let terminalCount = leftTerminalTabRowCount(shell: shell)
                for index in 0..<terminalCount {
                    let row = seyal_app_shell_row(
                        pane.appHandle,
                        UInt16(SEYAL_APP_ROW_TERMINAL_TAB),
                        UInt32(index)
                    )
                    guard row.title_len > 0 || row.id_lo != 0 || row.id_hi != 0 else { break }
                    let (paneLo, paneHi) = paneIdFromTerminalTabRow(row)
                    let title = productChromeCopyUTF8(row.title, row.title_len) ?? "Terminal"
                    let detail = productChromeCopyUTF8(row.detail, row.detail_len)
                    leftItems.addArrangedSubview(
                        contextRow(
                            primary: title,
                            secondary: Self.helpfulWorkspaceDetail(title: title, detail: detail),
                            identifier: "seyal-terminal-tab-left-\(index)",
                            selected: row.flags & UInt16(SEYAL_APP_ROW_SELECTED) != 0,
                            action: #selector(selectLeftTerminalTab(_:)),
                            kind: UInt16(SEYAL_APP_ACTION_SELECT_TERMINAL_TAB.rawValue),
                            idLo: row.id_lo,
                            idHi: row.id_hi,
                            paneLo: paneLo,
                            paneHi: paneHi
                        )
                    )
                }
            }
        }
        // Pane focus stays pane-local. C03 must not dump a Pane tree into the left rail.
    }

    /// C04 secondary path only when it adds information beyond the primary name.
    static func helpfulWorkspaceDetail(title: String, detail: String?) -> String? {
        guard let detail else { return nil }
        let trimmed = detail.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return nil }
        if trimmed.caseInsensitiveCompare(title) == .orderedSame {
            return nil
        }
        return trimmed
    }

    func appendLeftSection(
        title: String,
        identifier: String,
        topGap: Bool,
        populate: () -> Void
    ) {
        if topGap {
            leftItems.addArrangedSubview(leftSectionSpacer(height: 14))
        }
        leftItems.addArrangedSubview(sectionHeader(title, identifier: identifier))
        leftItems.setCustomSpacing(8, after: leftItems.arrangedSubviews.last!)
        populate()
    }

    func leftSectionSpacer(height: CGFloat) -> NSView {
        let spacer = NSView()
        spacer.translatesAutoresizingMaskIntoConstraints = false
        spacer.heightAnchor.constraint(equalToConstant: height).isActive = true
        spacer.setContentHuggingPriority(.required, for: .vertical)
        return spacer
    }

    func sectionHeader(_ title: String, identifier: String) -> NSView {
        let wrap = NSView()
        wrap.translatesAutoresizingMaskIntoConstraints = false
        wrap.setContentHuggingPriority(.defaultLow, for: .horizontal)
        let header = NSTextField(labelWithString: title)
        header.font = .systemFont(ofSize: 10, weight: .semibold)
        header.textColor = .secondaryLabelColor
        header.tag = 2
        header.setAccessibilityIdentifier(identifier)
        header.setAccessibilityElement(true)
        header.translatesAutoresizingMaskIntoConstraints = false
        header.setContentHuggingPriority(.defaultLow, for: .horizontal)
        wrap.addSubview(header)
        NSLayoutConstraint.activate([
            header.leadingAnchor.constraint(equalTo: wrap.leadingAnchor, constant: 12),
            header.trailingAnchor.constraint(lessThanOrEqualTo: wrap.trailingAnchor, constant: -12),
            header.topAnchor.constraint(equalTo: wrap.topAnchor),
            header.bottomAnchor.constraint(equalTo: wrap.bottomAnchor),
        ])
        return wrap
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
        left.isHidden = !leftOn
        left.setAccessibilityElement(leftOn)
        inspectorColumn.isHidden = !inspectorOn
        inspectorColumn.setAccessibilityElement(inspectorOn)
        inspector.setAccessibilityElement(inspectorOn)
        centerLeadingHost.isActive = !leftOn
        centerLeadingLeft.isActive = leftOn
        centerTrailingHost.isActive = !inspectorOn
        centerTrailingInspector.isActive = inspectorOn
        centerTopHost.isActive = true
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
        } else {
            left.layer?.backgroundColor = theme.utility.cgColor
            inspectorColumn.layer?.backgroundColor = theme.utility.cgColor
        }
        centerColumn.layer?.backgroundColor = theme.canvas.cgColor
        centerColumn.apply(theme: theme)
        transcript.backgroundColor = .clear
        left.layer?.borderWidth = 0
        recoveryLabel.font = .systemFont(ofSize: max(theme.uiFontSize - 1, 10), weight: .regular)
        blocks.edgeInsets = NSEdgeInsets(
            top: theme.terminalPadding,
            left: theme.windowPadding,
            bottom: theme.terminalPadding,
            right: theme.windowPadding
        )
        updateColdVisualProbe(theme)
        composer.apply(theme: theme)
        sessionBootstrap.apply(theme: theme)
        historyOverlay.apply(theme: theme)
        commandPalette.apply(theme: theme)
        for view in blocks.arrangedSubviews {
            (view as? CommandBlockView)?.apply(theme: theme)
        }
        for host in centerColumn.secondaryHosts {
            host.apply(theme: theme)
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
