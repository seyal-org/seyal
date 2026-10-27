import AppKit

/// Thin AppKit projection of the active Tab's Pane regions (#923 / #936) and
/// split dividers (#928).
///
/// Geometry, focus, ratios and live-surface placement come from Rust
/// (`seyal_app_pane_region` / `seyal_app_pane_divider`); this view only
/// positions frames. The focused chrome container (`liveContent`) hosts
/// composer/transcript/primary Metal. Additional LIVE leaves host secondary
/// Metal surfaces via `installSecondaryLive`. Every leaf is a focusable region
/// bound to its real PaneId; focusing dispatches Rust `FOCUS_PANE` through
/// `onFocusPane`. Dividers sit on the Rust-projected line; dragging one
/// forwards the raw pointer position in Tab unit space as `MOVE_SPLIT_DIVIDER`,
/// and Rust derives and clamps the ratio. This view re-reads only the Pane
/// projection, so a drag never rebuilds Blocks on every mouse move.
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
        let recoveryFlags: UInt16
        let recoveryExecutionLo: UInt64
        let recoveryExecutionHi: UInt64
        let recoveryBindingGeneration: UInt64
        let recoveryGeneration: UInt64

        init(
            paneLo: UInt64,
            paneHi: UInt64,
            rect: CGRect,
            focused: Bool,
            live: Bool,
            zoomed: Bool = false,
            occluded: Bool = false,
            title: String,
            recoveryFlags: UInt16 = 0,
            recoveryExecutionLo: UInt64 = 0,
            recoveryExecutionHi: UInt64 = 0,
            recoveryBindingGeneration: UInt64 = 0,
            recoveryGeneration: UInt64 = 0
        ) {
            self.paneLo = paneLo
            self.paneHi = paneHi
            self.rect = rect
            self.focused = focused
            self.live = live
            self.zoomed = zoomed
            self.occluded = occluded
            self.title = title
            self.recoveryFlags = recoveryFlags
            self.recoveryExecutionLo = recoveryExecutionLo
            self.recoveryExecutionHi = recoveryExecutionHi
            self.recoveryBindingGeneration = recoveryBindingGeneration
            self.recoveryGeneration = recoveryGeneration
        }

        /// Everything but geometry: a change here rebuilds region views.
        var identity: Region {
            Region(
                paneLo: paneLo,
                paneHi: paneHi,
                rect: .zero,
                focused: focused,
                live: live,
                zoomed: zoomed,
                occluded: occluded,
                title: title,
                recoveryFlags: recoveryFlags,
                recoveryExecutionLo: recoveryExecutionLo,
                recoveryExecutionHi: recoveryExecutionHi,
                recoveryBindingGeneration: recoveryBindingGeneration,
                recoveryGeneration: recoveryGeneration
            )
        }

        var recoveryTarget: SeyalAppPaneRecoveryTarget {
            var target = SeyalAppPaneRecoveryTarget()
            target.version = UInt16(SEYAL_APP_ABI_VERSION)
            target.size = recoveryFlags == 0
                ? 0
                : UInt16(MemoryLayout<SeyalAppPaneRecoveryTarget>.size)
            target.flags = recoveryFlags
            target.pane_lo = paneLo
            target.pane_hi = paneHi
            target.execution_lo = recoveryExecutionLo
            target.execution_hi = recoveryExecutionHi
            target.binding_generation = recoveryBindingGeneration
            target.recovery_generation = recoveryGeneration
            return target
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

    /// Container for the focused Pane's chrome (transcript/composer/primary Metal).
    let liveContent = NSView()
    var onFocusPane: ((UInt64, UInt64) -> Void)?
    /// Close a named leaf from non-live region surface chrome (B.1 strip).
    var onClosePane: ((UInt64, UInt64) -> Void)?
    var onRetryPaneRecovery: ((SeyalAppPaneRecoveryTarget) -> Void)?
    /// Rust application root the projection is read from; 0 until wired.
    var appHandle: UInt64 = 0
    private(set) var regions: [Region] = []
    private(set) var dividers: [Divider] = []
    private var regionViews: [PaneRegionView] = []
    private var dividerViews: [PaneDividerView] = []
    private var secondaryLiveHosts: [PaneKey: SecondaryLivePaneHost] = [:]
    private var theme: NativeTheme?
    /// Owns the focused chrome frame. A translates-on autoresizing mask freezes
    /// the last manual width, so the focused Metal surface misses later resizes.
    private var liveOriginX: NSLayoutConstraint!
    private var liveOriginY: NSLayoutConstraint!
    private var liveWidth: NSLayoutConstraint!
    private var liveHeight: NSLayoutConstraint!
    private var applyingLiveFrame = false

    private struct PaneKey: Hashable {
        let lo: UInt64
        let hi: UInt64
    }

    override var isFlipped: Bool { true }

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        // Explicit frame constraints, updated from the focused region. An
        // autoresizing mask turns the manual frame into a required constant
        // width, so a later window resize never reaches the focused surface.
        liveContent.translatesAutoresizingMaskIntoConstraints = false
        // Rust's region is the only size authority: content larger than the
        // region is clipped rather than drawn over a neighbouring Pane.
        liveContent.clipsToBounds = true
        addSubview(liveContent)
        liveOriginX = liveContent.leadingAnchor.constraint(equalTo: leadingAnchor)
        liveOriginY = liveContent.topAnchor.constraint(equalTo: topAnchor)
        liveWidth = liveContent.widthAnchor.constraint(equalToConstant: 1)
        liveHeight = liveContent.heightAnchor.constraint(equalToConstant: 1)
        NSLayoutConstraint.activate([liveOriginX, liveOriginY, liveWidth, liveHeight])
    }

    /// Registry handle → secondary host lookup for shared bridge poll routing.
    func secondaryHost(displayHandle: UInt64) -> SecondaryLivePaneHost? {
        secondaryLiveHosts.values.first { $0.displayHandle == displayHandle }
    }

    /// All secondary live hosts (Metal + subdued per-Pane composer).
    var secondaryHosts: [SecondaryLivePaneHost] {
        Array(secondaryLiveHosts.values)
    }

    func reconcileSecondaryComposers() {
        for host in secondaryLiveHosts.values {
            host.reconcileComposer()
        }
    }

    func applyThemeToSecondaryHosts(_ theme: NativeTheme) {
        for host in secondaryLiveHosts.values {
            host.apply(theme: theme)
        }
    }

    /// Ensure non-focused LIVE leaves with a display handle have Metal hosts.
    func reconcileSecondaryLiveHosts(
        bindings: [(paneLo: UInt64, paneHi: UInt64, displayHandle: UInt64, focused: Bool, live: Bool, recoveryTarget: SeyalAppPaneRecoveryTarget)]
    ) {
        var keep = Set<PaneKey>()
        for binding in bindings {
            let key = PaneKey(lo: binding.paneLo, hi: binding.paneHi)
            guard binding.live else {
                if let host = secondaryLiveHosts.removeValue(forKey: key) {
                    host.removeFromSuperview()
                }
                continue
            }
            guard !binding.focused else {
                if let host = secondaryLiveHosts.removeValue(forKey: key) {
                    host.removeFromSuperview()
                }
                continue
            }
            keep.insert(key)
            let host: SecondaryLivePaneHost
            if let existing = secondaryLiveHosts[key] {
                host = existing
                host.updateDisplayHandle(binding.displayHandle)
                host.updateRecoveryTarget(binding.recoveryTarget)
            } else {
                host = SecondaryLivePaneHost(
                    appHandle: appHandle,
                    paneLo: binding.paneLo,
                    paneHi: binding.paneHi,
                    displayHandle: binding.displayHandle,
                    recoveryTarget: binding.recoveryTarget
                )
                host.onFocus = { [weak self] in
                    self?.onFocusPane?(key.lo, key.hi)
                }
                host.onRetryRecovery = { [weak self] target in
                    self?.onRetryPaneRecovery?(target)
                }
                secondaryLiveHosts[key] = host
                addSubview(host, positioned: .above, relativeTo: liveContent)
                if binding.displayHandle != 0 {
                    host.noteRegistryPolled()
                }
            }
            // Publish the target frame in the same reconciliation turn that
            // installs the host. Waiting for a later parent layout pass leaves
            // the AX button at its initial zero frame and unhittable to XCUI.
            if let region = regions.first(where: { $0.paneLo == key.lo && $0.paneHi == key.hi }) {
                let inset: CGFloat = Self.showsSplitChrome(regions) ? PaneRegionView.borderWidth : 0
                host.frame = scaled(region.rect).insetBy(dx: inset, dy: inset)
            }
        }
        for key in secondaryLiveHosts.keys where !keep.contains(key) {
            secondaryLiveHosts.removeValue(forKey: key)?.removeFromSuperview()
        }
        needsLayout = true
    }

    /// Compatibility projection for component callers that only exercise the
    /// live-host path. Production projections always pass Rust recovery state.
    func reconcileSecondaryLiveHosts(
        bindings: [(paneLo: UInt64, paneHi: UInt64, displayHandle: UInt64, focused: Bool, live: Bool)]
    ) {
        reconcileSecondaryLiveHosts(
            bindings: bindings.map { binding in
                (
                    paneLo: binding.paneLo,
                    paneHi: binding.paneHi,
                    displayHandle: binding.displayHandle,
                    focused: binding.focused,
                    live: binding.live,
                    recoveryTarget: SeyalAppPaneRecoveryTarget()
                )
            }
        )
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
            let recovery = seyal_app_pane_recovery_target_for_pane(
                appHandle,
                raw.pane_lo,
                raw.pane_hi
            )
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
                    title: title,
                    recoveryFlags: recovery.flags,
                    recoveryExecutionLo: recovery.execution_lo,
                    recoveryExecutionHi: recovery.execution_hi,
                    recoveryBindingGeneration: recovery.binding_generation,
                    recoveryGeneration: recovery.recovery_generation
                )
            )
        }
        return regions
    }

    func apply(_ next: [Region], dividers nextDividers: [Divider]) {
        guard next != regions || nextDividers != dividers else { return }
        let rebuildRegions = next.map(\.identity) != regions.map(\.identity)
        let rebuildDividers = nextDividers.map(\.identity) != dividers.map(\.identity)
        regions = next
        dividers = nextDividers
        needsLayout = true
        // Geometry-only change (a ratio drag): keep the views and relayout.
        // Regions and dividers rebuild independently, so a Region change
        // mid-gesture (focus, live binding, title) never detaches the divider
        // AppKit is delivering the drag to.
        if rebuildRegions {
            rebuildRegionViews(next)
        }
        if rebuildDividers {
            rebuildDividerViews(nextDividers)
        }
    }

    /// Borders and the live-surface inset must agree, so both use the visible leaf count.
    private static func showsSplitChrome(_ regions: [Region]) -> Bool {
        regions.filter { !$0.occluded }.count > 1
    }

    private func rebuildRegionViews(_ next: [Region]) {
        regionViews.forEach { $0.removeFromSuperview() }
        let split = Self.showsSplitChrome(next)
        regionViews = next.enumerated().map { index, region in
            let view = PaneRegionView(region: region, index: index, split: split)
            view.isHidden = region.occluded
            view.onFocus = { [weak self] in
                self?.onFocusPane?(region.paneLo, region.paneHi)
            }
            view.onClose = { [weak self] in
                self?.onClosePane?(region.paneLo, region.paneHi)
            }
            if let theme { view.apply(theme: theme) }
            // Regions sit under the live container so its content stays on top.
            addSubview(view, positioned: .below, relativeTo: liveContent)
            return view
        }
        // Focused chrome container stays visible while any leaf is LIVE; it is
        // positioned on the focused LIVE region (or the sole LIVE region).
        liveContent.isHidden = !next.contains(where: \.live)
    }

    private func rebuildDividerViews(_ nextDividers: [Divider]) {
        dividerViews.forEach { $0.removeFromSuperview() }
        dividerViews = nextDividers.enumerated().map { index, divider in
            let view = PaneDividerView(index: index, vertical: divider.vertical)
            view.onDrag = { [weak self] point in
                self?.moveDivider(divider, to: point)
            }
            if let theme { view.apply(theme: theme) }
            // Dividers sit above live hosts so edges stay draggable.
            addSubview(view, positioned: .above, relativeTo: liveContent)
            return view
        }
        for host in secondaryLiveHosts.values {
            addSubview(host, positioned: .above, relativeTo: liveContent)
        }
    }

    func apply(theme: NativeTheme) {
        self.theme = theme
        regionViews.forEach { $0.apply(theme: theme) }
        dividerViews.forEach { $0.apply(theme: theme) }
    }

    /// Refresh non-live in-Tab surface strips (title / Close Pane visibility).
    /// Live leaves keep chrome inside `liveContent` / `SecondaryLivePaneHost`.
    func updatePlaceholderChrome(allowsClose: Bool, theme: NativeTheme?) {
        for view in regionViews {
            view.updatePlaceholderChrome(allowsClose: allowsClose, theme: theme)
        }
    }

    override func layout() {
        let liveFrame = focusedLiveFrame()
        if !applyingLiveFrame,
           liveFrame.width > 1,
           liveFrame.height > 1,
           (
               abs(liveOriginX.constant - liveFrame.minX) > 0.5
                   || abs(liveOriginY.constant - liveFrame.minY) > 0.5
                   || abs(liveWidth.constant - liveFrame.width) > 0.5
                   || abs(liveHeight.constant - liveFrame.height) > 0.5
           )
        {
            liveOriginX.constant = liveFrame.minX
            liveOriginY.constant = liveFrame.minY
            liveWidth.constant = liveFrame.width
            liveHeight.constant = liveFrame.height
            applyingLiveFrame = true
            layoutSubtreeIfNeeded()
            applyingLiveFrame = false
        }
        super.layout()
        let inset: CGFloat = Self.showsSplitChrome(regions) ? PaneRegionView.borderWidth : 0
        for (view, region) in zip(regionViews, regions) {
            view.frame = scaled(region.rect)
        }
        for region in regions where region.live && !region.focused && !region.occluded {
            let key = PaneKey(lo: region.paneLo, hi: region.paneHi)
            secondaryLiveHosts[key]?.frame = scaled(region.rect).insetBy(dx: inset, dy: inset)
        }
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

    /// Focused chrome (composer/transcript/primary Metal) follows the focused
    /// LIVE region; fall back to the first LIVE region, then the full Tab.
    private func focusedLiveFrame() -> CGRect {
        let inset: CGFloat = Self.showsSplitChrome(regions) ? PaneRegionView.borderWidth : 0
        let focusedLive = regions.first(where: { $0.live && $0.focused })?.rect
            ?? regions.first(where: \.live)?.rect
            ?? CGRect(x: 0, y: 0, width: 1, height: 1)
        return scaled(focusedLive).insetBy(dx: inset, dy: inset)
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
/// Region frame for one Tab leaf. LIVE leaves present Metal/composer via
/// `liveContent` / `SecondaryLivePaneHost`. Non-live split leaves still expose
/// B.1 pane surface chrome (title + Close) so Split is visually addressable
/// even before Runtime bind admits a secondary live host.
@MainActor
private final class PaneRegionView: NSView {
    static let borderWidth: CGFloat = 1

    var onFocus: (() -> Void)?
    var onClose: (() -> Void)?
    private let region: PaneLayoutView.Region
    private let split: Bool
    private let chromeBar = PaneChromeBarView()

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
        chromeBar.translatesAutoresizingMaskIntoConstraints = false
        // Live hosts own the strip; placeholder leaves keep B.1 chrome here.
        chromeBar.isHidden = !split || region.live
        chromeBar.onClose = { [weak self] in
            self?.onClose?()
        }
        addSubview(chromeBar)
        let trailing = chromeBar.trailingAnchor.constraint(equalTo: trailingAnchor)
        trailing.priority = .defaultHigh
        NSLayoutConstraint.activate([
            chromeBar.leadingAnchor.constraint(equalTo: leadingAnchor),
            trailing,
            chromeBar.topAnchor.constraint(equalTo: topAnchor),
        ])
        chromeBar.apply(title: region.title, theme: nil, focused: region.focused)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("PaneRegionView is programmatic")
    }

    func apply(theme: NativeTheme) {
        layer?.borderWidth = split ? Self.borderWidth : 0
        layer?.borderColor = (region.focused ? theme.accent : theme.seam).cgColor
        guard split, !region.live else {
            chromeBar.isHidden = true
            return
        }
        chromeBar.isHidden = false
        chromeBar.apply(title: region.title, theme: theme, focused: region.focused)
    }

    func updatePlaceholderChrome(allowsClose: Bool, theme: NativeTheme?) {
        guard split, !region.live else {
            chromeBar.isHidden = true
            return
        }
        chromeBar.isHidden = false
        chromeBar.apply(title: region.title, theme: theme, focused: region.focused)
        chromeBar.setCloseVisible(allowsClose)
    }

    /// Pane regions route clicks to Rust focus. Keep the custom accessibility
    /// group enabled so assistive technology and XCUI can invoke that action.
    override func isAccessibilityEnabled() -> Bool {
        true
    }

    override func hitTest(_ point: NSPoint) -> NSView? {
        let local = convert(point, from: superview)
        guard bounds.contains(local) else { return nil }
        let hit = super.hitTest(point)
        // Close Pane on the placeholder strip must receive the click.
        if let hit, hit.isDescendant(of: chromeBar) || hit === chromeBar {
            return hit
        }
        return self
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
