import AppKit
import XCTest

@testable import Seyal

/// Host component coverage for the Seyal Block Component chrome (#1010,
/// M003-BLOCK-COMPONENT-DESIGN §6/§8). Rust owns every input; these tests
/// prove the view renders exactly what the row says and routes actions.
@MainActor
final class SeyalBlockComponentTests: XCTestCase {
    private let dark = NativeThemeRealization.theme(for: NSAppearance(named: .darkAqua)!)
    /// Superviews are weak; keep each test container alive for the test.
    private var hosts: [NSView] = []

    private func block(
        state: UInt32 = SEYAL_APP_BLOCK_STATE_COMPLETED,
        selected: Bool = false,
        canRerun: Bool = true
    ) -> CommandBlockView {
        let view = CommandBlockView(
            row: CommandBlockRow(
                command: "ls /",
                state: UInt16(state),
                isSelected: selected,
                canRerun: canRerun
            ),
            cellHeight: 16,
            lines: 3
        )
        let container = NSView(frame: NSRect(x: 0, y: 0, width: 640, height: 120))
        container.addSubview(view)
        hosts.append(container)
        NSLayoutConstraint.activate([
            view.leadingAnchor.constraint(equalTo: container.leadingAnchor),
            view.trailingAnchor.constraint(equalTo: container.trailingAnchor),
            view.topAnchor.constraint(equalTo: container.topAnchor),
        ])
        view.apply(theme: dark)
        container.layoutSubtreeIfNeeded()
        return view
    }

    private func descendant<T: NSView>(_ root: NSView, _ id: String, as type: T.Type = NSView.self)
        -> T?
    {
        if root.accessibilityIdentifier() == id, !root.isHidden, let match = root as? T {
            return match
        }
        for child in root.subviews {
            if let match = descendant(child, id, as: type) { return match }
        }
        return nil
    }

    private let click = NSEvent.mouseEvent(
        with: .leftMouseDown, location: .zero, modifierFlags: [], timestamp: 0,
        windowNumber: 0, context: nil, eventNumber: 0, clickCount: 1, pressure: 1)!

    func testThemePacksRustBlockRoles() {
        let theme = seyal_app_theme(0)
        XCTAssertEqual(MemoryLayout<SeyalAppTheme>.size, 36)
        XCTAssertNotEqual(theme.block_focus, theme.accent, "Block focus is its own Rust role")
        XCTAssertEqual(dark.blockFocus, NativeThemeRealization.color(theme.block_focus))
        XCTAssertEqual(dark.blockSuccess, NativeThemeRealization.color(theme.success))
        XCTAssertEqual(dark.blockDanger, NativeThemeRealization.color(theme.danger))
    }

    func testRestBlockHidesActionsAndUsesRestSeam() {
        let view = block()
        XCTAssertFalse(view.actionsVisible, "actions are hidden at rest (board rule 3)")
        XCTAssertEqual(view.layer?.borderWidth, 1)
        XCTAssertEqual(view.layer?.borderColor, dark.blockSeamRest.cgColor)
        XCTAssertEqual(view.layer?.backgroundColor, dark.canvas.cgColor, "surface equals Metal canvas")
        XCTAssertEqual(view.accessibilityValue() as? String, "")
    }

    func testSelectedBlockRevealsActionsWithFocusBorder() {
        let view = block(selected: true)
        XCTAssertTrue(view.actionsVisible)
        XCTAssertEqual(view.layer?.borderWidth, 1.5)
        XCTAssertEqual(view.layer?.borderColor, dark.blockFocus.cgColor)
        XCTAssertEqual(view.accessibilityValue() as? String, "selected")
        XCTAssertEqual(view.accessibilityLabel(), "ls /")
    }

    func testHoverRevealsActionsAndHoverSeam() {
        let view = block()
        view.mouseEntered(with: click)
        XCTAssertTrue(view.actionsVisible)
        XCTAssertEqual(view.layer?.borderColor, dark.blockSeamHover.cgColor)
        view.mouseExited(with: click)
        XCTAssertFalse(view.actionsVisible)
        XCTAssertEqual(view.layer?.borderColor, dark.blockSeamRest.cgColor)
    }

    func testStatusFollowsRustBlockState() {
        let cases: [(UInt32, String, NSColor?)] = [
            (SEYAL_APP_BLOCK_STATE_COMPLETED, "Succeeded", dark.blockSuccess),
            (SEYAL_APP_BLOCK_STATE_FAILED, "Failed", dark.blockDanger),
            (SEYAL_APP_BLOCK_STATE_UNKNOWN, "Status unknown", dark.muted),
        ]
        for (state, name, tint) in cases {
            let view = block(state: state)
            let icon = descendant(view, "seyal-block-status", as: NSImageView.self)
            XCTAssertEqual(icon?.accessibilityLabel(), name)
            XCTAssertFalse(icon?.isHidden ?? true)
            XCTAssertEqual(icon?.contentTintColor, tint)
        }
        let running = block(state: SEYAL_APP_BLOCK_STATE_RUNNING, canRerun: false)
        XCTAssertNil(
            descendant(running, "seyal-block-status", as: NSImageView.self),
            "running hides the status icon")
        let spinner = descendant(running, "seyal-block-status", as: NSProgressIndicator.self)
        XCTAssertEqual(spinner?.accessibilityLabel(), "Running", "the spinner carries the status")
    }

    func testRerunAvailabilityIsProjectedFromRust() {
        let offered = descendant(block(selected: true), "seyal-block-action-rerun", as: NSButton.self)
        XCTAssertEqual(offered?.isEnabled, true)
        let withheld = descendant(
            block(selected: true, canRerun: false), "seyal-block-action-rerun", as: NSButton.self)
        XCTAssertEqual(withheld?.isEnabled, false)
    }

    func testOnlyRealActionsAreShownAndLabelled() {
        let view = block(selected: true)
        for (id, label) in [("copy", "Copy"), ("rerun", "Rerun"), ("more", "More")] {
            let button = descendant(view, "seyal-block-action-\(id)", as: NSButton.self)
            XCTAssertNotNil(button, id)
            XCTAssertEqual(button?.accessibilityLabel(), label)
        }
        for placeholder in ["filter", "workflow", "pin", "collapse"] {
            XCTAssertNil(
                descendant(view, "seyal-block-action-\(placeholder)"),
                "\(placeholder) is not rendered until its own Issue lands (design §9)")
        }
    }

    func testRerunButtonRoutesActionAndBodyClickSelects() {
        let view = block(selected: true)
        var actions: [CommandBlockAction] = []
        var selections: [Bool] = []
        view.onAction = { actions.append($0) }
        view.onSelect = { selections.append($0) }
        let rerun = descendant(view, "seyal-block-action-rerun", as: NSButton.self)
        rerun?.performClick(nil)
        XCTAssertEqual(actions, [.rerun])

        let bodyPoint = view.body.convert(
            NSPoint(x: view.body.bounds.midX, y: view.body.bounds.midY), to: view.superview)
        XCTAssertTrue(view.hitTest(bodyPoint) === view, "the output body selects the Block")
        let rerunPoint = rerun!.convert(
            NSPoint(x: rerun!.bounds.midX, y: rerun!.bounds.midY), to: view.superview)
        XCTAssertTrue(view.hitTest(rerunPoint) === rerun, "seam buttons keep their own clicks")
        view.mouseDown(with: click)
        XCTAssertEqual(selections, [true], "clicking the selected Block toggles it off")
    }
}
