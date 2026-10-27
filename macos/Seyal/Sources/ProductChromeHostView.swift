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
    /// Adaptive Depth C05: typography-led tabs with underline for the active Tab.
    /// Chips live in `tabScroll`; `+` / layout controls stay outside (VF-4).
    let tabScroll = NSScrollView()
    let tabScrollContent = NSView()
    let tabButtons = NSStackView()
    let tabUnderline = NSView()
    let tabTrailingCluster = NSStackView()
    var tabUnderlineLeading: NSLayoutConstraint?
    var tabUnderlineTrailing: NSLayoutConstraint?
    var selectedTabChip: TabChipView?
    private var isCompressingTabs = false
    let leftSeam = ChromeRegionSeamView()
    let inspectorSeam = ChromeRegionSeamView()
    /// Layout-chrome cluster (#922): apply to the active Tab/focused Pane,
    /// never duplicated per-Pane (M001-CORE-TERMINAL-REFERENCE-SCREEN.md §4.3).
    let newTabButton = NSButton(title: "+", target: nil, action: nil)
    let closeTabButton = IdentityButton(title: "Close Tab", target: nil, action: nil)
    let splitRightButton = NSButton(title: "Split Right", target: nil, action: nil)
    let splitDownButton = NSButton(title: "Split Down", target: nil, action: nil)
    let closePaneButton = IdentityButton(title: "Close Pane", target: nil, action: nil)
    /// In-Tab Pane header (M001 §4 / cmux-like) for the focused live leaf.
    let focusedPaneChrome = PaneChromeBarView()
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
        tabTitle.isHidden = true
        tabButtons.orientation = .horizontal
        tabButtons.alignment = .centerY
        tabButtons.spacing = 4
        tabButtons.translatesAutoresizingMaskIntoConstraints = false
        tabButtons.setAccessibilityElement(false)
        tabButtons.setHuggingPriority(.required, for: .horizontal)
        tabUnderline.translatesAutoresizingMaskIntoConstraints = false
        tabUnderline.wantsLayer = true
        tabUnderline.isHidden = true
        tabScrollContent.translatesAutoresizingMaskIntoConstraints = false
        tabScrollContent.addSubview(tabButtons)
        tabScrollContent.addSubview(tabUnderline)
        tabScroll.translatesAutoresizingMaskIntoConstraints = false
        tabScroll.drawsBackground = false
        tabScroll.borderType = .noBorder
        tabScroll.hasVerticalScroller = false
        tabScroll.hasHorizontalScroller = true
        tabScroll.autohidesScrollers = true
        tabScroll.horizontalScrollElasticity = .allowed
        tabScroll.verticalScrollElasticity = .none
        tabScroll.documentView = tabScrollContent
        tabScroll.setAccessibilityElement(false)
        tabScroll.setAccessibilityIdentifier("seyal-tab-strip-scroll")
        // Pin the document to the clip view so horizontal overflow scrolls (VF-4).
        NSLayoutConstraint.activate([
            tabScrollContent.leadingAnchor.constraint(equalTo: tabScroll.contentView.leadingAnchor),
            tabScrollContent.topAnchor.constraint(equalTo: tabScroll.contentView.topAnchor),
            tabScrollContent.heightAnchor.constraint(equalTo: tabScroll.contentView.heightAnchor),
        ])
        tabTrailingCluster.orientation = .horizontal
        tabTrailingCluster.alignment = .centerY
        tabTrailingCluster.spacing = 8
        tabTrailingCluster.translatesAutoresizingMaskIntoConstraints = false
        tabTrailingCluster.setAccessibilityElement(false)
        tabStrip.addSubview(tabScroll)
        tabStrip.addSubview(tabTrailingCluster)
        tabStrip.addSubview(tabTitle)
        // Global closeTabButton is demoted: Close lives on each C05 chip.
        closeTabButton.isHidden = true
        closeTabButton.setAccessibilityElement(false)
        for button in [newTabButton, splitRightButton, splitDownButton, closePaneButton] {
            button.translatesAutoresizingMaskIntoConstraints = false
            button.bezelStyle = .inline
            button.isBordered = false
            button.font = .systemFont(ofSize: 11, weight: .regular)
            tabTrailingCluster.addArrangedSubview(button)
        }

        left.translatesAutoresizingMaskIntoConstraints = false
        left.wantsLayer = true
        left.addSubview(leftSeam)
        leftItems.orientation = .vertical
        leftItems.alignment = .width
        leftItems.spacing = 2
        leftItems.translatesAutoresizingMaskIntoConstraints = false
        left.addSubview(workspacesButton)
        left.addSubview(tabsButton)
        left.addSubview(leftItems)
        workspacesButton.translatesAutoresizingMaskIntoConstraints = false
        tabsButton.translatesAutoresizingMaskIntoConstraints = false

        inspector.orientation = .vertical
        inspector.alignment = .width
        inspector.spacing = 4
        inspector.translatesAutoresizingMaskIntoConstraints = false
        expose(inspector, identifier: "seyal-inspector")
        attention.orientation = .vertical
        attention.alignment = .leading
        attention.translatesAutoresizingMaskIntoConstraints = false
        expose(attention, identifier: "seyal-attention")
        recoveryLabel.font = .systemFont(ofSize: 11, weight: .regular)
        recoveryLabel.tag = 2
        recoveryLabel.setAccessibilityElement(true)
        recoveryLabel.setAccessibilityIdentifier("seyal-recovery")
        recoveryLabel.stringValue = "disconnected"
        recoveryLabel.translatesAutoresizingMaskIntoConstraints = false

        inspectorColumn.translatesAutoresizingMaskIntoConstraints = false
        inspectorColumn.wantsLayer = true
        inspectorColumn.addSubview(inspectorSeam)
        inspectorColumn.addSubview(inspector)
        inspectorColumn.addSubview(attention)

        blocks.orientation = .vertical
        blocks.alignment = .width
        // M003-BLOCK-COMPONENT-DESIGN §3: block.gap; Blocks inset from the Pane.
        blocks.spacing = CommandBlockView.ChromeMetrics.blockGap
        blocks.edgeInsets = NSEdgeInsets(top: 12, left: 12, bottom: 12, right: 12)
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
        focusedPaneChrome.translatesAutoresizingMaskIntoConstraints = false
        focusedPaneChrome.onClose = { [weak self] in
            guard let self else { return }
            self.closeFocusedPane(self.closePaneButton)
        }
        // Transcript/Metal/composer first; pane chrome last so × stays hittable.
        liveContent.addSubview(transcript)
        liveContent.addSubview(pane)
        liveContent.addSubview(composer)
        liveContent.addSubview(historyOverlay)
        liveContent.addSubview(focusedPaneChrome)
        centerColumn.onFocusPane = { [weak self] lo, hi in
            self?.focusPaneRegion(lo: lo, hi: hi)
        }
        centerColumn.onClosePane = { [weak self] lo, hi in
            self?.closePaneIdentity(lo: lo, hi: hi)
        }
        centerColumn.appHandle = pane.appHandle

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
            self?.commandPaletteDidDismiss()
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

            // C05 Tab strip over center only; pane surface strips sit directly under it.
            tabStrip.leadingAnchor.constraint(equalTo: centerColumn.leadingAnchor),
            tabStrip.trailingAnchor.constraint(equalTo: centerColumn.trailingAnchor),
            tabStrip.topAnchor.constraint(equalTo: topAnchor),
            tabStrip.heightAnchor.constraint(equalToConstant: 40),
            // Scrollable chip row; trailing + / layout cluster stays fixed (VF-4).
            tabScroll.leadingAnchor.constraint(equalTo: tabStrip.leadingAnchor, constant: 12),
            tabScroll.trailingAnchor.constraint(
                equalTo: tabTrailingCluster.leadingAnchor,
                constant: -12
            ),
            tabScroll.centerYAnchor.constraint(equalTo: tabStrip.centerYAnchor),
            tabScroll.heightAnchor.constraint(equalToConstant: 30),
            tabButtons.leadingAnchor.constraint(equalTo: tabScrollContent.leadingAnchor),
            tabButtons.trailingAnchor.constraint(equalTo: tabScrollContent.trailingAnchor),
            tabButtons.topAnchor.constraint(equalTo: tabScrollContent.topAnchor),
            tabButtons.heightAnchor.constraint(equalToConstant: 28),
            tabScrollContent.heightAnchor.constraint(equalToConstant: 30),
            tabUnderline.heightAnchor.constraint(equalToConstant: 2),
            tabUnderline.bottomAnchor.constraint(equalTo: tabScrollContent.bottomAnchor),
            tabTrailingCluster.trailingAnchor.constraint(
                equalTo: tabStrip.trailingAnchor,
                constant: -12
            ),
            tabTrailingCluster.centerYAnchor.constraint(equalTo: tabStrip.centerYAnchor),
            tabTrailingCluster.heightAnchor.constraint(equalToConstant: 28),

            left.leadingAnchor.constraint(equalTo: leadingAnchor),
            left.topAnchor.constraint(equalTo: topAnchor),
            left.bottomAnchor.constraint(equalTo: bottomAnchor),
            left.widthAnchor.constraint(equalToConstant: 220),

            leftSeam.trailingAnchor.constraint(equalTo: left.trailingAnchor),
            leftSeam.topAnchor.constraint(equalTo: left.topAnchor),
            leftSeam.bottomAnchor.constraint(equalTo: left.bottomAnchor),
            workspacesButton.leadingAnchor.constraint(equalTo: left.leadingAnchor, constant: 10),
            workspacesButton.topAnchor.constraint(equalTo: left.topAnchor, constant: 10),
            tabsButton.leadingAnchor.constraint(equalTo: workspacesButton.trailingAnchor, constant: 8),
            tabsButton.centerYAnchor.constraint(equalTo: workspacesButton.centerYAnchor),
            leftItems.leadingAnchor.constraint(equalTo: left.leadingAnchor, constant: 10),
            leftItems.trailingAnchor.constraint(equalTo: left.trailingAnchor, constant: -10),
            leftItems.topAnchor.constraint(equalTo: workspacesButton.bottomAnchor, constant: 12),
            leftItems.bottomAnchor.constraint(lessThanOrEqualTo: left.bottomAnchor, constant: -10),

            inspectorColumn.trailingAnchor.constraint(equalTo: trailingAnchor),
            inspectorColumn.topAnchor.constraint(equalTo: topAnchor),
            inspectorColumn.bottomAnchor.constraint(equalTo: bottomAnchor),
            inspectorColumn.widthAnchor.constraint(equalToConstant: 248),

            inspectorSeam.leadingAnchor.constraint(equalTo: inspectorColumn.leadingAnchor),
            inspectorSeam.topAnchor.constraint(equalTo: inspectorColumn.topAnchor),
            inspectorSeam.bottomAnchor.constraint(equalTo: inspectorColumn.bottomAnchor),
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
            focusedPaneChrome.leadingAnchor.constraint(equalTo: liveContent.leadingAnchor),
            focusedPaneChrome.trailingAnchor.constraint(equalTo: liveContent.trailingAnchor),
            focusedPaneChrome.topAnchor.constraint(equalTo: liveContent.topAnchor),
            transcript.leadingAnchor.constraint(equalTo: liveContent.leadingAnchor, constant: 20),
            transcript.trailingAnchor.constraint(equalTo: liveContent.trailingAnchor, constant: -20),
            transcript.topAnchor.constraint(equalTo: focusedPaneChrome.bottomAnchor, constant: 8),
            transcript.heightAnchor.constraint(greaterThanOrEqualToConstant: 240),
            composer.leadingAnchor.constraint(equalTo: liveContent.leadingAnchor, constant: 24),
            composer.trailingAnchor.constraint(equalTo: liveContent.trailingAnchor, constant: -24),
            composer.topAnchor.constraint(equalTo: transcript.bottomAnchor, constant: 12),
            composer.bottomAnchor.constraint(equalTo: liveContent.bottomAnchor, constant: -16),
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
        // Raw/TUI Metal fills the Pane below cmux-like chrome, never over it.
        paneFillsCenter = [
            pane.leadingAnchor.constraint(equalTo: liveContent.leadingAnchor),
            pane.trailingAnchor.constraint(equalTo: liveContent.trailingAnchor),
            pane.topAnchor.constraint(equalTo: focusedPaneChrome.bottomAnchor),
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
        }
        pane.inputSurface.onRecoveryEffectsPending = { [weak self] in
            self?.reconcileChrome()
        }
        composer.onCommandConsumed = { [weak self] in
            self?.reconcileChrome()
        }
        pane.inputSurface.onCommandConsumed = { [weak self] in
            self?.reconcileChrome()
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
        pane.inputSurface.onHistoryCopy = { [weak self] _, text in
            self?.writePasteboard(text)
        }
        pane.inputSurface.onHistoryRangeChanged = { [weak self] range in
            self?.applyHistoryRange(range)
        }
        pane.inputSurface.onComposerResultChanged = { [weak self] result in
            let accepted = result.code == .accepted || result.code == .unsupported
            self?.composer.applyComposerResult(requestID: result.requestID, accepted: accepted)
            if !accepted {
                self?.composer.presentAdmissionFailure(result.code)
            }
            self?.reconcileChrome()
        }
        pane.inputSurface.onComposerStatusChanged = { [weak self] status in
            guard let self else { return false }
            let accepted = self.relayComposerStatus(status)
            if accepted {
                self.reconcileChrome()
            }
            return accepted
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
            centerColumn.reconcileSecondaryComposers()
            updateFocusedPaneChrome(shell: seyal_app_shell(pane.appHandle))
            historyOverlay.reconcile()
            commandPalette.reconcile()
            driveRecovery()
            recoveryLabel.stringValue = recoveryText(seyal_app_snapshot(pane.appHandle))
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
        reconcilePaneHosts(paneCount: Int(shell.pane_count))
        updateFocusedPaneChrome(shell: shell)
        let direct = snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue)
            || snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue)
        if !direct {
            rebuildBlocks()
        }
        applyTranscriptPresentation(snapshot)
        composer.reconcile()
        centerColumn.reconcileSecondaryComposers()
        historyOverlay.reconcile()
        commandPalette.reconcile()
        driveRecovery()
        recoveryLabel.stringValue = recoveryText(seyal_app_snapshot(pane.appHandle))
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
            reconcileChrome()
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

    func routeFocus() {
        // An open palette owns focus; eligibility-driven routing resumes
        // only after it closes (see `onDismissed`).
        guard !commandPalette.isOpen else { return }
        // Revoke Metal IME/AX before focusing the composer so Flow never keeps
        // a competing terminal first responder (#866).
        inputSurface.syncInputRoutePresentation()
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

    func updateFocusedPaneChrome(shell: SeyalAppShell) {
        let focused = centerColumn.regions.first(where: \.focused)
        let title = focused?.title.isEmpty == false ? focused!.title : "Pane"
        let theme = NativeThemeRealization.theme(for: effectiveAppearance)
        let allowsClose = shell.flags & UInt16(SEYAL_APP_SHELL_ALLOWS_PANE_CLOSE) != 0
        focusedPaneChrome.apply(title: title, theme: theme, focused: true)
        focusedPaneChrome.setCloseVisible(allowsClose)
        // Non-live split leaves keep B.1 surface chrome in the region frame.
        centerColumn.updatePlaceholderChrome(allowsClose: allowsClose, theme: theme)
        // Per-pane × is the primary close; strip Close Pane stays as a compact fallback.
        for host in centerColumn.secondaryHosts {
            let region = centerColumn.regions.first {
                $0.paneLo == host.paneLo && $0.paneHi == host.paneHi
            }
            host.updateChrome(
                title: region?.title.isEmpty == false ? region!.title : "Pane",
                allowsClose: allowsClose,
                theme: theme,
                focused: false
            )
            host.onClosePane = { [weak self] in
                self?.closePaneIdentity(lo: host.paneLo, hi: host.paneHi)
            }
        }
    }

    func rebuildTabStrip(shell: SeyalAppShell) {
        tabButtons.arrangedSubviews.forEach { $0.removeFromSuperview() }
        tabUnderline.isHidden = true
        selectedTabChip = nil
        // ADR-018 hierarchical close: sole Tab × removes the Window. Keep the
        // chip close visible whenever Rust admits CloseTab.
        let showClose = shell.flags & UInt16(SEYAL_APP_SHELL_ALLOWS_TAB_CLOSE) != 0
        let theme = NativeThemeRealization.theme(for: effectiveAppearance)
        for index in 0..<Int(shell.tab_count) {
            let row = seyal_app_shell_row(pane.appHandle, UInt16(SEYAL_APP_ROW_TAB), UInt32(index))
            let title = productChromeCopyUTF8(row.title, row.title_len) ?? "Tab"
            let selected = row.flags & UInt16(SEYAL_APP_ROW_SELECTED) != 0
            let chip = TabChipView(
                title: title,
                selected: selected,
                showClose: showClose,
                tabIndex: index,
                idLo: row.id_lo,
                idHi: row.id_hi,
                containmentGeneration: shell.containment_generation,
                target: self,
                selectAction: #selector(selectTab(_:)),
                closeAction: #selector(closeActiveTab(_:))
            )
            chip.apply(theme: theme)
            tabButtons.addArrangedSubview(chip)
            if selected {
                selectedTabChip = chip
                // Test/host still probe closeTabButton identity for fencing.
                closeTabButton.idLo = row.id_lo
                closeTabButton.idHi = row.id_hi
                closeTabButton.containmentGeneration = shell.containment_generation
            }
        }
        tabUnderlineLeading?.isActive = false
        tabUnderlineTrailing?.isActive = false
        tabUnderlineLeading = nil
        tabUnderlineTrailing = nil
        if let selectedChip = selectedTabChip {
            tabUnderline.isHidden = false
            let leading = tabUnderline.leadingAnchor.constraint(equalTo: selectedChip.leadingAnchor)
            let trailing = tabUnderline.trailingAnchor.constraint(equalTo: selectedChip.trailingAnchor)
            tabUnderlineLeading = leading
            tabUnderlineTrailing = trailing
            NSLayoutConstraint.activate([leading, trailing])
        }
        // Omit rather than disable (mirrors the command palette's own
        // omission of "New Tab"/"Split" when the M001 shell policy
        // disallows them; see build_commands).
        newTabButton.isHidden = shell.flags & UInt16(SEYAL_APP_SHELL_ALLOWS_TAB_CREATION) == 0
        splitRightButton.isHidden = shell.flags & UInt16(SEYAL_APP_SHELL_ALLOWS_PANE_SPLITTING) == 0
        splitDownButton.isHidden = splitRightButton.isHidden
        closeTabButton.isHidden = true
        closePaneButton.isHidden = shell.flags & UInt16(SEYAL_APP_SHELL_ALLOWS_PANE_CLOSE) == 0
        configureCloseButtons(shell: shell)
        compressTabChips(theme: theme)
        revealSelectedTabChip()
    }

    /// VF-4: compress chips inside `tab_min_width`…`tab_max_width`; leftover width scrolls.
    @discardableResult
    func compressTabChips(theme: NativeTheme) -> Bool {
        let chips = tabButtons.arrangedSubviews.compactMap { $0 as? TabChipView }
        guard !chips.isEmpty else { return false }
        let minW = theme.tabMinWidth
        let maxW = theme.tabMaxWidth
        let gaps = tabButtons.spacing * CGFloat(max(chips.count - 1, 0))
        let preferred = chips.map {
            $0.preferredWidth(minWidth: minW, maxWidth: maxW, fontSize: theme.uiFontSize)
        }
        let preferredSum = preferred.reduce(0, +) + gaps
        let available = max(tabScroll.contentView.bounds.width, tabScroll.bounds.width)
        let widths: [CGFloat]
        if available <= 1 || preferredSum <= available {
            widths = preferred
        } else {
            let minSum = minW * CGFloat(chips.count) + gaps
            if available <= minSum {
                widths = Array(repeating: minW, count: chips.count)
            } else {
                let budget = available - gaps
                let natural = preferred.reduce(0, +)
                let scale = budget / max(natural, 1)
                widths = preferred.map { max(minW, min(maxW, $0 * scale)) }
            }
        }
        var changed = false
        for (chip, width) in zip(chips, widths) {
            let before = chip.bounds.width
            chip.applyWidth(width, minWidth: minW, maxWidth: maxW)
            if abs(before - width) > 0.5 { changed = true }
        }
        return changed
    }

    func revealSelectedTabChip() {
        guard let chip = selectedTabChip else { return }
        let chipRect = chip.convert(chip.bounds, to: tabScrollContent)
        tabScroll.contentView.scrollToVisible(chipRect)
        tabScroll.reflectScrolledClipView(tabScroll.contentView)
    }

    override func layout() {
        super.layout()
        guard !isCompressingTabs else { return }
        isCompressingTabs = true
        defer { isCompressingTabs = false }
        let theme = NativeThemeRealization.theme(for: effectiveAppearance)
        _ = compressTabChips(theme: theme)
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
                    showsAccent: false,
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
        var lastSection: String?
        for index in 0..<Int(chrome.inspector_row_count) {
            let row = seyal_app_chrome_row(pane.appHandle, UInt16(SEYAL_APP_ROW_INSPECTOR), UInt32(index))
            let joined = productChromeCopyUTF8(row.title, row.title_len) ?? ""
            let value = productChromeCopyUTF8(row.detail, row.detail_len) ?? ""
            let parts = joined.split(separator: "·", maxSplits: 1, omittingEmptySubsequences: false)
            let section: String
            let label: String
            if parts.count == 2 {
                section = parts[0].trimmingCharacters(in: .whitespaces)
                label = parts[1].trimmingCharacters(in: .whitespaces)
            } else {
                section = ""
                label = joined
            }
            if section != lastSection {
                if lastSection != nil {
                    let seam = NSView()
                    seam.translatesAutoresizingMaskIntoConstraints = false
                    seam.wantsLayer = true
                    seam.identifier = NSUserInterfaceItemIdentifier("seyal-inspector-section-seam")
                    seam.heightAnchor.constraint(equalToConstant: 1).isActive = true
                    inspector.addArrangedSubview(seam)
                    inspector.setCustomSpacing(14, after: seam)
                }
                if !section.isEmpty {
                    let header = NSTextField(labelWithString: section.uppercased())
                    header.font = .systemFont(ofSize: 10, weight: .semibold)
                    header.tag = 3
                    header.setAccessibilityIdentifier("seyal-inspector-section-\(section)")
                    inspector.addArrangedSubview(header)
                    inspector.setCustomSpacing(8, after: header)
                }
                lastSection = section
            }
            let rowStack = NSStackView()
            rowStack.orientation = .horizontal
            rowStack.alignment = .firstBaseline
            rowStack.distribution = .fill
            rowStack.spacing = 8
            rowStack.translatesAutoresizingMaskIntoConstraints = false
            let caption = NSTextField(labelWithString: label)
            caption.font = .systemFont(ofSize: 11, weight: .regular)
            caption.tag = 2
            caption.setContentHuggingPriority(.defaultLow, for: .horizontal)
            caption.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
            let body = NSTextField(labelWithString: value)
            body.font = .systemFont(ofSize: 11, weight: .regular)
            body.alignment = .right
            body.maximumNumberOfLines = 2
            body.lineBreakMode = .byTruncatingTail
            body.setContentHuggingPriority(.defaultHigh, for: .horizontal)
            body.setContentCompressionResistancePriority(.required, for: .horizontal)
            body.tag = 4
            rowStack.addArrangedSubview(caption)
            rowStack.addArrangedSubview(body)
            inspector.addArrangedSubview(rowStack)
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
        centerColumn.apply(theme: theme)
        transcript.backgroundColor = .clear
        left.layer?.borderWidth = 0
        inspectorColumn.layer?.borderWidth = 0
        leftSeam.apply(color: theme.seam)
        inspectorSeam.apply(color: theme.seam)
        tabUnderline.layer?.backgroundColor = theme.blockFocus.cgColor
        for case let chip as TabChipView in tabButtons.arrangedSubviews {
            chip.apply(theme: theme)
        }
        compressTabChips(theme: theme)
        for case let row as ChromeContextRowView in leftItems.arrangedSubviews {
            row.apply(theme: theme)
        }
        for view in inspector.arrangedSubviews {
            if let field = view as? NSTextField {
                if field.tag == 3 {
                    field.textColor = theme.secondary
                    field.font = .systemFont(ofSize: max(theme.uiFontSize - 3, 10), weight: .semibold)
                } else if field.tag == 2 {
                    field.textColor = theme.muted
                } else if field.tag == 4 {
                    field.textColor = theme.text
                }
            } else if let stack = view as? NSStackView {
                for case let field as NSTextField in stack.arrangedSubviews {
                    if field.tag == 2 {
                        field.textColor = theme.muted
                        field.font = .systemFont(ofSize: max(theme.uiFontSize - 2, 10), weight: .regular)
                    } else if field.tag == 4 {
                        field.textColor = theme.text
                        field.font = .systemFont(ofSize: max(theme.uiFontSize - 2, 10), weight: .regular)
                    }
                }
            } else if view.identifier?.rawValue == "seyal-inspector-section-seam" {
                view.layer?.backgroundColor = theme.seam.cgColor
            }
        }
        tabTitle.font = .systemFont(ofSize: theme.uiFontSize, weight: .semibold)
        recoveryLabel.font = .systemFont(ofSize: max(theme.uiFontSize - 1, 10), weight: .regular)
        blocks.edgeInsets = NSEdgeInsets(
            top: theme.terminalPadding,
            left: theme.windowPadding,
            bottom: theme.terminalPadding,
            right: theme.windowPadding
        )
        updateColdVisualProbe(theme)
        let chrome = seyal_app_chrome(pane.appHandle)
        applySwitcherAppearance(leftPanel: chrome.left_panel, theme: theme)
        for button in [newTabButton, closeTabButton, splitRightButton, splitDownButton, closePaneButton] {
            button.contentTintColor = theme.muted
            button.font = .systemFont(ofSize: theme.uiFontSize, weight: .medium)
        }
        composer.apply(theme: theme)
        centerColumn.applyThemeToSecondaryHosts(theme)
        focusedPaneChrome.apply(
            title: centerColumn.regions.first(where: \.focused)?.title ?? "Pane",
            theme: theme,
            focused: true
        )
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
