import AppKit
import XCTest
@testable import Seyal

/// #923: the production composition projects the active Tab's Rust PaneTree
/// into one host region per leaf. Geometry, focus and the single live surface
/// all come from Rust; the host only places frames and forwards focus/close.
@MainActor
final class PaneRegionProjectionTests: XCTestCase {
    private func makeHost() -> ProductChromeHostView {
        let view = ProductChromeHostView(frame: NSRect(x: 0, y: 0, width: 1200, height: 760))
        reconcile(view)
        return view
    }

    private func reconcile(_ view: ProductChromeHostView) {
        view.reconcileChrome()
        view.layoutSubtreeIfNeeded()
    }

    func testSplitRightAndDownProjectOneDistinctRegionPerLeaf() throws {
        let view = makeHost()
        let handle = view.pane.appHandle

        try clickButton(view, "seyal-split-right")
        reconcile(view)
        XCTAssertEqual(seyal_app_shell(handle).pane_count, 2)
        try assertRegions(view, count: 2)
        XCTAssertNotNil(paneChild(view, "seyal-pane-divider-0"), "a Split projects its divider")

        try clickButton(view, "seyal-split-down")
        reconcile(view)
        XCTAssertEqual(seyal_app_shell(handle).pane_count, 3)
        try assertRegions(view, count: 3)
    }

    func testClickingUnfocusedRegionFocusesThatPaneInRustAndChrome() throws {
        let view = makeHost()
        let handle = view.pane.appHandle
        try clickButton(view, "seyal-split-right")
        reconcile(view)

        let unfocused = try XCTUnwrap(
            (0..<2).first { seyal_app_pane_region(handle, UInt32($0)).flags
                & UInt16(SEYAL_APP_PANE_REGION_FOCUSED) == 0 },
            "a two-leaf split has one unfocused region"
        )
        let target = seyal_app_pane_region(handle, UInt32(unfocused))
        let regionView = try XCTUnwrap(paneChild(view, "seyal-pane-region-\(unfocused)"))
        XCTAssertTrue(regionView.accessibilityPerformPress(), "an unfocused region accepts press")
        reconcile(view)

        let shell = seyal_app_shell(handle)
        XCTAssertEqual(shell.focused_pane_lo, target.pane_lo)
        XCTAssertEqual(shell.focused_pane_hi, target.pane_hi)
        try assertRegions(view, count: 2)
        let focusedView = try XCTUnwrap(paneChild(view, "seyal-pane-region-\(unfocused)"))
        XCTAssertEqual(focusedView.accessibilityValue() as? String, "focused")
        assertChromeSelectsFocusedPane(view)

        // Pressing the region that is already focused is not a focus request.
        XCTAssertFalse(focusedView.accessibilityPerformPress())
        XCTAssertEqual(seyal_app_shell(handle).focused_pane_lo, target.pane_lo)
    }

    func testClosingNonLastPaneCollapsesTreeAndRemovesRegion() throws {
        let view = makeHost()
        let handle = view.pane.appHandle
        try clickButton(view, "seyal-split-right")
        reconcile(view)
        XCTAssertEqual(seyal_app_shell(handle).pane_count, 2)

        try clickButton(view, "seyal-close-pane")
        reconcile(view)

        XCTAssertEqual(seyal_app_shell(handle).pane_count, 1)
        try assertRegions(view, count: 1)
        XCTAssertNil(paneChild(view, "seyal-pane-divider-0"), "no divider without a Split")
        let closePane = try XCTUnwrap(button(view, "seyal-close-pane"))
        XCTAssertTrue(closePane.isHidden, "the last Pane offers no Close Pane control")
    }

    func testLastPaneAndStalePaneIdsFailClosed() throws {
        let view = makeHost()
        let handle = view.pane.appHandle
        let sole = seyal_app_shell(handle)
        XCTAssertEqual(sole.pane_count, 1)
        XCTAssertEqual(sole.flags & UInt16(SEYAL_APP_SHELL_ALLOWS_PANE_CLOSE), 0)
        XCTAssertNotEqual(
            applyPane(handle, kind: SEYAL_APP_ACTION_CLOSE_PANE, lo: sole.focused_pane_lo, hi: sole.focused_pane_hi),
            0,
            "the last Pane cannot close"
        )
        XCTAssertEqual(seyal_app_shell(handle).pane_count, 1)

        try clickButton(view, "seyal-split-right")
        reconcile(view)
        let split = seyal_app_shell(handle)
        let removed = (lo: split.focused_pane_lo, hi: split.focused_pane_hi)
        try clickButton(view, "seyal-close-pane")
        reconcile(view)
        let survivor = seyal_app_shell(handle)
        XCTAssertEqual(survivor.pane_count, 1)
        XCTAssertFalse(survivor.focused_pane_lo == removed.lo && survivor.focused_pane_hi == removed.hi)

        XCTAssertNotEqual(
            applyPane(handle, kind: SEYAL_APP_ACTION_FOCUS_PANE, lo: removed.lo, hi: removed.hi),
            0,
            "focusing a closed PaneId fails closed"
        )
        XCTAssertNotEqual(
            applyPane(handle, kind: SEYAL_APP_ACTION_CLOSE_PANE, lo: removed.lo, hi: removed.hi),
            0,
            "closing a closed PaneId fails closed"
        )
        let after = seyal_app_shell(handle)
        XCTAssertEqual(after.pane_count, 1)
        XCTAssertEqual(after.focused_pane_lo, survivor.focused_pane_lo)
        XCTAssertEqual(after.focused_pane_hi, survivor.focused_pane_hi)
        try assertRegions(view, count: 1)
    }

    // MARK: - Assertions

    /// Rust projects exactly `count` regions; the host shows one view per
    /// region, frames do not overlap, exactly one is focused, and the single
    /// live surface sits in the focused region.
    private func assertRegions(
        _ view: ProductChromeHostView,
        count: Int,
        file: StaticString = #filePath,
        line: UInt = #line
    ) throws {
        let handle = view.pane.appHandle
        XCTAssertEqual(seyal_app_pane_region(handle, UInt32(count)).size, 0, "no region past the leaf count", file: file, line: line)
        XCTAssertNil(paneChild(view, "seyal-pane-region-\(count)"), file: file, line: line)

        var frames: [NSRect] = []
        var focusedIndex: Int?
        var panes = Set<String>()
        for index in 0..<count {
            let region = seyal_app_pane_region(handle, UInt32(index))
            XCTAssertNotEqual(region.size, 0, "region \(index) projects", file: file, line: line)
            panes.insert("\(region.pane_lo):\(region.pane_hi)")
            let focused = region.flags & UInt16(SEYAL_APP_PANE_REGION_FOCUSED) != 0
            let live = region.flags & UInt16(SEYAL_APP_PANE_REGION_LIVE) != 0
            XCTAssertFalse(live && !focused, "only the focused region may be live", file: file, line: line)
            let regionView = try XCTUnwrap(paneChild(view, "seyal-pane-region-\(index)"), file: file, line: line)
            XCTAssertEqual(
                regionView.accessibilityValue() as? String,
                focused ? "focused" : "",
                "region \(index) mirrors Rust focus",
                file: file,
                line: line
            )
            XCTAssertGreaterThan(regionView.frame.width, 0, file: file, line: line)
            XCTAssertGreaterThan(regionView.frame.height, 0, file: file, line: line)
            frames.append(regionView.frame)
            if focused {
                XCTAssertNil(focusedIndex, "exactly one focused region", file: file, line: line)
                focusedIndex = index
            }
        }
        XCTAssertEqual(panes.count, count, "each region names a distinct PaneId", file: file, line: line)
        for first in 0..<frames.count {
            for second in (first + 1)..<frames.count {
                let overlap = frames[first].intersection(frames[second])
                XCTAssertTrue(
                    overlap.isNull || overlap.width * overlap.height == 0,
                    "regions \(first) and \(second) overlap",
                    file: file,
                    line: line
                )
            }
        }
        let focused = try XCTUnwrap(focusedIndex, "one region is focused", file: file, line: line)
        let shell = seyal_app_shell(handle)
        let focusedRegion = seyal_app_pane_region(handle, UInt32(focused))
        XCTAssertEqual(focusedRegion.pane_lo, shell.focused_pane_lo, file: file, line: line)
        XCTAssertEqual(focusedRegion.pane_hi, shell.focused_pane_hi, file: file, line: line)
        // No Runtime binding in a component host, so Rust keeps the one live
        // surface on the focused leaf; the host places it in that region.
        XCTAssertNotEqual(
            focusedRegion.flags & UInt16(SEYAL_APP_PANE_REGION_LIVE),
            0,
            "the focused leaf hosts the live surface",
            file: file,
            line: line
        )
        let live = try XCTUnwrap(view.pane.superview, file: file, line: line)
        XCTAssertFalse(live.isHidden, file: file, line: line)
        // In a split the live surface sits inside the region's 1pt border.
        let inset: CGFloat = count > 1 ? 1 : 0
        XCTAssertEqual(
            live.frame,
            frames[focused].insetBy(dx: inset, dy: inset),
            "one live surface, in the focused region",
            file: file,
            line: line
        )
    }

    /// The left panel's Pane rows follow Rust selection: exactly the focused
    /// PaneId's row is selected.
    private func assertChromeSelectsFocusedPane(
        _ view: ProductChromeHostView,
        file: StaticString = #filePath,
        line: UInt = #line
    ) {
        let handle = view.pane.appHandle
        let shell = seyal_app_shell(handle)
        for index in 0..<Int(shell.pane_count) {
            let row = seyal_app_shell_row(handle, UInt16(SEYAL_APP_ROW_PANE), UInt32(index))
            let isFocused = row.id_lo == shell.focused_pane_lo && row.id_hi == shell.focused_pane_hi
            XCTAssertEqual(
                row.flags & UInt16(SEYAL_APP_ROW_SELECTED) != 0,
                isFocused,
                "Rust selects only the focused Pane row",
                file: file,
                line: line
            )
            let rowButton = button(view, "seyal-pane-\(index)")
            XCTAssertNotNil(rowButton, "Pane row \(index) is shown", file: file, line: line)
            XCTAssertEqual(rowButton?.state, isFocused ? .on : .off, "row \(index) selection", file: file, line: line)
        }
    }

    // MARK: - Helpers

    private func clickButton(_ view: NSView, _ identifier: String) throws {
        let control = try XCTUnwrap(button(view, identifier), identifier)
        XCTAssertFalse(control.isHidden, "\(identifier) is shown")
        control.performClick(nil)
    }

    private func applyPane(_ handle: UInt64, kind: SeyalAppActionKind, lo: UInt64, hi: UInt64) -> Int32 {
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = UInt16(kind.rawValue)
        action.target_execution_lo = lo
        action.target_execution_hi = hi
        return seyal_app_apply(handle, &action)
    }
}

@MainActor
private func paneChild(_ root: NSView, _ identifier: String) -> NSView? {
    if root.accessibilityIdentifier() == identifier {
        return root
    }
    for child in root.subviews {
        if let found = paneChild(child, identifier) {
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
