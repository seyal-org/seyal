import AppKit

/// Thin ADR-026 session affordance. Copy and decisions stay Rust-owned; this
/// host only renders `seyal_bridge_session_*` and forwards button presses.
@MainActor
final class SessionBootstrapBanner: NSView {
    private let titleLabel = NSTextField(labelWithString: "")
    private let bodyLabel = NSTextField(wrappingLabelWithString: "")
    private let primary = NSButton(title: "", target: nil, action: nil)
    private let dismiss = NSButton(title: "", target: nil, action: nil)
    private var lastKind: UInt8 = 0
    private var lastRevision: UInt64 = 0

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        translatesAutoresizingMaskIntoConstraints = false
        wantsLayer = true
        layer?.cornerRadius = 8
        layer?.cornerCurve = .continuous
        isHidden = true
        setAccessibilityIdentifier("seyal-session-bootstrap")
        setAccessibilityRole(.group)
        setAccessibilityElement(true)

        titleLabel.font = .systemFont(ofSize: 13, weight: .semibold)
        titleLabel.setAccessibilityIdentifier("seyal-session-bootstrap-title")
        bodyLabel.font = .systemFont(ofSize: 11, weight: .regular)
        bodyLabel.setAccessibilityIdentifier("seyal-session-bootstrap-body")
        bodyLabel.maximumNumberOfLines = 6

        primary.target = self
        primary.action = #selector(primaryClicked)
        primary.bezelStyle = .rounded
        primary.setAccessibilityIdentifier("seyal-session-bootstrap-primary")
        dismiss.target = self
        dismiss.action = #selector(dismissClicked)
        dismiss.bezelStyle = .rounded
        dismiss.setAccessibilityIdentifier("seyal-session-bootstrap-dismiss")

        let buttons = NSStackView(views: [primary, dismiss])
        buttons.orientation = .horizontal
        buttons.spacing = 8
        buttons.translatesAutoresizingMaskIntoConstraints = false

        let stack = NSStackView(views: [titleLabel, bodyLabel, buttons])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 6
        stack.translatesAutoresizingMaskIntoConstraints = false
        addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 12),
            stack.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -12),
            stack.topAnchor.constraint(equalTo: topAnchor, constant: 10),
            stack.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -10),
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("SessionBootstrapBanner is programmatic")
    }

    func apply(theme: NativeTheme) {
        layer?.backgroundColor = theme.elevated.withAlphaComponent(0.92).cgColor
        titleLabel.textColor = theme.text
        bodyLabel.textColor = theme.muted
    }

    /// Refresh from the focused display client's Rust session surface.
    func reconcile() {
        let surface = seyal_bridge_session_surface()
        if surface.kind == lastKind, surface.revision == lastRevision, surface.kind != 0 {
            return
        }
        lastKind = surface.kind
        lastRevision = surface.revision
        guard surface.kind != 0 else {
            isHidden = true
            return
        }
        isHidden = false
        titleLabel.stringValue = copyString(0)
        if surface.kind == 2 {
            bodyLabel.stringValue = copyString(1)
            bodyLabel.isHidden = false
            primary.title = copyString(2)
            dismiss.title = copyString(3)
            dismiss.isHidden = false
        } else {
            bodyLabel.stringValue = ""
            bodyLabel.isHidden = true
            primary.title = copyString(4)
            dismiss.isHidden = true
        }
        setAccessibilityLabel(titleLabel.stringValue)
    }

    private func copyString(_ kind: UInt8) -> String {
        guard let ptr = seyal_bridge_session_copy(kind) else { return "" }
        return String(cString: ptr)
    }

    @objc private func primaryClicked() {
        if lastKind == 1 {
            _ = seyal_bridge_session_request()
        } else if lastKind == 2 {
            _ = seyal_bridge_session_consent(1)
        }
        reconcile()
    }

    @objc private func dismissClicked() {
        guard lastKind == 2 else { return }
        _ = seyal_bridge_session_consent(0)
        reconcile()
    }
}
