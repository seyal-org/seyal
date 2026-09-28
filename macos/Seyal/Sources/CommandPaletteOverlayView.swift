import AppKit

/// Thin projection of the Rust global command palette (#932).
/// Open/closed, query, rows and selection are read from
/// `seyal_app_palette`; keys and clicks only dispatch actions. This view
/// keeps no command list of its own and invents no action the host has not
/// been told about by Rust.
@MainActor
final class CommandPaletteOverlayView: NSView, NSTextFieldDelegate {
    /// Rust state changed; the host should reconcile chrome.
    var onChanged: (() -> Void)?
    /// Overlay closed (run/escape/click-outside); the host should restore
    /// focus to whatever owned it before the palette opened.
    var onDismissed: (() -> Void)?

    private let appHandle: UInt64
    private let card = NSView()
    private let query = NSTextField(string: "")
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
        query.isBordered = false
        query.drawsBackground = false
        query.focusRingType = .none
        query.font = .systemFont(ofSize: 15, weight: .regular)
        query.translatesAutoresizingMaskIntoConstraints = false
        query.setAccessibilityIdentifier("seyal-command-palette-query")
        query.cell?.sendsActionOnEndEditing = false

        rows.orientation = .vertical
        rows.alignment = .leading
        rows.spacing = 1
        rows.translatesAutoresizingMaskIntoConstraints = false
        rows.setAccessibilityIdentifier("seyal-command-palette-rows")

        card.addSubview(query)
        card.addSubview(rows)
        addSubview(card)

        cardWidth = card.widthAnchor.constraint(equalToConstant: 560)
        NSLayoutConstraint.activate([
            card.centerXAnchor.constraint(equalTo: centerXAnchor),
            card.topAnchor.constraint(equalTo: topAnchor, constant: 96),
            card.widthAnchor.constraint(lessThanOrEqualTo: widthAnchor, constant: -48),
            cardWidth,
            query.leadingAnchor.constraint(equalTo: card.leadingAnchor, constant: 18),
            query.trailingAnchor.constraint(equalTo: card.trailingAnchor, constant: -18),
            query.topAnchor.constraint(equalTo: card.topAnchor, constant: 16),
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
        if query.stringValue != text {
            query.stringValue = text
        }
        let isGoto = palette.flags & UInt16(SEYAL_APP_PALETTE_GOTO) != 0
        let truncated = palette.flags & UInt16(SEYAL_APP_PALETTE_TRUNCATED) != 0
        if isGoto {
            let scope = gotoScopeName(UInt8(palette.reserved & 0xff))
            query.placeholderString = truncated
                ? "Go to \(scope) (truncated)…"
                : "Go to \(scope)…"
        } else {
            query.placeholderString = "Type a command..."
        }
        selected = Int(palette.selected)
        rebuildRows(count: Int(palette.row_count))
        setAccessibilityValue(truncated ? "\(palette.row_count) truncated" : "\(palette.row_count)")
        if !wasOpen {
            wasOpen = true
            focusQuery()
        }
        paint()
    }

    // MARK: NSTextFieldDelegate

    func controlTextDidChange(_ notification: Notification) {
        dispatch(kind: UInt16(SEYAL_APP_ACTION_SET_PALETTE_QUERY.rawValue), payload: query.stringValue)
    }

    func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        switch selector {
        case #selector(NSResponder.moveUp(_:)):
            move(by: -1)
        case #selector(NSResponder.moveDown(_:)):
            move(by: 1)
        case #selector(NSResponder.insertNewline(_:)):
            run()
        case #selector(NSResponder.cancelOperation(_:)), #selector(NSStandardKeyBindingResponding.complete(_:)):
            // NSTextField's field editor reports Escape as either selector.
            close()
        case #selector(NSResponder.insertTab(_:)):
            // Scope modes stay separated: Tab cycles Workspaces→Tabs→Panes→Sessions.
            cycleGotoScope()
        default:
            return false
        }
        return true
    }

    // MARK: Actions

    private func move(by delta: Int32) {
        dispatchAction(
            kind: UInt16(SEYAL_APP_ACTION_MOVE_PALETTE_SELECTION.rawValue),
            reserved: UInt32(bitPattern: delta)
        )
    }

    private func run() {
        // Echo the frozen row's ResourceAddress when present (SPEC-022 R7.2).
        // Verb/chrome rows have address_len == 0; Rust runs the stored command.
        let row = seyal_app_palette_row(appHandle, UInt32(selected))
        if row.address_len > 0 {
            var payload = Data()
            var version = row.address_version.littleEndian
            var kind = row.address_kind.littleEndian
            payload.append(Data(bytes: &version, count: 2))
            payload.append(Data(bytes: &kind, count: 2))
            withUnsafeBytes(of: row.address_bytes) { bytes in
                payload.append(contentsOf: bytes.prefix(Int(row.address_len)))
            }
            dispatchAction(
                kind: UInt16(SEYAL_APP_ACTION_RUN_PALETTE.rawValue),
                payloadBytes: payload
            )
        } else {
            dispatchAction(kind: UInt16(SEYAL_APP_ACTION_RUN_PALETTE.rawValue))
        }
    }

    private func close() {
        dispatchAction(kind: UInt16(SEYAL_APP_ACTION_CLOSE_PALETTE.rawValue))
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

    private func dispatch(kind: UInt16, payload: String? = nil, reserved: UInt32 = 0) {
        let bytes = payload.map { Data($0.utf8) }
        dispatchAction(kind: kind, payloadBytes: bytes, reserved: reserved)
    }

    /// Byte-payload apply path. Named distinctly from `dispatch(payload:)` so
    /// defaulted call sites are not ambiguous under current Swift overload rules.
    private func dispatchAction(
        kind: UInt16,
        payloadBytes: Data? = nil,
        reserved: UInt32 = 0
    ) {
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

    /// Navigation-only goto surface (SPEC-022 N4). Reuses this overlay;
    /// default scope is Panes.
    func requestOpenGoto(scope: SeyalAppGotoScope = SEYAL_APP_GOTO_PANES) {
        let snapshot = seyal_app_snapshot(appHandle)
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_OPEN_GOTO.rawValue)
        action.reserved = UInt32(scope.rawValue)
        action.applySnapshotFence(snapshot)
        guard seyal_app_apply(appHandle, &action) == 0 else { return }
        onChanged?()
    }

    private func cycleGotoScope() {
        let palette = seyal_app_palette(appHandle)
        guard palette.flags & UInt16(SEYAL_APP_PALETTE_GOTO) != 0 else { return }
        let next = (Int(palette.reserved & 0xff) + 1) % 4
        dispatchAction(
            kind: UInt16(SEYAL_APP_ACTION_SET_GOTO_SCOPE.rawValue),
            reserved: UInt32(next)
        )
    }

    private func gotoScopeName(_ value: UInt8) -> String {
        switch value {
        case UInt8(SEYAL_APP_GOTO_WORKSPACES.rawValue): return "Workspaces"
        case UInt8(SEYAL_APP_GOTO_TABS.rawValue): return "Tabs"
        case UInt8(SEYAL_APP_GOTO_PANES.rawValue): return "Panes"
        case UInt8(SEYAL_APP_GOTO_SESSIONS.rawValue): return "Sessions"
        default: return "…"
        }
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
