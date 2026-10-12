import AppKit

/// Thin projection of the Rust composer history overlay (#933).
/// Open/closed, query, rows and selection are read from
/// `seyal_app_composer_history`; keys and clicks only dispatch actions.
/// This view keeps no command list of its own.
@MainActor
final class ComposerHistoryOverlayView: NSView, NSTextFieldDelegate {
    /// Rust state changed; the host should reconcile chrome.
    var onChanged: (() -> Void)?
    /// Overlay closed (select/escape); the host should refocus the composer.
    var onDismissed: (() -> Void)?

    private let appHandle: UInt64
    private let query = NSTextField(string: "")
    private let rows = NSStackView()
    private var rowLabels: [NSTextField] = []
    private var theme: NativeTheme?
    private var wasOpen = false
    private var selected = 0

    init(appHandle: UInt64) {
        self.appHandle = appHandle
        super.init(frame: .zero)
        translatesAutoresizingMaskIntoConstraints = false
        wantsLayer = true
        layer?.cornerRadius = 10
        layer?.cornerCurve = .continuous
        layer?.masksToBounds = true
        isHidden = true
        setAccessibilityIdentifier("seyal-composer-history")
        setAccessibilityElement(true)
        setAccessibilityRole(.group)

        query.delegate = self
        query.isBordered = false
        query.drawsBackground = false
        query.focusRingType = .none
        query.font = .monospacedSystemFont(ofSize: 13, weight: .regular)
        query.translatesAutoresizingMaskIntoConstraints = false
        query.setAccessibilityIdentifier("seyal-composer-history-query")
        query.cell?.sendsActionOnEndEditing = false

        rows.orientation = .vertical
        rows.alignment = .leading
        rows.spacing = 2
        rows.translatesAutoresizingMaskIntoConstraints = false
        rows.setAccessibilityIdentifier("seyal-composer-history-rows")

        addSubview(query)
        addSubview(rows)
        NSLayoutConstraint.activate([
            query.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 14),
            query.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -14),
            query.topAnchor.constraint(equalTo: topAnchor, constant: 10),
            rows.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 8),
            rows.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -8),
            rows.topAnchor.constraint(equalTo: query.bottomAnchor, constant: 8),
            rows.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -8),
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("ComposerHistoryOverlayView is programmatic")
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
        let history = seyal_app_composer_history(appHandle)
        let open = history.flags & UInt16(SEYAL_APP_HISTORY_OPEN) != 0
        isHidden = !open
        guard open else {
            if wasOpen {
                wasOpen = false
                onDismissed?()
            }
            return
        }
        let text = copyUTF8(history.query_utf8, history.query_utf8_len) ?? ""
        if query.stringValue != text {
            query.stringValue = text
        }
        query.placeholderString = copyString(UInt16(SEYAL_APP_COPY_COMPOSER_HISTORY_PLACEHOLDER))
        selected = Int(history.selected)
        rebuildRows(count: Int(history.row_count))
        setAccessibilityValue("\(history.row_count)")
        if !wasOpen {
            wasOpen = true
            focusQuery()
        }
        paint()
    }

    // MARK: NSTextFieldDelegate

    func controlTextDidChange(_ notification: Notification) {
        dispatch(kind: UInt16(SEYAL_APP_ACTION_SET_COMPOSER_HISTORY_FILTER.rawValue), payload: query.stringValue)
    }

    func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        switch selector {
        case #selector(NSResponder.moveUp(_:)):
            move(by: -1)
        case #selector(NSResponder.moveDown(_:)):
            move(by: 1)
        case #selector(NSResponder.insertNewline(_:)):
            select()
        case #selector(NSResponder.cancelOperation(_:)), #selector(NSStandardKeyBindingResponding.complete(_:)):
            // NSTextField's field editor reports Escape as either selector.
            close()
        default:
            return false
        }
        return true
    }

    // MARK: Actions

    private func move(by delta: Int32) {
        dispatch(kind: UInt16(SEYAL_APP_ACTION_MOVE_COMPOSER_HISTORY_SELECTION.rawValue), reserved: UInt32(bitPattern: delta))
    }

    private func select() {
        let composer = seyal_app_composer(appHandle)
        dispatch(kind: UInt16(SEYAL_APP_ACTION_SELECT_COMPOSER_HISTORY.rawValue), epoch: composer.epoch)
    }

    private func close() {
        dispatch(kind: UInt16(SEYAL_APP_ACTION_CLOSE_COMPOSER_HISTORY.rawValue))
    }

    @objc private func rowClicked(_ recognizer: NSClickGestureRecognizer) {
        guard let label = recognizer.view as? NSTextField else { return }
        let delta = Int32(label.tag - selected)
        if delta != 0 {
            move(by: delta)
        }
        select()
    }

    private func dispatch(kind: UInt16, payload: String? = nil, reserved: UInt32 = 0, epoch: UInt64 = 0) {
        let snapshot = seyal_app_snapshot(appHandle)
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = kind
        action.applySnapshotFence(snapshot)
        action.reserved = reserved
        action.target_pty_generation = epoch
        let utf8 = Array((payload ?? "").utf8)
        utf8.withUnsafeBufferPointer { buffer in
            action.payload = payload == nil ? nil : buffer.baseAddress
            action.payload_len = payload == nil ? 0 : UInt32(buffer.count)
            _ = seyal_app_apply(appHandle, &action)
        }
        onChanged?()
    }

    // MARK: Projection

    private func rebuildRows(count: Int) {
        while rowLabels.count > count {
            let label = rowLabels.removeLast()
            rows.removeArrangedSubview(label)
            label.removeFromSuperview()
        }
        while rowLabels.count < count {
            let label = NSTextField(labelWithString: "")
            label.font = .monospacedSystemFont(ofSize: 13, weight: .regular)
            label.lineBreakMode = .byTruncatingTail
            label.wantsLayer = true
            label.layer?.cornerRadius = 6
            label.translatesAutoresizingMaskIntoConstraints = false
            label.addGestureRecognizer(
                NSClickGestureRecognizer(target: self, action: #selector(rowClicked(_:)))
            )
            rows.addArrangedSubview(label)
            label.widthAnchor.constraint(equalTo: rows.widthAnchor).isActive = true
            rowLabels.append(label)
        }
        for (index, label) in rowLabels.enumerated() {
            let row = seyal_app_history_row(appHandle, UInt32(index))
            label.tag = index
            label.stringValue = copyUTF8(row.title, row.title_len) ?? ""
            label.setAccessibilityIdentifier("seyal-composer-history-row-\(index)")
            label.setAccessibilityElement(true)
            label.setAccessibilityLabel(label.stringValue)
            label.setAccessibilityValue(index == selected ? "selected" : "")
        }
    }

    private func paint() {
        guard let theme else { return }
        layer?.backgroundColor = theme.elevated.cgColor
        layer?.borderWidth = 1
        layer?.borderColor = theme.accent.withAlphaComponent(0.45).cgColor
        for (index, label) in rowLabels.enumerated() {
            let isSelected = index == selected
            label.textColor = isSelected ? theme.text : theme.secondary
            label.layer?.backgroundColor = isSelected
                ? theme.accent.withAlphaComponent(0.18).cgColor
                : NSColor.clear.cgColor
        }
    }

    private func copyString(_ kind: UInt16) -> String {
        let row = seyal_app_copy(appHandle, kind)
        return copyUTF8(row.title, row.title_len) ?? ""
    }

    private func copyUTF8(_ pointer: UnsafePointer<UInt8>?, _ length: UInt32) -> String? {
        guard length > 0, let pointer else { return nil }
        return String(decoding: UnsafeBufferPointer(start: pointer, count: Int(length)), as: UTF8.self)
    }
}
