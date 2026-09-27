import AppKit

/// Thin AppKit projection of the active Tab's Pane regions (#923).
///
/// Geometry, focus and live-surface placement come from Rust
/// (`seyal_app_pane_region`); this view only positions frames. The one live
/// terminal/Metal/composer container (`liveContent`) sits in the LIVE region and
/// is hidden when no region is LIVE. Every leaf is a focusable region bound to
/// its real PaneId; focusing dispatches Rust `FOCUS_PANE` through `onFocusPane`.
@MainActor
final class PaneLayoutView: NSView {
    struct Region: Equatable {
        let paneLo: UInt64
        let paneHi: UInt64
        /// Unit space, origin top-left (this view is flipped).
        let rect: CGRect
        let focused: Bool
        let live: Bool
        let title: String
    }

    /// Container for the single live Pane surface; owned by the caller's
    /// constraints internally, positioned here by frame.
    let liveContent = NSView()
    var onFocusPane: ((UInt64, UInt64) -> Void)?
    private(set) var regions: [Region] = []
    private var regionViews: [PaneRegionView] = []
    private var theme: NativeTheme?

    override var isFlipped: Bool { true }

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        liveContent.translatesAutoresizingMaskIntoConstraints = true
        // Track the Tab's bounds until the first projection sets a region frame.
        liveContent.autoresizingMask = [.width, .height]
        addSubview(liveContent)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("PaneLayoutView is programmatic")
    }

    /// Read every region for `paneCount` leaves. A zero-size row means Rust
    /// no longer has that index (stale count); stop rather than guess.
    static func readRegions(appHandle: UInt64, paneCount: Int) -> [Region] {
        var regions: [Region] = []
        for index in 0..<paneCount {
            let raw = seyal_app_pane_region(appHandle, UInt32(index))
            guard raw.size != 0 else { break }
            let row = seyal_app_shell_row(appHandle, UInt16(SEYAL_APP_ROW_PANE), UInt32(index))
            let title = row.title.flatMap { bytes in
                String(
                    decoding: UnsafeBufferPointer(start: bytes, count: Int(row.title_len)),
                    as: UTF8.self
                )
            } ?? "Pane"
            regions.append(
                Region(
                    paneLo: raw.pane_lo,
                    paneHi: raw.pane_hi,
                    rect: CGRect(
                        x: CGFloat(raw.x),
                        y: CGFloat(raw.y),
                        width: CGFloat(raw.width),
                        height: CGFloat(raw.height)
                    ),
                    focused: raw.flags & UInt16(SEYAL_APP_PANE_REGION_FOCUSED) != 0,
                    live: raw.flags & UInt16(SEYAL_APP_PANE_REGION_LIVE) != 0,
                    title: title
                )
            )
        }
        return regions
    }

    func apply(_ next: [Region]) {
        guard next != regions else { return }
        regions = next
        regionViews.forEach { $0.removeFromSuperview() }
        let split = next.count > 1
        regionViews = next.enumerated().map { index, region in
            let view = PaneRegionView(region: region, index: index, split: split)
            view.onFocus = { [weak self] in
                self?.onFocusPane?(region.paneLo, region.paneHi)
            }
            if let theme { view.apply(theme: theme) }
            // Regions sit under the live container so its content stays on top.
            addSubview(view, positioned: .below, relativeTo: liveContent)
            return view
        }
        liveContent.isHidden = !next.contains(where: \.live)
        needsLayout = true
    }

    func apply(theme: NativeTheme) {
        self.theme = theme
        regionViews.forEach { $0.apply(theme: theme) }
    }

    override func layout() {
        super.layout()
        for (view, region) in zip(regionViews, regions) {
            view.frame = scaled(region.rect)
        }
        // Before the first projection the live container fills the Tab so the
        // single production Pane can present and bind.
        let live = regions.first(where: \.live)?.rect ?? CGRect(x: 0, y: 0, width: 1, height: 1)
        let inset: CGFloat = regions.count > 1 ? PaneRegionView.borderWidth : 0
        liveContent.frame = scaled(live).insetBy(dx: inset, dy: inset)
    }

    private func scaled(_ unit: CGRect) -> CGRect {
        CGRect(
            x: (unit.minX * bounds.width).rounded(),
            y: (unit.minY * bounds.height).rounded(),
            width: (unit.width * bounds.width).rounded(),
            height: (unit.height * bounds.height).rounded()
        )
    }
}

/// One Pane leaf. In a split it draws a focus-accented border and, when not
/// live, the Pane title; clicking an unfocused region requests Rust focus.
@MainActor
private final class PaneRegionView: NSView {
    static let borderWidth: CGFloat = 1

    var onFocus: (() -> Void)?
    private let region: PaneLayoutView.Region
    private let split: Bool
    private let title = NSTextField(labelWithString: "")

    init(region: PaneLayoutView.Region, index: Int, split: Bool) {
        self.region = region
        self.split = split
        super.init(frame: .zero)
        wantsLayer = true
        setAccessibilityElement(true)
        setAccessibilityRole(.group)
        setAccessibilityIdentifier("seyal-pane-region-\(index)")
        setAccessibilityLabel(region.title)
        setAccessibilityValue(region.focused ? "focused" : "")
        title.stringValue = region.title
        title.translatesAutoresizingMaskIntoConstraints = false
        title.isHidden = !split || region.live
        addSubview(title)
        NSLayoutConstraint.activate([
            title.centerXAnchor.constraint(equalTo: centerXAnchor),
            title.centerYAnchor.constraint(equalTo: centerYAnchor),
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("PaneRegionView is programmatic")
    }

    func apply(theme: NativeTheme) {
        layer?.borderWidth = split ? Self.borderWidth : 0
        layer?.borderColor = (region.focused ? theme.accent : theme.seam).cgColor
        title.textColor = theme.muted
        title.font = .systemFont(ofSize: theme.uiFontSize, weight: .regular)
    }

    override func mouseDown(with event: NSEvent) {
        guard !region.focused else {
            super.mouseDown(with: event)
            return
        }
        onFocus?()
    }

    override func accessibilityPerformPress() -> Bool {
        guard !region.focused else { return false }
        onFocus?()
        return true
    }
}
