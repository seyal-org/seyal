import AppKit

/// Single-line palette query. Forwards every stroke to the Rust keybinding
/// router (ADR-015). A consumed WorkspaceCommand is already applied; this
/// view does not choose `command_palette.close` or any other command.
@MainActor
private final class PaletteQueryEditor: NSTextView {
    var routeKeystroke: ((NSEvent) -> KeybindingStrokeNormalizer.RouteResult)?
    var onRouted: (() -> Void)?
    var onMoveSelection: ((Int32) -> Void)?
    var onRunSelection: (() -> Void)?

    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        switch routeKeystroke?(event) ?? .fallsThrough {
        case .consumed:
            onRouted?()
            return true
        case .nativeCommand, .fallsThrough:
            return super.performKeyEquivalent(with: event)
        }
    }

    override func keyDown(with event: NSEvent) {
        let marked = hasMarkedText()
        switch routeKeystroke?(event) ?? .fallsThrough {
        case .consumed:
            onRouted?()
            return
        case .nativeCommand:
            super.keyDown(with: event)
            return
        case .fallsThrough:
            if marked {
                super.keyDown(with: event)
                return
            }
        }
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        if flags.contains(.command) {
            super.keyDown(with: event)
            return
        }
        switch event.specialKey {
        case .some(.upArrow):
            onMoveSelection?(-1)
        case .some(.downArrow):
            onMoveSelection?(1)
        case .some(.carriageReturn), .some(.newline), .some(.enter):
            onRunSelection?()
        case .some(.tab), .some(.backTab):
            break
        default:
            if event.keyCode == 53 || event.charactersIgnoringModifiers == "\u{1b}" {
                return
            }
            super.keyDown(with: event)
        }
    }
}

/// Thin projection of the Rust global command palette (#932).
/// Open/closed, query, rows and selection are read from
/// `seyal_app_palette`; keys and clicks only dispatch actions. This view
/// keeps no command list of its own and invents no action the host has not
/// been told about by Rust.
@MainActor
final class CommandPaletteOverlayView: NSView, NSTextViewDelegate {
    /// Rust state changed; the host should reconcile chrome.
    var onChanged: (() -> Void)?
    /// Overlay closed (run/escape/click-outside); the host should restore
    /// focus to whatever owned it before the palette opened.
    var onDismissed: (() -> Void)?

    private let appHandle: UInt64
    private let card = NSView()
    private let query = PaletteQueryEditor()
    private let placeholder = NSTextField(labelWithString: "Type a command...")
    private let rows = NSStackView()
    private var rowViews: [(label: NSTextField, category: NSTextField)] = []
    private var theme: NativeTheme?
    private var wasOpen = false
    private var selected = 0
    private var cardWidth: NSLayoutConstraint!

    init(appHandle: UInt64) {
        self.appHandle = appHandle
        super.init(frame: .zero)
        translatesAutoresizingMaskIntoConstraints = false
        isHidden = true
        setAccessibilityIdentifier("seyal-command-palette-scrim")
        // Accessible (not merely decorative): XCUIAutomation locates it by
        // identifier for click-outside-to-close coverage, matching every
        // other overlay root in this host (#933's history overlay, etc.).
        setAccessibilityElement(true)
        setAccessibilityRole(.group)

        card.translatesAutoresizingMaskIntoConstraints = false
        card.wantsLayer = true
        card.layer?.cornerRadius = 12
        card.layer?.cornerCurve = .continuous
        card.layer?.masksToBounds = true
        card.setAccessibilityIdentifier("seyal-command-palette")
        card.setAccessibilityElement(true)
        card.setAccessibilityRole(.group)

        query.delegate = self
        query.isRichText = false
        query.isAutomaticQuoteSubstitutionEnabled = false
        query.isAutomaticDashSubstitutionEnabled = false
        query.isAutomaticTextReplacementEnabled = false
        query.drawsBackground = false
        query.focusRingType = .none
        query.font = .systemFont(ofSize: 15, weight: .regular)
        query.textContainer?.lineFragmentPadding = 4
        query.textContainer?.widthTracksTextView = true
        query.textContainer?.maximumNumberOfLines = 1
        query.textContainerInset = NSSize(width: 0, height: 2)
        query.isHorizontallyResizable = false
        query.isVerticallyResizable = false
        query.translatesAutoresizingMaskIntoConstraints = false
        query.setAccessibilityIdentifier("seyal-command-palette-query")
        query.setAccessibilityElement(true)
        query.routeKeystroke = { [weak self] event in
            self?.routePaletteKeystroke(event) ?? .fallsThrough
        }
        query.onRouted = { [weak self] in self?.onChanged?() }
        query.onMoveSelection = { [weak self] delta in self?.move(by: delta) }
        query.onRunSelection = { [weak self] in self?.run() }

        placeholder.font = .systemFont(ofSize: 15, weight: .regular)
        placeholder.textColor = .placeholderTextColor
        placeholder.translatesAutoresizingMaskIntoConstraints = false
        placeholder.setAccessibilityElement(false)

        rows.orientation = .vertical
        rows.alignment = .leading
        rows.spacing = 1
        rows.translatesAutoresizingMaskIntoConstraints = false
        rows.setAccessibilityIdentifier("seyal-command-palette-rows")

        card.addSubview(placeholder)
        card.addSubview(query)
        card.addSubview(rows)
        addSubview(card)

        cardWidth = card.widthAnchor.constraint(equalToConstant: 560)
        NSLayoutConstraint.activate([
            card.centerXAnchor.constraint(equalTo: centerXAnchor),
            card.topAnchor.constraint(equalTo: topAnchor, constant: 96),
            card.widthAnchor.constraint(lessThanOrEqualTo: widthAnchor, constant: -48),
            cardWidth,
            query.leadingAnchor.constraint(equalTo: card.leadingAnchor, constant: 14),
            query.trailingAnchor.constraint(equalTo: card.trailingAnchor, constant: -14),
            query.topAnchor.constraint(equalTo: card.topAnchor, constant: 12),
            query.heightAnchor.constraint(equalToConstant: 24),
            placeholder.leadingAnchor.constraint(equalTo: query.leadingAnchor, constant: 6),
            placeholder.centerYAnchor.constraint(equalTo: query.centerYAnchor),
            rows.leadingAnchor.constraint(equalTo: card.leadingAnchor, constant: 14),
            rows.trailingAnchor.constraint(equalTo: card.trailingAnchor, constant: -14),
            rows.topAnchor.constraint(equalTo: query.bottomAnchor, constant: 12),
            rows.bottomAnchor.constraint(equalTo: card.bottomAnchor, constant: -10),
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("CommandPaletteOverlayView is programmatic")
    }

    var isOpen: Bool { !isHidden }

    func focusQuery() {
        window?.makeFirstResponder(query)
    }

    func apply(theme: NativeTheme) {
        self.theme = theme
        query.textColor = theme.text
        query.insertionPointColor = theme.text
        placeholder.textColor = theme.muted
        paint()
    }

    func reconcile() {
        let palette = seyal_app_palette(appHandle)
        let open = palette.flags & UInt16(SEYAL_APP_PALETTE_OPEN) != 0
        isHidden = !open
        guard open else {
            if wasOpen {
                wasOpen = false
                onDismissed?()
            }
            return
        }
        let text = copyUTF8(palette.query_utf8, palette.query_utf8_len) ?? ""
        if query.string != text {
            query.string = text
        }
        placeholder.isHidden = !query.string.isEmpty
        selected = Int(palette.selected)
        rebuildRows(count: Int(palette.row_count))
        setAccessibilityValue("\(palette.row_count)")
        if !wasOpen {
            wasOpen = true
            focusQuery()
        }
        paint()
    }

    // MARK: NSTextViewDelegate

    func textDidChange(_ notification: Notification) {
        placeholder.isHidden = !query.string.isEmpty
        dispatch(kind: UInt16(SEYAL_APP_ACTION_SET_PALETTE_QUERY.rawValue), payload: query.string)
    }

    /// Rust owns the match. Swift forwards the normalized stroke and does not
    /// choose the WorkspaceCommand. Palette row motion runs only after a miss.
    private func routePaletteKeystroke(_ event: NSEvent) -> KeybindingStrokeNormalizer.RouteResult {
        KeybindingStrokeNormalizer.route(
            appHandle: appHandle,
            event: event,
            composerFocused: false,
            compositionActive: query.hasMarkedText()
        )
    }

    // MARK: Actions

    private func move(by delta: Int32) {
        dispatch(kind: UInt16(SEYAL_APP_ACTION_MOVE_PALETTE_SELECTION.rawValue), reserved: UInt32(bitPattern: delta))
    }

    private func run() {
        dispatch(kind: UInt16(SEYAL_APP_ACTION_RUN_PALETTE.rawValue))
    }

    private func close() {
        dispatch(kind: UInt16(SEYAL_APP_ACTION_CLOSE_PALETTE.rawValue))
    }

    /// AppKit hit-tests to the deepest view under the cursor; a click that
    /// lands on `card` (or a subview without its own handling) never
    /// reaches this override, so only a genuine click outside the card
    /// closes the palette. `card` itself has no mouseDown handling, so a
    /// click on its empty background is a safe no-op rather than a close.
    override func mouseDown(with event: NSEvent) {
        close()
    }

    @objc private func rowClicked(_ recognizer: NSClickGestureRecognizer) {
        guard let label = recognizer.view as? NSTextField else { return }
        let delta = Int32(label.tag - selected)
        if delta != 0 {
            move(by: delta)
        }
        run()
    }

    private func dispatch(kind: UInt16, reserved: UInt32 = 0) {
        dispatch(kind: kind, payloadBytes: nil, reserved: reserved)
    }

    private func dispatch(kind: UInt16, payload: String, reserved: UInt32 = 0) {
        dispatch(kind: kind, payloadBytes: Data(payload.utf8), reserved: reserved)
    }

    private func dispatch(kind: UInt16, payloadBytes: Data?, reserved: UInt32 = 0) {
        let snapshot = seyal_app_snapshot(appHandle)
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = kind
        action.applySnapshotFence(snapshot)
        action.reserved = reserved
        if let payloadBytes {
            payloadBytes.withUnsafeBytes { buffer in
                action.payload = buffer.bindMemory(to: UInt8.self).baseAddress
                action.payload_len = UInt32(payloadBytes.count)
                _ = seyal_app_apply(appHandle, &action)
            }
        } else {
            action.payload = nil
            action.payload_len = 0
            _ = seyal_app_apply(appHandle, &action)
        }
        onChanged?()
    }

    /// Rust decides whether the palette is reachable at all (it always is:
    /// the fence check passes even while unbound). A rejected open leaves
    /// the overlay untouched.
    func requestOpen() {
        let snapshot = seyal_app_snapshot(appHandle)
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_OPEN_PALETTE.rawValue)
        action.applySnapshotFence(snapshot)
        guard seyal_app_apply(appHandle, &action) == 0 else { return }
        onChanged?()
    }

    // MARK: Projection

    private func rebuildRows(count: Int) {
        while rowViews.count > count {
            let removed = rowViews.removeLast()
            rows.removeArrangedSubview(removed.label)
            removed.label.removeFromSuperview()
        }
        while rowViews.count < count {
            let label = NSTextField(labelWithString: "")
            label.font = .systemFont(ofSize: 13, weight: .regular)
            label.lineBreakMode = .byTruncatingTail
            label.wantsLayer = true
            label.layer?.cornerRadius = 6
            label.translatesAutoresizingMaskIntoConstraints = false

            let category = NSTextField(labelWithString: "")
            category.font = .systemFont(ofSize: 11, weight: .medium)
            category.alignment = .right
            category.translatesAutoresizingMaskIntoConstraints = false
            label.addSubview(category)
            NSLayoutConstraint.activate([
                category.trailingAnchor.constraint(equalTo: label.trailingAnchor, constant: -10),
                category.centerYAnchor.constraint(equalTo: label.centerYAnchor),
            ])

            label.addGestureRecognizer(
                NSClickGestureRecognizer(target: self, action: #selector(rowClicked(_:)))
            )
            rows.addArrangedSubview(label)
            label.widthAnchor.constraint(equalTo: rows.widthAnchor).isActive = true
            label.heightAnchor.constraint(equalToConstant: 28).isActive = true
            rowViews.append((label, category))
        }
        for (index, view) in rowViews.enumerated() {
            let row = seyal_app_palette_row(appHandle, UInt32(index))
            view.label.tag = index
            view.label.stringValue = copyUTF8(row.title, row.title_len) ?? ""
            view.category.stringValue = copyUTF8(row.detail, row.detail_len) ?? ""
            view.label.setAccessibilityIdentifier("seyal-command-palette-row-\(index)")
            view.label.setAccessibilityElement(true)
            // NSTextField(labelWithString:) does not bridge stringValue to
            // accessibilityLabel automatically; XCUIElement.label reads
            // nothing without this (see #933's ComposerHistoryOverlayView).
            view.label.setAccessibilityLabel(view.label.stringValue)
            view.label.setAccessibilityValue(index == selected ? "selected" : "")
        }
    }

    private func paint() {
        guard let theme else { return }
        layer?.backgroundColor = theme.canvas.withAlphaComponent(0.35).cgColor
        card.layer?.backgroundColor = theme.elevated.withAlphaComponent(0.98).cgColor
        card.layer?.borderWidth = 1
        card.layer?.borderColor = theme.seam.cgColor
        for (index, view) in rowViews.enumerated() {
            let isSelected = index == selected
            view.label.textColor = isSelected ? theme.text : theme.secondary
            view.category.textColor = theme.muted
            view.label.layer?.backgroundColor = isSelected
                ? theme.accent.withAlphaComponent(0.16).cgColor
                : NSColor.clear.cgColor
        }
    }

    private func copyUTF8(_ pointer: UnsafePointer<UInt8>?, _ length: UInt32) -> String? {
        guard length > 0, let pointer else { return nil }
        return String(decoding: UnsafeBufferPointer(start: pointer, count: Int(length)), as: UTF8.self)
    }
}
