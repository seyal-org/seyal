import AppKit

/// Composer editor. Product shortcuts (including history recall) are matched in
/// Rust via the SPEC-024 table; this view only forwards a normalized stroke.
@MainActor
private final class ComposerTextView: NSTextView {
    /// Returns true when the key event was consumed by Rust routing.
    var onRouteKeystroke: ((NSEvent) -> Bool)?

    /// Busy / DirectInput must not let a non-editable dock steal keys from the
    /// Running Block live surface (#1468).
    override var acceptsFirstResponder: Bool { isEditable }

    override func keyDown(with event: NSEvent) {
        if onRouteKeystroke?(event) == true {
            return
        }
        super.keyDown(with: event)
    }

    /// XCUI Cmd-V paste can update `string` without a delegate `textDidChange`
    /// turn; push the draft so CAN_SUBMIT / execute enable stay in sync (#1424).
    override func paste(_ sender: Any?) {
        super.paste(sender)
        delegate?.textDidChange?(Notification(name: NSText.didChangeNotification, object: self))
    }
}

/// Native IME/editor bridge only. Draft, submit, and C09 copy stay Rust-owned.
@MainActor
final class ComposerBridgeView: NSView, NSTextViewDelegate {
    /// Second argument is ApplicationRoot's pending request id (0 if unknown).
    var onSubmitComposer: ((String, UInt64) -> Int32)?
    var onSubmitRaw: ((String) -> Int32)?
    /// Rust accepted OpenComposerHistory; the host reconciles the overlay.
    var onHistoryOpened: (() -> Void)?
    /// A table match changed product chrome (palette, tabs, presentation).
    var onCommandConsumed: (() -> Void)?
    var onFocusEligibilityChanged: (() -> Void)?
    /// Host focuses this Pane before draft/submit when it is not already focused.
    var onEnsureFocused: (() -> Void)?

    private let appHandle: UInt64
    /// When set, this bridge reads the named Pane's composer and focuses that
    /// Pane before mutating (ADR-009 multipane / fence authority).
    private let boundPaneLo: UInt64?
    private let boundPaneHi: UInt64?
    /// Multipane Flow host Controller. DirectInput hide follows this handle's
    /// live target so a sibling Available cannot resurrect the Busy dock.
    var boundDisplayHandle: UInt64 = 0
    private let textView = ComposerTextView()
    private let placeholder = NSTextField(labelWithString: "")
    private let execute = NSButton(title: "", target: nil, action: nil)
    private let history = NSButton(title: "", target: nil, action: nil)
    private var heightConstraint: NSLayoutConstraint!
    private var textViewLeadingConstraint: NSLayoutConstraint!
    private var textViewTopConstraint: NSLayoutConstraint!
    private var textViewBottomConstraint: NSLayoutConstraint!
    private var textViewDockTrailingConstraint: NSLayoutConstraint!
    private var textViewInlineTrailingConstraint: NSLayoutConstraint!
    private var inlineRelayPresentation = false
    private var lastEpoch: UInt64 = 0
    private var focusEligibilityGeneration: UInt64 = 0
    private var hasReconciledComposerState = false
    private var theme: NativeTheme?
    private var editing = false

    var isFocusEligibleForRecovery: Bool {
        hasReconciledComposerState && !isHidden && textView.isEditable && window != nil
    }

    init(appHandle: UInt64, paneLo: UInt64? = nil, paneHi: UInt64? = nil) {
        self.appHandle = appHandle
        self.boundPaneLo = paneLo
        self.boundPaneHi = paneHi
        super.init(frame: .zero)
        translatesAutoresizingMaskIntoConstraints = false
        wantsLayer = true
        layer?.cornerRadius = 10
        layer?.cornerCurve = .continuous
        layer?.masksToBounds = true
        focusRingType = .none
        if let boundPaneLo, let boundPaneHi {
            setAccessibilityIdentifier(
                String(format: "seyal-composer-%016llx-%016llx", boundPaneHi, boundPaneLo)
            )
        } else {
            setAccessibilityIdentifier("seyal-composer")
        }
        // Group, not a leaf textArea: XCUI must see both the dock and the
        // execute control. A leaf role hid `seyal-composer-execute`.
        setAccessibilityRole(.group)
        setAccessibilityElement(true)

        placeholder.font = .monospacedSystemFont(ofSize: 13, weight: .regular)
        placeholder.tag = 2
        placeholder.setAccessibilityElement(false)

        execute.target = self
        execute.action = #selector(executeClicked)
        execute.isBordered = false
        execute.font = .systemFont(ofSize: 13, weight: .medium)
        execute.focusRingType = .none
        execute.setButtonType(.momentaryPushIn)
        execute.setContentHuggingPriority(.required, for: .horizontal)
        execute.setContentCompressionResistancePriority(.required, for: .horizontal)
        execute.setAccessibilityIdentifier("seyal-composer-execute")
        execute.setAccessibilityRole(.button)

        history.target = self
        history.action = #selector(historyClicked)
        history.isBordered = false
        history.font = .systemFont(ofSize: 11, weight: .medium)
        history.focusRingType = .none
        history.setButtonType(.momentaryPushIn)
        history.setContentHuggingPriority(.required, for: .horizontal)
        history.setContentCompressionResistancePriority(.required, for: .horizontal)
        history.setAccessibilityIdentifier("seyal-composer-history-toggle")
        history.setAccessibilityRole(.button)
        history.toolTip = "Command history"

        textView.delegate = self
        textView.onRouteKeystroke = { [weak self] event in
            self?.routeComposerKeystroke(event) ?? false
        }
        textView.isRichText = false
        textView.isAutomaticQuoteSubstitutionEnabled = false
        textView.isAutomaticDashSubstitutionEnabled = false
        textView.isAutomaticTextReplacementEnabled = false
        textView.font = .monospacedSystemFont(ofSize: 13, weight: .regular)
        textView.drawsBackground = false
        textView.focusRingType = .none
        textView.isVerticallyResizable = true
        textView.textContainer?.lineFragmentPadding = 0
        textView.textContainer?.widthTracksTextView = true
        textView.translatesAutoresizingMaskIntoConstraints = false
        textView.setAccessibilityElement(true)
        // Pane-bound editors start unfocused. Only the input-owned composer
        // keeps `seyal-composer-editor`, so XCUI firstMatch cannot paste into
        // a Busy sibling that is still streaming (#1433 / #1424).
        textView.setAccessibilityIdentifier(
            boundPaneLo == nil ? "seyal-composer-editor" : "seyal-secondary-composer-editor"
        )

        addSubview(textView)
        addSubview(placeholder)
        addSubview(history)
        addSubview(execute)
        placeholder.translatesAutoresizingMaskIntoConstraints = false
        history.translatesAutoresizingMaskIntoConstraints = false
        execute.translatesAutoresizingMaskIntoConstraints = false
        heightConstraint = heightAnchor.constraint(equalToConstant: 40)
        textViewLeadingConstraint = textView.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 14)
        textViewTopConstraint = textView.topAnchor.constraint(equalTo: topAnchor, constant: 8)
        textViewBottomConstraint = textView.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -8)
        textViewDockTrailingConstraint = textView.trailingAnchor.constraint(
            equalTo: history.leadingAnchor,
            constant: -8
        )
        textViewInlineTrailingConstraint = textView.trailingAnchor.constraint(
            equalTo: trailingAnchor,
            constant: -1
        )
        NSLayoutConstraint.activate([
            heightConstraint,
            textViewLeadingConstraint,
            textViewDockTrailingConstraint,
            history.trailingAnchor.constraint(equalTo: execute.leadingAnchor, constant: -4),
            history.centerYAnchor.constraint(equalTo: centerYAnchor),
            history.heightAnchor.constraint(equalToConstant: 28),
            textViewTopConstraint,
            textViewBottomConstraint,
            placeholder.leadingAnchor.constraint(equalTo: textView.leadingAnchor),
            placeholder.centerYAnchor.constraint(equalTo: centerYAnchor),
            execute.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -8),
            execute.centerYAnchor.constraint(equalTo: centerYAnchor),
            execute.widthAnchor.constraint(greaterThanOrEqualToConstant: 28),
            execute.heightAnchor.constraint(equalToConstant: 28),
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("ComposerBridgeView is programmatic")
    }

    @discardableResult
    func focusEditor() -> Bool {
        // Never force-focus a non-editable Busy dock over DirectInput Metal.
        guard textView.isEditable else { return false }
        return window?.makeFirstResponder(textView) ?? false
    }

    /// Placeholder sits above the editor in z-order; clicks on empty-draft
    /// chrome must still focus the text view (#867 / #1474).
    override func hitTest(_ point: NSPoint) -> NSView? {
        // DirectInput retracts the dock with alpha 0 (still AX-visible Busy).
        // Do not steal clicks from the Running Block Metal surface.
        if alphaValue < 0.01 { return nil }
        let hit = super.hitTest(point)
        if hit === placeholder || hit?.isDescendant(of: placeholder) == true {
            return textView.isEditable ? textView : self
        }
        return hit
    }

    override func mouseDown(with event: NSEvent) {
        if focusEditor() {
            textView.mouseDown(with: event)
            return
        }
        super.mouseDown(with: event)
    }

  var editorIsFirstResponder: Bool {
    window?.firstResponder === textView
  }

    /// Multipane focused host uses the primary `seyal-composer` AX id so XCUI
    /// and existing host assertions keep working; unfocused keeps pane-scoped id.
    /// The editor id follows the same rule: a visible Busy unfocused composer
    /// must not stay `seyal-composer-editor` or firstMatch paste hits it (#1433).
    func setPrimaryAccessibilityIdentifier(_ primary: Bool) {
        guard let boundPaneLo, let boundPaneHi else {
            setAccessibilityIdentifier("seyal-composer")
            textView.setAccessibilityIdentifier("seyal-composer-editor")
            setAccessibilityElement(true)
            return
        }
        if primary {
            setAccessibilityIdentifier("seyal-composer")
            textView.setAccessibilityIdentifier("seyal-composer-editor")
        } else {
            setAccessibilityIdentifier(
                String(format: "seyal-composer-%016llx-%016llx", boundPaneHi, boundPaneLo)
            )
            textView.setAccessibilityIdentifier("seyal-secondary-composer-editor")
        }
        setAccessibilityElement(true)
    }

    /// Hide the single-pane dock from AX while multipane leaf hosts own Flow.
    func retirePrimaryAccessibilityIdentity() {
        setAccessibilityIdentifier("seyal-composer-legacy-dock")
        textView.setAccessibilityIdentifier("seyal-composer-legacy-editor")
        setAccessibilityElement(false)
        textView.setAccessibilityElement(false)
        execute.setAccessibilityElement(false)
    }

    /// Restore the single-pane dock as the primary Flow composer AX target.
    func restorePrimaryAccessibilityIdentity() {
        guard boundPaneLo == nil else { return }
        setAccessibilityIdentifier("seyal-composer")
        textView.setAccessibilityIdentifier("seyal-composer-editor")
        setAccessibilityElement(true)
        textView.setAccessibilityElement(true)
        execute.setAccessibilityElement(true)
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        focusEligibilityGeneration &+= 1
        guard isFocusEligibleForRecovery else { return }
        enqueueFocusEligibilityChange(generation: focusEligibilityGeneration)
    }

    func apply(theme: NativeTheme) {
        self.theme = theme
        placeholder.textColor = theme.muted
        placeholder.font = .monospacedSystemFont(ofSize: theme.terminalFontSize, weight: .regular)
        textView.textColor = theme.text
        textView.font = .monospacedSystemFont(ofSize: theme.terminalFontSize, weight: .regular)
        textView.insertionPointColor = theme.accent
        execute.font = .systemFont(ofSize: theme.uiFontSize, weight: .medium)
        paintChrome()
    }

    func setInlineRelayPresentation(_ inline: Bool) {
        guard inlineRelayPresentation != inline else { return }
        inlineRelayPresentation = inline
        textViewDockTrailingConstraint.isActive = !inline
        textViewInlineTrailingConstraint.isActive = inline
        heightConstraint.constant = inline ? 24 : 40
        textViewLeadingConstraint.constant = inline ? 0 : 14
        textViewTopConstraint.constant = inline ? 1 : 8
        textViewBottomConstraint.constant = inline ? -1 : -8
        history.isHidden = inline
        execute.isHidden = inline
        if inline {
            placeholder.stringValue = ""
        } else {
            placeholder.stringValue = copyString(UInt16(SEYAL_APP_COPY_COMPOSER_PLACEHOLDER))
        }
        paintChrome()
        needsLayout = true
    }

    private func readComposer() -> SeyalAppComposer {
        if let lo = boundPaneLo, let hi = boundPaneHi {
            return seyal_app_composer_for_pane(appHandle, lo, hi)
        }
        return seyal_app_composer(appHandle)
    }

    /// Focus this Pane when bound and not already the fence authority, then
    /// return a fresh focused snapshot for mutating actions.
    @discardableResult
    private func prepareFocusedMutation() -> SeyalAppSnapshot {
        prepareBoundPaneInputFocus()
        return seyal_app_snapshot(appHandle)
    }

    /// If this editor reports a draft change before Rust focus reaches its Pane,
    /// reconcile that focus and restore this responder before applying the
    /// draft mutation to Rust.
    private func prepareBoundPaneInputFocus() {
        if let paneLo = boundPaneLo, let paneHi = boundPaneHi {
            let shell = seyal_app_shell(appHandle)
            if shell.focused_pane_lo != paneLo || shell.focused_pane_hi != paneHi {
                onEnsureFocused?()
                if textView.isEditable {
                    _ = window?.makeFirstResponder(textView)
                }
            }
        }
    }

    func reconcile() {
        let composer = readComposer()
        let snapshot = seyal_app_snapshot(appHandle)
        let wasFocusEligible = isFocusEligibleForRecovery
        // Focused bridge follows snapshot eligibility; pane-bound bridges use
        // only that Pane's composer mode so a sibling TUI/Raw does not hide them.
        let direct = boundPaneLo == nil
            && (snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue)
                || snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue))
        // Warp-like: Block-scoped DirectInput owns keys in the Running Block.
        // Hide the Busy dock so users are not offered a second typing surface.
        // Pane-bound: this Pane's composer flag and/or Controller target — never
        // the shared ApplicationRoot snapshot (siblings clear that on Available).
        let blockScopedDirectInput: Bool = {
            if boundPaneLo != nil {
                guard boundDisplayHandle != 0 else {
                    return seyalComposerFlagsPublishBlockScopedDirectInput(composer.flags)
                }
                return seyalPublishesBlockScopedDirectInput(
                    flags: composer.flags,
                    target: seyal_bridge_composer_target_for_handle(boundDisplayHandle)
                )
            }
            if seyalPublishesBlockScopedDirectInput(
                flags: composer.flags,
                target: seyal_bridge_composer_target()
            ) {
                return true
            }
            return snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_FLOW.rawValue)
                && snapshot.flags & UInt16(SEYAL_APP_SNAP_BLOCK_SCOPED_DIRECT_INPUT) != 0
        }()
        // Raw/TUI/Hidden may fully remove the dock. Block-scoped DirectInput
        // keeps the view in the AX tree (alpha/placement owned by the host) so
        // Busy admission stays observable as `busy` instead of a stale
        // `available` after `isHidden` (#1474 headed Busy XCUI).
        let hidden = direct
            || composer.mode == UInt16(SEYAL_APP_COMPOSER_HIDDEN.rawValue)
        // Presentation/epoch transitions revoke composer IME ownership: cancel
        // uncommitted marked text without submitting (SPEC-008 / #866).
        if (direct || hidden || blockScopedDirectInput || composer.epoch != lastEpoch),
            hasMarkedText()
        {
            textView.inputContext?.discardMarkedText()
        }
        isHidden = hidden
        let available = composer.mode == UInt16(SEYAL_APP_COMPOSER_AVAILABLE.rawValue)
        let relay = composer.mode == UInt16(SEYAL_APP_COMPOSER_RELAY.rawValue)
        let busy = composer.mode == UInt16(SEYAL_APP_COMPOSER_BUSY.rawValue)
        textView.isEditable = available || relay
        hasReconciledComposerState = true
        // Resign a Busy/disabled editor so DirectInput Metal can keep keys.
        if !textView.isEditable, window?.firstResponder === textView {
            window?.makeFirstResponder(nil)
        }
        let isNowFocusEligible = isFocusEligibleForRecovery
        execute.title = copyString(UInt16(SEYAL_APP_COPY_COMPOSER_EXECUTE))
        execute.isEnabled = composer.flags & UInt16(SEYAL_APP_COMPOSER_CAN_SUBMIT) != 0
        let recall = seyal_app_composer_history(appHandle)
        history.title = copyString(UInt16(SEYAL_APP_COPY_COMPOSER_HISTORY))
        history.isEnabled = available && recall.flags & UInt16(SEYAL_APP_HISTORY_HAS_ENTRIES) != 0
        if inlineRelayPresentation {
            placeholder.stringValue = ""
        } else if busy || blockScopedDirectInput {
            // Match Rust ComposerMode::Busy placeholder — never look Available.
            placeholder.stringValue = "Waiting for prompt..."
        } else {
            placeholder.stringValue = copyString(UInt16(SEYAL_APP_COPY_COMPOSER_PLACEHOLDER))
        }
        // DirectInput retracts the dock visually via host placement. When Rust
        // mode is Busy, AX must read `busy` (not a stale Available). When Rust
        // mode is Available again, Available wins — sticky DirectInput flags
        // must not keep AX Busy after the prompt returns (#1474 SSH/Busy).
        let axValue: String =
            relay
            ? "relay"
            : (available
                ? "available"
                : ((busy || blockScopedDirectInput) ? "busy" : "hidden"))
        setAccessibilityValue(axValue)
        // XCUI samples AX asynchronously; without a value-changed post the
        // waiter can keep a stale `available` across a quiet Busy latch (#1474).
        NSAccessibility.post(element: self, notification: .valueChanged)
        if composer.epoch != lastEpoch {
            lastEpoch = composer.epoch
            // The Rust draft is authoritative in every mode (SPEC-008 §4). A
            // Runtime-published busy state with an empty draft (command
            // accepted, prompt not back yet) must show the placeholder, not
            // the text that was already submitted. Skip identical rewrites so
            // status/focus reconcile does not reset the caret (#867 / #1474).
            let decoded: String
            if composer.draft_utf8_len > 0, let bytes = composer.draft_utf8 {
                decoded = String(
                    decoding: UnsafeBufferPointer(start: bytes, count: Int(composer.draft_utf8_len)),
                    as: UTF8.self
                )
            } else {
                decoded = ""
            }
            if textView.string != decoded {
                textView.string = decoded
            }
        }
        placeholder.isHidden = !textView.string.isEmpty
        expandToDraft()
        paintChrome()
        if !isNowFocusEligible, wasFocusEligible {
            focusEligibilityGeneration &+= 1
        } else if isNowFocusEligible, !wasFocusEligible {
            focusEligibilityGeneration &+= 1
            enqueueFocusEligibilityChange(generation: focusEligibilityGeneration)
        }
    }

    private func enqueueFocusEligibilityChange(generation: UInt64) {
        DispatchQueue.main.async { [weak self] in
            guard let self,
                  self.focusEligibilityGeneration == generation,
                  self.isFocusEligibleForRecovery
            else { return }
            self.onFocusEligibilityChanged?()
        }
    }

    func textDidBeginEditing(_ notification: Notification) {
        editing = true
        paintChrome()
    }

    func textDidEndEditing(_ notification: Notification) {
        editing = false
        paintChrome()
    }

    func textDidChange(_ notification: Notification) {
        placeholder.isHidden = !textView.string.isEmpty
        pushDraft()
        let composer = readComposer()
        execute.isEnabled = composer.flags & UInt16(SEYAL_APP_COMPOSER_CAN_SUBMIT) != 0
        expandToDraft()
    }

    func textView(_ textView: NSTextView, shouldChangeTextIn affectedCharRange: NSRange, replacementString: String?) -> Bool {
        if replacementString == "\n" || replacementString == "\r" {
            if shiftHeld() {
                return true
            }
            submit()
            return false
        }
        return true
    }

    func textView(_ textView: NSTextView, doCommandBy commandSelector: Selector) -> Bool {
        if commandSelector == #selector(NSResponder.insertNewline(_:)) {
            if shiftHeld() {
                return false
            }
            submit()
            return true
        }
        // Composer is not shell authority (M001 §5.2): do not insert literal
        // Tab/Backtab into the draft. Shell completion stays on the terminal
        // surface until supported integration proves equivalent semantics.
        if commandSelector == #selector(NSResponder.insertTab(_:))
            || commandSelector == #selector(NSResponder.insertBacktab(_:))
        {
            return true
        }
        return false
    }

    @objc private func executeClicked() {
        submit()
    }

    @objc private func historyClicked() {
        openHistory()
    }

    /// Rust owns the match; a consumed WorkspaceCommand already mutated product
    /// state. Do not insert matched/Command strokes into the draft.
    private func routeComposerKeystroke(_ event: NSEvent) -> Bool {
        switch KeybindingStrokeNormalizer.route(
            appHandle: appHandle,
            event: event,
            composerFocused: true,
            compositionActive: hasMarkedText()
        ) {
        case .consumed:
            reconcile()
            onCommandConsumed?()
            let history = seyal_app_composer_history(appHandle)
            if history.flags & UInt16(SEYAL_APP_HISTORY_OPEN) != 0 {
                onHistoryOpened?()
            }
            return true
        case .nativeCommand:
            // Reserved Command — ordinary native text commands
            // (⌘←/⌘⌫/…); never PTY (this is the composer NSTextView).
            return false
        case .fallsThrough:
            return false
        }
    }

    private func hasMarkedText() -> Bool {
        textView.hasMarkedText()
    }

    /// R6.2.1 / R8.4: host `performKeyEquivalent` must see the same marked-text
    /// state as `routeComposerKeystroke`.
    var hasMarkedComposition: Bool { hasMarkedText() }

    /// Rust decides whether recall is available (mode, entries); a rejected
    /// open leaves the composer untouched.
    private func openHistory() {
        pushDraft()
        let snapshot = prepareFocusedMutation()
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_OPEN_COMPOSER_HISTORY.rawValue)
        action.applySnapshotFence(snapshot)
        guard seyal_app_apply(appHandle, &action) == 0 else { return }
        onHistoryOpened?()
    }

    private func shiftHeld() -> Bool {
        NSApp.currentEvent?.modifierFlags.contains(.shift) == true
    }

    private func paintChrome() {
        guard let theme else { return }
        if inlineRelayPresentation {
            layer?.backgroundColor = NSColor.clear.cgColor
            layer?.borderWidth = 0
            layer?.cornerRadius = 0
            return
        }
        layer?.backgroundColor = theme.elevated.cgColor
        layer?.borderWidth = 1
        layer?.borderColor = editing
            ? theme.accent.withAlphaComponent(0.45).cgColor
            : theme.seam.cgColor
        execute.contentTintColor = theme.muted
        execute.attributedTitle = NSAttributedString(
            string: execute.title,
            attributes: [
                .font: NSFont.systemFont(ofSize: 13, weight: .medium),
                .foregroundColor: execute.isEnabled ? theme.accent : theme.muted,
            ]
        )
        history.attributedTitle = NSAttributedString(
            string: history.title,
            attributes: [
                .font: NSFont.systemFont(ofSize: 11, weight: .medium),
                .foregroundColor: history.isEnabled ? theme.secondary : theme.muted,
            ]
        )
    }

    private func expandToDraft() {
        guard let container = textView.textContainer,
              let layout = textView.layoutManager
        else { return }
        layout.ensureLayout(for: container)
        let used = layout.usedRect(for: container).height
        heightConstraint.constant = inlineRelayPresentation
            ? min(max(24, used + 2), 120) : min(max(40, used + 16), 120)
    }

    private func copyString(_ kind: UInt16) -> String {
        let row = seyal_app_copy(appHandle, kind)
        return copyUTF8(row.title, row.title_len) ?? ""
    }

    private func pushDraft(command: String? = nil) {
        // Capture before focus reconcile: FOCUS_PANE → chrome reconcile can
        // rewrite textView from an empty Rust draft and drop the XCUI paste
        // (#1474 pane-local marker routing).
        let draft = command ?? textView.string
        let snapshot = prepareFocusedMutation()
        if textView.string != draft {
            textView.string = draft
        }
        let composer = readComposer()
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_SET_COMPOSER_DRAFT.rawValue)
        action.applySnapshotFence(snapshot)
        action.target_generation = composer.epoch
        let utf8 = Array(draft.utf8)
        utf8.withUnsafeBufferPointer { buffer in
            action.payload = buffer.baseAddress
            action.payload_len = UInt32(buffer.count)
            _ = seyal_app_apply(appHandle, &action)
        }
    }

    private func submit() {
        let command = textView.string
        guard !command.isEmpty else { return }
        pushDraft(command: command)
        let composer = readComposer()
        guard composer.flags & UInt16(SEYAL_APP_COMPOSER_CAN_SUBMIT) != 0 else { return }
        let snapshot = prepareFocusedMutation()
        if textView.string != command {
            textView.string = command
        }
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_SUBMIT_COMPOSER.rawValue)
        action.applySnapshotFence(snapshot)
        action.target_generation = composer.epoch
        guard seyal_app_apply(appHandle, &action) == 0 else { return }
        // Keep pending_request_id until Runtime's ComposerResult is drained.
        // Optimistic accept cleared pending before Busy status arrived and let
        // a stale Submit→Available projection win on quiet commands (#1474).
        // Pin the AppRoot pending id onto the Controller so ComposerResult
        // cannot be attributed across independently allocating clients (#1474).
        let pendingRequestID = readComposer().request_id
        var status = onSubmitComposer?(command, pendingRequestID) ?? -10
        if status == -12 {
            let payload = command.hasSuffix("\r") ? command : command + "\r"
            status = onSubmitRaw?(payload) ?? -10
        }
        if status != 0, pendingRequestID != 0 {
            // Transport rejected the command; clear pending without wiping the
            // draft so the user can retry (ApplyResult accepted=false).
            if let lo = boundPaneLo, let hi = boundPaneHi {
                _ = seyal_app_apply_composer_result_for_pane(
                    appHandle, lo, hi, pendingRequestID, 0
                )
            } else {
                applyComposerResult(requestID: pendingRequestID, accepted: false)
            }
        }
        reconcile()
    }

    func applyComposerResult(requestID: UInt64, accepted: Bool) {
        let snapshot = prepareFocusedMutation()
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_APPLY_COMPOSER_RESULT.rawValue)
        action.applySnapshotFence(snapshot)
        action.target_execution_lo = requestID
        action.reserved = accepted ? 1 : 0
        _ = seyal_app_apply(appHandle, &action)
        reconcile()
    }

    /// Native projection of a failed Runtime admission. Rust keeps the draft;
    /// the host only announces the failure (no secret-bearing text).
    func presentAdmissionFailure(_ code: NativeComposerResult.Code) {
        let message: String
        switch code {
        case .busy:
            message = "Command not accepted: shell is busy."
        case .backpressure:
            message = "Command not accepted: terminal is busy. Retry."
        case .invalid:
            message = "Command not accepted."
        case .accepted, .unsupported:
            return
        }
        setAccessibilityValue(message)
        SeyalAccessibilityAnnouncement.post(message, element: self)
    }
}

private func copyUTF8(_ pointer: UnsafePointer<UInt8>?, _ length: UInt32) -> String? {
    guard length > 0, let pointer else { return nil }
    return String(decoding: UnsafeBufferPointer(start: pointer, count: Int(length)), as: UTF8.self)
}
