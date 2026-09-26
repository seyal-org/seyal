import AppKit

/// Composer editor that reports `⌃R` (history recall, #933) instead of
/// letting NSTextView swallow it. Every other key stays native.
@MainActor
private final class ComposerTextView: NSTextView {
    var onHistoryShortcut: (() -> Void)?

    override func keyDown(with event: NSEvent) {
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        if flags == .control, event.charactersIgnoringModifiers == "r" {
            onHistoryShortcut?()
            return
        }
        super.keyDown(with: event)
    }
}

/// Native IME/editor bridge only. Draft, submit, and C09 copy stay Rust-owned.
@MainActor
final class ComposerBridgeView: NSView, NSTextViewDelegate {
    var onSubmitComposer: ((String) -> Int32)?
    var onSubmitRaw: ((String) -> Int32)?
    /// Rust accepted OpenComposerHistory; the host reconciles the overlay.
    var onHistoryOpened: (() -> Void)?

    private let appHandle: UInt64
    private let textView = ComposerTextView()
    private let placeholder = NSTextField(labelWithString: "")
    private let execute = NSButton(title: "", target: nil, action: nil)
    private let history = NSButton(title: "", target: nil, action: nil)
    private var heightConstraint: NSLayoutConstraint!
    private var lastEpoch: UInt64 = 0
    private var theme: NativeTheme?
    private var editing = false

    init(appHandle: UInt64) {
        self.appHandle = appHandle
        super.init(frame: .zero)
        translatesAutoresizingMaskIntoConstraints = false
        wantsLayer = true
        layer?.cornerRadius = 10
        layer?.cornerCurve = .continuous
        layer?.masksToBounds = true
        focusRingType = .none
        setAccessibilityIdentifier("seyal-composer")
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
        textView.onHistoryShortcut = { [weak self] in self?.openHistory() }
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
        textView.setAccessibilityIdentifier("seyal-composer-editor")

        addSubview(textView)
        addSubview(placeholder)
        addSubview(history)
        addSubview(execute)
        placeholder.translatesAutoresizingMaskIntoConstraints = false
        history.translatesAutoresizingMaskIntoConstraints = false
        execute.translatesAutoresizingMaskIntoConstraints = false
        heightConstraint = heightAnchor.constraint(equalToConstant: 40)
        NSLayoutConstraint.activate([
            heightConstraint,
            textView.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 14),
            textView.trailingAnchor.constraint(equalTo: history.leadingAnchor, constant: -8),
            history.trailingAnchor.constraint(equalTo: execute.leadingAnchor, constant: -4),
            history.centerYAnchor.constraint(equalTo: centerYAnchor),
            history.heightAnchor.constraint(equalToConstant: 28),
            textView.topAnchor.constraint(equalTo: topAnchor, constant: 8),
            textView.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -8),
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

    func focusEditor() {
        window?.makeFirstResponder(textView)
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

    func reconcile() {
        let composer = seyal_app_composer(appHandle)
        let snapshot = seyal_app_snapshot(appHandle)
        let direct = snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue)
            || snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue)
        isHidden = direct
        let available = composer.mode == UInt16(SEYAL_APP_COMPOSER_AVAILABLE.rawValue)
        let busy = composer.mode == UInt16(SEYAL_APP_COMPOSER_BUSY.rawValue)
        textView.isEditable = available
        execute.title = copyString(UInt16(SEYAL_APP_COPY_COMPOSER_EXECUTE))
        execute.isEnabled = composer.flags & UInt16(SEYAL_APP_COMPOSER_CAN_SUBMIT) != 0
        let recall = seyal_app_composer_history(appHandle)
        history.title = copyString(UInt16(SEYAL_APP_COPY_COMPOSER_HISTORY))
        history.isEnabled = available && recall.flags & UInt16(SEYAL_APP_HISTORY_HAS_ENTRIES) != 0
        placeholder.stringValue = copyString(UInt16(SEYAL_APP_COPY_COMPOSER_PLACEHOLDER))
        setAccessibilityValue(available ? "available" : (busy ? "busy" : "hidden"))
        if composer.epoch != lastEpoch {
            lastEpoch = composer.epoch
            // The Rust draft is authoritative in every mode (SPEC-008 §4). A
            // Runtime-published busy state with an empty draft (command
            // accepted, prompt not back yet) must show the placeholder, not
            // the text that was already submitted.
            if composer.draft_utf8_len > 0, let bytes = composer.draft_utf8 {
                textView.string = String(
                    decoding: UnsafeBufferPointer(start: bytes, count: Int(composer.draft_utf8_len)),
                    as: UTF8.self
                )
            } else {
                textView.string = ""
            }
        }
        placeholder.isHidden = !textView.string.isEmpty
        expandToDraft()
        paintChrome()
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
        let composer = seyal_app_composer(appHandle)
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
        return false
    }

    @objc private func executeClicked() {
        submit()
    }

    @objc private func historyClicked() {
        openHistory()
    }

    /// Rust decides whether recall is available (mode, entries); a rejected
    /// open leaves the composer untouched.
    private func openHistory() {
        pushDraft()
        let snapshot = seyal_app_snapshot(appHandle)
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
        heightConstraint.constant = min(max(40, used + 16), 120)
    }

    private func copyString(_ kind: UInt16) -> String {
        let row = seyal_app_copy(appHandle, kind)
        return copyUTF8(row.title, row.title_len) ?? ""
    }

    private func pushDraft() {
        let composer = seyal_app_composer(appHandle)
        let snapshot = seyal_app_snapshot(appHandle)
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_SET_COMPOSER_DRAFT.rawValue)
        action.applySnapshotFence(snapshot)
        action.target_pty_generation = composer.epoch
        let utf8 = Array(textView.string.utf8)
        utf8.withUnsafeBufferPointer { buffer in
            action.payload = buffer.baseAddress
            action.payload_len = UInt32(buffer.count)
            _ = seyal_app_apply(appHandle, &action)
        }
    }

    /// Block Rerun (#1010): Rust has already loaded the Block's command as the
    /// draft. Mirror that Rust draft and submit it through the ordinary path.
    func submitRustDraft() {
        let composer = seyal_app_composer(appHandle)
        guard composer.draft_utf8_len > 0, let bytes = composer.draft_utf8 else { return }
        textView.string = String(
            decoding: UnsafeBufferPointer(start: bytes, count: Int(composer.draft_utf8_len)),
            as: UTF8.self
        )
        submit()
    }

    private func submit() {
        pushDraft()
        let composer = seyal_app_composer(appHandle)
        guard composer.flags & UInt16(SEYAL_APP_COMPOSER_CAN_SUBMIT) != 0 else { return }
        let command = textView.string
        let snapshot = seyal_app_snapshot(appHandle)
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_SUBMIT_COMPOSER.rawValue)
        action.applySnapshotFence(snapshot)
        action.target_pty_generation = composer.epoch
        guard seyal_app_apply(appHandle, &action) == 0 else { return }
        var status = onSubmitComposer?(command) ?? -10
        if status == -12 {
            let payload = command.hasSuffix("\r") ? command : command + "\r"
            status = onSubmitRaw?(payload) ?? -10
        }
        if status == 0 {
            let submitted = seyal_app_composer(appHandle)
            if submitted.request_id != 0 {
                applyComposerResult(requestID: submitted.request_id, accepted: true)
            }
        }
        reconcile()
    }

    func applyComposerResult(requestID: UInt64, accepted: Bool) {
        let snapshot = seyal_app_snapshot(appHandle)
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
}

private func copyUTF8(_ pointer: UnsafePointer<UInt8>?, _ length: UInt32) -> String? {
    guard length > 0, let pointer else { return nil }
    return String(decoding: UnsafeBufferPointer(start: pointer, count: Int(length)), as: UTF8.self)
}
