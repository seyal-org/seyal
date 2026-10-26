import AppKit
import XCTest
@testable import Seyal

/// #928: on the production composition (C3 pane splitting), dragging a Split
/// divider goes through the real host path (`mouseDragged` → `onDrag` →
/// `MOVE_SPLIT_DIVIDER`). Rust derives and clamps the ratio; the host only
/// re-projects regions and dividers from Rust.
@MainActor
final class DividerDragProjectionTests: XCTestCase {
    /// The offscreen host window, never ordered front. XCTest keeps every
    /// test-case instance alive until the run ends, so each test detaches it
    /// with `closeHost()`; a leaked window would hold a `ProductChromeHostView`
    /// that later live-app tests could find in `NSApp.windows`.
    private var window: NSWindow?
    private var hostWindowsBefore = 0

    /// A windowed host so divider drags convert real window coordinates.
    private func makeHost() -> ProductChromeHostView {
        hostWindowsBefore = productHostWindowCount()
        let view = ProductChromeHostView(frame: NSRect(x: 0, y: 0, width: 1200, height: 760))
        let window = NSWindow(
            contentRect: view.frame,
            styleMask: [.titled, .resizable],
            backing: .buffered,
            defer: false
        )
        window.isReleasedWhenClosed = false
        window.contentView = view
        self.window = window
        reconcile(view)
        return view
    }

    /// Detaches and closes the host window. A closed window can stay in
    /// `NSApp.windows`, so the host view is removed from it explicitly.
    private func closeHost() {
        guard let window else { return }
        window.contentView = nil
        window.close()
        self.window = nil
        XCTAssertEqual(
            productHostWindowCount(), hostWindowsBefore,
            "no test host window stays discoverable by live-app tests"
        )
    }

    private func productHostWindowCount() -> Int {
        NSApp.windows.filter { $0.contentView is ProductChromeHostView }.count
    }

    private func reconcile(_ view: ProductChromeHostView) {
        view.reconcileChrome()
        view.layoutSubtreeIfNeeded()
    }

    func testDraggingDividerUpdatesRustRatioAndRegionFrames() throws {
        let view = makeHost()
        defer { closeHost() }
        let handle = view.pane.appHandle
        try click(view, "seyal-split-right")
        reconcile(view)
        let divider = try XCTUnwrap(element(view, "seyal-pane-divider-0"))
        XCTAssertEqual(divider.accessibilityValue() as? String, "0.50")
        XCTAssertEqual(seyal_app_pane_divider(handle, 0).ratio, 0.5, accuracy: 0.0001)
        let before = try regionFrames(view, count: 2)

        try drag(divider, toFraction: 0.3)
        reconcile(view)

        let rust = seyal_app_pane_divider(handle, 0)
        XCTAssertEqual(rust.ratio, 0.3, accuracy: 0.01, "Rust derives the ratio from the pointer")
        XCTAssertEqual(rust.line_x, rust.x + rust.width * rust.ratio, accuracy: 0.0001)
        let after = try regionFrames(view, count: 2)
        XCTAssertEqual(
            try XCTUnwrap(element(view, "seyal-pane-divider-0")).accessibilityValue() as? String,
            "0.30",
            "the divider re-projects Rust's ratio"
        )
        let leading = after.min { $0.minX < $1.minX }!
        let trailing = after.max { $0.minX < $1.minX }!
        XCTAssertLessThan(leading.width, before.map(\.width).min()!, "leading region shrinks")
        XCTAssertGreaterThan(trailing.width, before.map(\.width).max()!, "trailing region grows")
        XCTAssertEqual(seyal_app_shell(handle).pane_count, 2, "a drag never changes the tree")
        try assertLiveSurfaceInFocusedRegion(view, count: 2)
    }

    func testDragClampsToRustRatioBounds() throws {
        let view = makeHost()
        defer { closeHost() }
        let handle = view.pane.appHandle
        try click(view, "seyal-split-right")
        reconcile(view)
        let divider = try XCTUnwrap(element(view, "seyal-pane-divider-0"))

        try drag(divider, toFraction: 0.01)
        reconcile(view)
        XCTAssertEqual(seyal_app_pane_divider(handle, 0).ratio, 0.1, accuracy: 0.0001, "clamped to MIN")
        XCTAssertEqual(
            try XCTUnwrap(element(view, "seyal-pane-divider-0")).accessibilityValue() as? String,
            "0.10"
        )

        try drag(try XCTUnwrap(element(view, "seyal-pane-divider-0")), toFraction: 0.99)
        reconcile(view)
        XCTAssertEqual(seyal_app_pane_divider(handle, 0).ratio, 0.9, accuracy: 0.0001, "clamped to MAX")
        let frames = try regionFrames(view, count: 2)
        XCTAssertTrue(frames.allSatisfy { $0.width > 0 }, "a clamped Split keeps both regions visible")
    }

    func testNestedDividerDragResizesOnlyItsOwnSplit() throws {
        let view = makeHost()
        defer { closeHost() }
        let handle = view.pane.appHandle
        try click(view, "seyal-split-right")
        reconcile(view)
        try click(view, "seyal-split-down")
        reconcile(view)
        XCTAssertEqual(seyal_app_shell(handle).pane_count, 3)

        let (outer, inner) = try dividerIndices(handle)
        let outerBefore = seyal_app_pane_divider(handle, UInt32(outer)).ratio
        let innerView = try XCTUnwrap(element(view, "seyal-pane-divider-\(inner)"))
        try drag(innerView, toFraction: 0.7)
        reconcile(view)

        XCTAssertEqual(seyal_app_pane_divider(handle, UInt32(inner)).ratio, 0.7, accuracy: 0.01)
        XCTAssertEqual(
            seyal_app_pane_divider(handle, UInt32(outer)).ratio,
            outerBefore,
            accuracy: 0.0001,
            "dragging the nested divider leaves the outer Split's ratio alone"
        )
        try assertLiveSurfaceInFocusedRegion(view, count: 3)
    }

    /// A Region change mid-gesture (focus, live binding or title) must not
    /// detach the divider AppKit is delivering the drag to; the rest of the
    /// drag keeps reaching Rust.
    func testDragContinuesWhenRegionsChangeMidGesture() throws {
        let view = makeHost()
        defer { closeHost() }
        let handle = view.pane.appHandle
        try click(view, "seyal-split-right")
        reconcile(view)
        let divider = try XCTUnwrap(element(view, "seyal-pane-divider-0"))
        try drag(divider, toFraction: 0.4)
        reconcile(view)
        XCTAssertEqual(seyal_app_pane_divider(handle, 0).ratio, 0.4, accuracy: 0.01)

        let unfocused = try XCTUnwrap(
            (0..<2).first { element(view, "seyal-pane-region-\($0)")?.accessibilityValue() as? String != "focused" }
        )
        XCTAssertTrue(try XCTUnwrap(element(view, "seyal-pane-region-\(unfocused)")).accessibilityPerformPress())
        reconcile(view)

        XCTAssertNotNil(divider.superview, "the divider receiving the drag stays in the layout")
        XCTAssertTrue(element(view, "seyal-pane-divider-0") === divider, "an unchanged divider is not rebuilt")
        try drag(divider, toFraction: 0.7)
        reconcile(view)
        XCTAssertEqual(seyal_app_pane_divider(handle, 0).ratio, 0.7, accuracy: 0.01, "the rest of the drag reaches Rust")
        XCTAssertEqual(divider.accessibilityValue() as? String, "0.70")
    }

    /// Rust's region is the only layout authority: shrinking the live
    /// (focused) region below its content's natural size clips that content
    /// instead of growing the window or the Pane layout (ADR-015).
    func testNarrowLiveRegionNeverResizesTheWindow() throws {
        let view = makeHost()
        defer { closeHost() }
        let window = try XCTUnwrap(view.window)
        try click(view, "seyal-split-right")
        reconcile(view)
        window.layoutIfNeeded()
        let windowBefore = window.frame
        let divider = try XCTUnwrap(element(view, "seyal-pane-divider-0"))
        let layout = try XCTUnwrap(divider.superview)
        let layoutBefore = layout.bounds

        // Split Right focuses the new trailing leaf, which hosts the live surface.
        try drag(divider, toFraction: 0.99)
        reconcile(view)
        window.layoutIfNeeded()

        XCTAssertEqual(seyal_app_pane_divider(view.pane.appHandle, 0).ratio, 0.9, accuracy: 0.0001)
        XCTAssertEqual(window.frame, windowBefore, "live content must not grow the window")
        XCTAssertEqual(layout.bounds, layoutBefore, "live content must not grow the Pane layout")
        try assertLiveSurfaceInFocusedRegion(view, count: 2)
        let live = try XCTUnwrap(view.pane.superview)
        XCTAssertEqual(live.frame.width, (layout.bounds.width * 0.1).rounded() - 2, accuracy: 1, "live width follows Rust")
        XCTAssertTrue(live.clipsToBounds, "oversized live content is clipped to its region")
    }

    func testStaleDividerTargetFailsClosedAndLastPaneRulesHold() throws {
        let view = makeHost()
        defer { closeHost() }
        let handle = view.pane.appHandle
        try click(view, "seyal-split-right")
        reconcile(view)
        let divider = seyal_app_pane_divider(handle, 0)
        try drag(try XCTUnwrap(element(view, "seyal-pane-divider-0")), toFraction: 0.4)
        reconcile(view)

        try click(view, "seyal-close-pane")
        reconcile(view)
        XCTAssertEqual(seyal_app_shell(handle).pane_count, 1, "Close Pane still collapses a resized Split")
        XCTAssertNil(element(view, "seyal-pane-divider-0"))
        XCTAssertEqual(seyal_app_pane_region(handle, 0).width, 1.0, "the survivor fills the Tab")

        var stale = SeyalAppAction()
        stale.version = UInt16(SEYAL_APP_ABI_VERSION)
        stale.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        stale.kind = UInt16(SEYAL_APP_ACTION_MOVE_SPLIT_DIVIDER.rawValue)
        stale.target_execution_lo = divider.leading_pane_lo
        stale.target_execution_hi = divider.leading_pane_hi
        stale.reserved = Float(0.3).bitPattern
        XCTAssertNotEqual(seyal_app_apply(handle, &stale), 0, "a removed Split's divider fails closed")
        XCTAssertEqual(seyal_app_pane_region(handle, 0).width, 1.0, "rejected drag changes nothing")
        XCTAssertEqual(seyal_app_shell(handle).pane_count, 1)
        let closePane = try XCTUnwrap(button(view, "seyal-close-pane"))
        XCTAssertFalse(closePane.isHidden, "the surviving Pane still offers hierarchical Close Pane")
    }

    // MARK: - Helpers

    /// Drive the production divider drag: a real `leftMouseDragged` event at
    /// `fraction` of the parent layout along the divider's drag axis.
    private func drag(_ divider: NSView, toFraction fraction: CGFloat) throws {
        let layout = try XCTUnwrap(divider.superview, "divider sits in the Pane layout")
        let window = try XCTUnwrap(divider.window, "host is windowed")
        let vertical = divider.accessibilityOrientation() == .vertical
        let point = vertical
            ? NSPoint(x: layout.bounds.width * fraction, y: divider.frame.midY)
            : NSPoint(x: divider.frame.midX, y: layout.bounds.height * fraction)
        let event = try XCTUnwrap(
            NSEvent.mouseEvent(
                with: .leftMouseDragged,
                location: layout.convert(point, to: nil),
                modifierFlags: [],
                timestamp: ProcessInfo.processInfo.systemUptime,
                windowNumber: window.windowNumber,
                context: nil,
                eventNumber: 0,
                clickCount: 1,
                pressure: 1
            )
        )
        divider.mouseDragged(with: event)
    }

    /// Outer (side-by-side) and inner (stacked) divider indices after
    /// Split Right then Split Down.
    private func dividerIndices(_ handle: UInt64) throws -> (outer: Int, inner: Int) {
        let rows = (0..<2).map { seyal_app_pane_divider(handle, UInt32($0)) }
        XCTAssertTrue(rows.allSatisfy { $0.size != 0 }, "two Splits project two dividers")
        let outer = try XCTUnwrap(rows.firstIndex { $0.axis == 0 }, "a side-by-side divider")
        let inner = try XCTUnwrap(rows.firstIndex { $0.axis != 0 }, "a stacked divider")
        return (outer, inner)
    }

    private func regionFrames(_ view: NSView, count: Int) throws -> [NSRect] {
        try (0..<count).map { try XCTUnwrap(element(view, "seyal-pane-region-\($0)")).frame }
    }

    /// The single live surface stays inside the focused region (inset by the
    /// 1pt split border) after any resize.
    private func assertLiveSurfaceInFocusedRegion(_ view: ProductChromeHostView, count: Int) throws {
        let focused = try XCTUnwrap(
            (0..<count).first { element(view, "seyal-pane-region-\($0)")?.accessibilityValue() as? String == "focused" }
        )
        let region = try XCTUnwrap(element(view, "seyal-pane-region-\(focused)")).frame
        let live = try XCTUnwrap(view.pane.superview)
        XCTAssertEqual(live.frame, region.insetBy(dx: 1, dy: 1))
    }

    private func click(_ view: NSView, _ identifier: String) throws {
        let control = try XCTUnwrap(button(view, identifier), identifier)
        XCTAssertFalse(control.isHidden, "\(identifier) is shown")
        control.performClick(nil)
    }
}

@MainActor
private func element(_ root: NSView, _ identifier: String) -> NSView? {
    if root.accessibilityIdentifier() == identifier {
        return root
    }
    for child in root.subviews {
        if let found = element(child, identifier) {
            return found
        }
    }
    return nil
}

@MainActor
private func button(_ root: NSView, _ identifier: String) -> NSButton? {
    if let control = root as? NSButton, control.accessibilityIdentifier() == identifier {
        return control
    }
    for child in root.subviews {
        if let found = button(child, identifier) {
            return found
        }
    }
    return nil
}
