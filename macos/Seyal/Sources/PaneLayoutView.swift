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
    struct TerminalTab: Equatable {
        let lo: UInt64
        let hi: UInt64
        let selected: Bool
    }

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
        let terminalTabs: [TerminalTab]

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
            recoveryGeneration: UInt64 = 0,
            terminalTabs: [TerminalTab] = []
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
            self.terminalTabs = terminalTabs
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
                recoveryGeneration: recoveryGeneration,
                terminalTabs: terminalTabs
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
    var onSelectTerminalTab: ((UInt64, UInt64, UInt64, UInt64) -> Void)?
    var onCloseTerminalTab: ((UInt64, UInt64, UInt64, UInt64) -> Void)?
    var onNewTerminal: ((UInt64, UInt64) -> Void)?
    var onRetryPaneRecovery: ((SeyalAppPaneRecoveryTarget) -> Void)?
    /// Rust application root the projection is read from; 0 until wired.
    var appHandle: UInt64 = 0
    private(set) var regions: [Region] = []
    private(set) var dividers: [Divider] = []
    private var regionViews: [PaneRegionView] = []
    private var dividerViews: [PaneDividerView] = []
    private var terminalStrips: [PaneKey: PaneTerminalTabStrip] = [:]
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
                    recoveryGeneration: recovery.recovery_generation,
                    terminalTabs: readTerminalTabs(appHandle: appHandle, paneLo: raw.pane_lo, paneHi: raw.pane_hi)
                )
            )
        }
        return regions
    }

    private static func readTerminalTabs(appHandle: UInt64, paneLo: UInt64, paneHi: UInt64) -> [TerminalTab] {
        let count = Int(seyal_app_pane_terminal_tab_count(appHandle, paneLo, paneHi))
        return (0..<count).compactMap { index in
            let row = seyal_app_pane_terminal_tab(appHandle, paneLo, paneHi, UInt32(index))
            guard row.size != 0 else { return nil }
            return TerminalTab(lo: row.terminal_lo, hi: row.terminal_hi, selected: row.flags & 1 != 0)
        }
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
            rebuildTerminalStrips(next)
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
            secondaryLiveHosts[key]?.frame = contentFrame(region.rect, inset: inset)
        }
        for region in regions where !region.occluded {
            let key = PaneKey(lo: region.paneLo, hi: region.paneHi)
            var stripFrame = scaled(region.rect).insetBy(dx: inset, dy: inset)
            stripFrame.size.height = min(PaneTerminalTabStrip.height, stripFrame.height)
            terminalStrips[key]?.frame = stripFrame
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
        return contentFrame(focusedLive, inset: inset)
    }

    private func contentFrame(_ unit: CGRect, inset: CGFloat) -> CGRect {
        var frame = scaled(unit).insetBy(dx: inset, dy: inset)
        frame.origin.y += PaneTerminalTabStrip.height
        frame.size.height = max(0, frame.height - PaneTerminalTabStrip.height)
        return frame
    }

    private func rebuildTerminalStrips(_ next: [Region]) {
        terminalStrips.values.forEach { $0.removeFromSuperview() }
        terminalStrips.removeAll()
        for region in next {
            let key = PaneKey(lo: region.paneLo, hi: region.paneHi)
            let strip = PaneTerminalTabStrip(paneLo: region.paneLo, paneHi: region.paneHi, tabs: region.terminalTabs)
            strip.onSelect = { [weak self] tab in self?.onSelectTerminalTab?(key.lo, key.hi, tab.lo, tab.hi) }
            strip.onClose = { [weak self] tab in self?.onCloseTerminalTab?(key.lo, key.hi, tab.lo, tab.hi) }
            strip.onNew = { [weak self] in
                self?.onFocusPane?(key.lo, key.hi)
                self?.onNewTerminal?(key.lo, key.hi)
            }
            terminalStrips[key] = strip
            addSubview(strip, positioned: .above, relativeTo: liveContent)
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

@MainActor
private final class PaneTerminalTabStrip: NSView {
    static let height: CGFloat = 30
    var onSelect: ((PaneLayoutView.TerminalTab) -> Void)?
    var onClose: ((PaneLayoutView.TerminalTab) -> Void)?
    var onNew: (() -> Void)?
    private let tabs: [PaneLayoutView.TerminalTab]

    init(paneLo: UInt64, paneHi: UInt64, tabs: [PaneLayoutView.TerminalTab]) {
        self.tabs = tabs
        super.init(frame: .zero)
        translatesAutoresizingMaskIntoConstraints = false
        setAccessibilityElement(true)
        setAccessibilityRole(.group)
        setAccessibilityIdentifier("seyal-pane-\(paneHi)-\(paneLo)-terminal-tabs")
        let stack = NSStackView()
        stack.orientation = .horizontal
        stack.alignment = .centerY
        stack.spacing = 4
        stack.translatesAutoresizingMaskIntoConstraints = false
        addSubview(stack)
        setAccessibilityChildren([stack])
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 6),
            stack.trailingAnchor.constraint(lessThanOrEqualTo: trailingAnchor, constant: -6),
            stack.topAnchor.constraint(equalTo: topAnchor, constant: 2),
            stack.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -2),
        ])
        for (index, tab) in tabs.enumerated() {
            let group = NSStackView()
            group.orientation = .horizontal
            group.spacing = 0
            let button = NSButton(title: "Terminal \(index + 1)", target: self, action: #selector(selectTab(_:)))
            button.bezelStyle = .recessed
            button.identifier = NSUserInterfaceItemIdentifier("seyal-terminal-tab-\(tab.hi)-\(tab.lo)")
            button.setAccessibilityIdentifier(button.identifier!.rawValue)
            button.setAccessibilityValue(tab.selected ? "selected" : "")
            button.state = tab.selected ? .on : .off
            button.tag = index
            group.addArrangedSubview(button)
            let close = NSButton(title: "×", target: self, action: #selector(closeTab(_:)))
            close.bezelStyle = .texturedRounded
            close.setAccessibilityIdentifier("seyal-terminal-tab-\(tab.hi)-\(tab.lo)-close")
            close.setAccessibilityLabel("Close Terminal \(index + 1)")
            close.tag = index
            group.addArrangedSubview(close)
            stack.addArrangedSubview(group)
        }
        let add = NSButton(title: "+", target: self, action: #selector(newTerminal))
        add.bezelStyle = .texturedRounded
        add.setAccessibilityIdentifier("seyal-pane-\(paneHi)-\(paneLo)-new-terminal")
        add.setAccessibilityLabel("New Terminal")
        stack.addArrangedSubview(add)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("PaneTerminalTabStrip is programmatic") }

    @objc private func selectTab(_ sender: NSButton) {
        guard tabs.indices.contains(sender.tag) else { return }
        onSelect?(tabs[sender.tag])
    }

    @objc private func closeTab(_ sender: NSButton) {
        guard tabs.indices.contains(sender.tag) else { return }
        onClose?(tabs[sender.tag])
    }

    @objc private func newTerminal() { onNew?() }
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
        // Title placeholder only when the leaf has no live Metal host yet.
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

    /// Pane regions route clicks to Rust focus. Keep the custom accessibility
    /// group enabled so assistive technology and XCUI can invoke that action.
    override func isAccessibilityEnabled() -> Bool {
        true
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
