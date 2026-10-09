import AppKit

/// Thin AppKit projection of the active Tab's Pane regions (#923) and split
/// dividers (#928).
///
/// Geometry, focus, ratios and live-surface placement come from Rust
/// (`seyal_app_pane_region` / `seyal_app_pane_divider`); this view only
/// positions frames. The one live terminal/Metal/composer container
/// (`liveContent`) sits in the LIVE region and is hidden when no region is
/// LIVE. Every leaf is a focusable region bound to its real PaneId; focusing
/// dispatches Rust `FOCUS_PANE` through `onFocusPane`. Dividers sit on the
/// Rust-projected line; dragging one forwards the raw pointer position in Tab
/// unit space as `MOVE_SPLIT_DIVIDER`, and Rust derives and clamps the ratio.
/// This view re-reads only the Pane projection, so a drag never rebuilds
/// Blocks on every mouse move.
@MainActor
final class PaneLayoutView: NSView {
    struct Region: Equatable {
        let paneLo: UInt64
        let paneHi: UInt64
        /// Unit space, origin top-left (this view is flipped).
        let rect: CGRect
        let focused: Bool
        let live: Bool
        let zoomed: Bool
        let occluded: Bool
        let title: String

        /// Everything but geometry: a change here rebuilds region views.
        var identity: Region {
            Region(
                paneLo: paneLo, paneHi: paneHi, rect: .zero, focused: focused, live: live,
                zoomed: zoomed, occluded: occluded, title: title
            )
        }
    }

    struct Divider: Equatable {
        let leadingLo: UInt64
        let leadingHi: UInt64
        /// Side-by-side Split: the divider is vertical and drags horizontally.
        let vertical: Bool
        /// Unit rect of the whole area the Split divides.
        let area: CGRect
        /// Unit position of the divider line (Rust-projected).
        let line: CGPoint
        let ratio: CGFloat

        var identity: Divider {
            Divider(
                leadingLo: leadingLo, leadingHi: leadingHi, vertical: vertical,
                area: .zero, line: .zero, ratio: 0
            )
        }
    }

    /// Container for the single live Pane surface; owned by the caller's
    /// constraints internally, positioned here by frame.
    let liveContent = NSView()
    var onFocusPane: ((UInt64, UInt64) -> Void)?
    /// Rust application root the projection is read from; 0 until wired.
    var appHandle: UInt64 = 0
    private(set) var regions: [Region] = []
    private(set) var dividers: [Divider] = []
    private var regionViews: [PaneRegionView] = []
    private var dividerViews: [PaneDividerView] = []
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

    /// Re-read every region for `paneCount` leaves and its `paneCount - 1`
    /// dividers from Rust and apply them. A zero-size row means Rust no longer
    /// has that index (stale count); reading stops rather than guessing.
    func reconcile(paneCount: Int) {
        apply(
            Self.readRegions(appHandle: appHandle, paneCount: paneCount),
            dividers: Self.readDividers(appHandle: appHandle, count: max(paneCount - 1, 0))
        )
    }

    /// Forward a drag to Rust as the raw pointer coordinate along the
    /// divider's axis, normalised to Tab unit space. Rust owns the ratio.
    private func moveDivider(_ divider: Divider, to point: CGPoint) {
        guard bounds.width > 0, bounds.height > 0 else { return }
        let position = divider.vertical ? point.x / bounds.width : point.y / bounds.height
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(SEYAL_APP_ACTION_MOVE_SPLIT_DIVIDER.rawValue)
        action.target_execution_lo = divider.leadingLo
        action.target_execution_hi = divider.leadingHi
        action.reserved = Float(position).bitPattern
        guard seyal_app_apply(appHandle, &action) == 0 else { return }
        reconcile(paneCount: Int(seyal_app_shell(appHandle).pane_count))
    }

    private static func readDividers(appHandle: UInt64, count: Int) -> [Divider] {
        var dividers: [Divider] = []
        for index in 0..<count {
            let raw = seyal_app_pane_divider(appHandle, UInt32(index))
            guard raw.size != 0 else { break }
            dividers.append(
                Divider(
                    leadingLo: raw.leading_pane_lo,
                    leadingHi: raw.leading_pane_hi,
                    vertical: raw.axis == 0,
                    area: CGRect(
                        x: CGFloat(raw.x),
                        y: CGFloat(raw.y),
                        width: CGFloat(raw.width),
                        height: CGFloat(raw.height)
                    ),
                    line: CGPoint(x: CGFloat(raw.line_x), y: CGFloat(raw.line_y)),
                    ratio: CGFloat(raw.ratio)
                )
            )
        }
        return dividers
    }

    private static func readRegions(appHandle: UInt64, paneCount: Int) -> [Region] {
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
                    zoomed: raw.flags & UInt16(SEYAL_APP_PANE_REGION_ZOOMED) != 0,
                    occluded: raw.flags & UInt16(SEYAL_APP_PANE_REGION_OCCLUDED) != 0,
                    title: title
                )
            )
        }
        return regions
    }

    func apply(_ next: [Region], dividers nextDividers: [Divider]) {
        guard next != regions || nextDividers != dividers else { return }
        let rebuild = next.map(\.identity) != regions.map(\.identity)
            || nextDividers.map(\.identity) != dividers.map(\.identity)
        regions = next
        dividers = nextDividers
        needsLayout = true
        // Geometry-only change (a ratio drag): keep the views, including the
        // divider being dragged, and just relayout.
        guard rebuild else { return }
        regionViews.forEach { $0.removeFromSuperview() }
        let visible = next.filter { !$0.occluded }
        let split = visible.count > 1
        regionViews = next.enumerated().map { index, region in
            let view = PaneRegionView(region: region, index: index, split: split)
            view.isHidden = region.occluded
            view.onFocus = { [weak self] in
                self?.onFocusPane?(region.paneLo, region.paneHi)
            }
            if let theme { view.apply(theme: theme) }
            // Regions sit under the live container so its content stays on top.
            addSubview(view, positioned: .below, relativeTo: liveContent)
            return view
        }
        liveContent.isHidden = !next.contains(where: \.live)
        dividerViews.forEach { $0.removeFromSuperview() }
        dividerViews = nextDividers.enumerated().map { index, divider in
            let view = PaneDividerView(index: index, vertical: divider.vertical)
            view.onDrag = { [weak self] point in
                self?.moveDivider(divider, to: point)
            }
            if let theme { view.apply(theme: theme) }
            // Dividers sit above the live container so its edge stays draggable.
            addSubview(view, positioned: .above, relativeTo: liveContent)
            return view
        }
    }

    func apply(theme: NativeTheme) {
        self.theme = theme
        regionViews.forEach { $0.apply(theme: theme) }
        dividerViews.forEach { $0.apply(theme: theme) }
    }

    override func layout() {
        super.layout()
        for (view, region) in zip(regionViews, regions) {
            view.frame = scaled(region.rect)
        }
        // Before the first projection the live container fills the Tab so the
        // single production Pane can present and bind.
        let live = regions.first(where: \.live)?.rect ?? CGRect(x: 0, y: 0, width: 1, height: 1)
        let inset: CGFloat = regions.filter { !$0.occluded }.count > 1 ? PaneRegionView.borderWidth : 0
        liveContent.frame = scaled(live).insetBy(dx: inset, dy: inset)
        for (view, divider) in zip(dividerViews, dividers) {
            // Centre a fixed-thickness hit zone on the Rust-projected line.
            let area = scaled(divider.area)
            let half = PaneDividerView.thickness / 2
            view.ratio = divider.ratio
            view.frame = divider.vertical
                ? CGRect(x: (divider.line.x * bounds.width).rounded() - half, y: area.minY,
                         width: PaneDividerView.thickness, height: area.height)
                : CGRect(x: area.minX, y: (divider.line.y * bounds.height).rounded() - half,
                         width: area.width, height: PaneDividerView.thickness)
        }
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

/// One Split divider hit zone. Dragging reports the raw pointer position in
/// the parent's (flipped) coordinates; Rust owns the ratio and layout.
@MainActor
private final class PaneDividerView: NSView {
    static let thickness: CGFloat = 6

    var onDrag: ((CGPoint) -> Void)?
    var ratio: CGFloat = 0.5 {
        didSet { setAccessibilityValue(String(format: "%.2f", ratio)) }
    }
    private let vertical: Bool

    init(index: Int, vertical: Bool) {
        self.vertical = vertical
        super.init(frame: .zero)
        wantsLayer = true
        setAccessibilityElement(true)
        setAccessibilityRole(.splitter)
        setAccessibilityIdentifier("seyal-pane-divider-\(index)")
        setAccessibilityOrientation(vertical ? .vertical : .horizontal)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("PaneDividerView is programmatic")
    }

    func apply(theme: NativeTheme) {
        layer?.backgroundColor = NSColor.clear.cgColor
    }

    override func resetCursorRects() {
        addCursorRect(bounds, cursor: vertical ? .resizeLeftRight : .resizeUpDown)
    }

    override func mouseDown(with event: NSEvent) {
        // Swallow so the Pane beneath does not take focus from a drag start.
    }

    override func mouseDragged(with event: NSEvent) {
        guard let parent = superview else { return }
        onDrag?(parent.convert(event.locationInWindow, from: nil))
    }
}
