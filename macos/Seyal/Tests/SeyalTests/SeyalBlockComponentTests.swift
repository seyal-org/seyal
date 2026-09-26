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
    private var windows: [NSWindow] = []

    /// Sentinel labels prove the view draws Rust's rows and hardcodes none.
    private func actions(canRerun: Bool) -> [CommandBlockActionRow] {
        func row(_ kind: UInt32, _ placement: UInt32, _ label: String, _ shortcut: String = "",
            enabled: Bool = true) -> CommandBlockActionRow
        {
            CommandBlockActionRow(
                kind: UInt16(kind), placement: UInt16(placement), label: label,
                shortcut: shortcut, enabled: enabled)
        }
        return [
            row(SEYAL_APP_BLOCK_ACTION_COPY_MENU, SEYAL_APP_BLOCK_ACTION_SEAM, "Rust:Copy"),
            row(SEYAL_APP_BLOCK_ACTION_RERUN, SEYAL_APP_BLOCK_ACTION_SEAM, "Rust:Rerun", enabled: canRerun),
            row(SEYAL_APP_BLOCK_ACTION_MORE_MENU, SEYAL_APP_BLOCK_ACTION_SEAM, "Rust:More"),
            row(SEYAL_APP_BLOCK_ACTION_COPY_COMMAND, SEYAL_APP_BLOCK_ACTION_IN_COPY_MENU,
                "Rust:Copy command"),
            row(SEYAL_APP_BLOCK_ACTION_INSPECT, SEYAL_APP_BLOCK_ACTION_IN_MORE_MENU, "Rust:Inspect"),
        ]
    }

    private func block(
        state: UInt32 = SEYAL_APP_BLOCK_STATE_COMPLETED,
        selected: Bool = false,
        canRerun: Bool = true,
        statusLabel: String = "Rust:Status"
    ) -> CommandBlockView {
        let view = CommandBlockView(
            row: CommandBlockRow(
                command: "ls /",
                state: UInt16(state),
                statusLabel: statusLabel,
                isSelected: selected,
                actions: actions(canRerun: canRerun)
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
        let theme = seyal_app_theme(0, 0)
        XCTAssertEqual(MemoryLayout<SeyalAppTheme>.size, 36)
        XCTAssertNotEqual(theme.block_focus, theme.accent, "Block focus is its own Rust role")
        XCTAssertEqual(dark.blockFocus, NativeThemeRealization.color(theme.block_focus))
        XCTAssertEqual(dark.blockSuccess, NativeThemeRealization.color(theme.success))
        XCTAssertEqual(dark.blockDanger, NativeThemeRealization.color(theme.danger))
        XCTAssertEqual(theme.reserved & 1, 1, "default signals allow motion")
        let reduced = seyal_app_theme(0, 1)
        XCTAssertEqual(reduced.reserved & 1, 0, "reduce_motion clears allows_motion")
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
            (SEYAL_APP_BLOCK_STATE_COMPLETED, "Rust:Succeeded", dark.blockSuccess),
            (SEYAL_APP_BLOCK_STATE_FAILED, "Rust:Failed", dark.blockDanger),
            (SEYAL_APP_BLOCK_STATE_UNKNOWN, "Rust:Unknown", dark.muted),
        ]
        for (state, name, tint) in cases {
            let view = block(state: state, statusLabel: name)
            let icon = descendant(view, "seyal-block-status", as: NSImageView.self)
            XCTAssertEqual(icon?.accessibilityLabel(), name)
            XCTAssertFalse(icon?.isHidden ?? true)
            XCTAssertEqual(icon?.contentTintColor, tint)
        }
        let running = block(
            state: SEYAL_APP_BLOCK_STATE_RUNNING, canRerun: false, statusLabel: "Rust:Running")
        XCTAssertNil(
            descendant(running, "seyal-block-status", as: NSImageView.self),
            "running hides the status icon")
        let spinner = descendant(running, "seyal-block-status", as: NSProgressIndicator.self)
        XCTAssertEqual(spinner?.accessibilityLabel(), "Rust:Running", "the spinner carries the status")
    }

    func testReduceMotionStopsRunningSpinner() {
        let theme = NativeTheme(
            canvas: dark.canvas, container: dark.container, utility: dark.utility,
            elevated: dark.elevated, text: dark.text, secondary: dark.secondary, muted: dark.muted,
            accent: dark.accent, seam: dark.seam, success: dark.success, warning: dark.warning,
            danger: dark.danger, blockFocus: dark.blockFocus, blockSeamRest: dark.blockSeamRest,
            blockSeamHover: dark.blockSeamHover, blockSuccess: dark.blockSuccess,
            blockDanger: dark.blockDanger, allowsMotion: false, appearance: dark.appearance,
            uiFontSize: dark.uiFontSize, terminalFontSize: dark.terminalFontSize,
            windowPadding: dark.windowPadding, terminalPadding: dark.terminalPadding,
            reduceMaterial: dark.reduceMaterial, utilityMaterial: dark.utilityMaterial,
            utilityOpacity: dark.utilityOpacity)
        let view = block(
            state: SEYAL_APP_BLOCK_STATE_RUNNING, canRerun: false, statusLabel: "Rust:Running")
        view.apply(theme: theme)
        let spinner = descendant(view, "seyal-block-status", as: NSProgressIndicator.self)
        XCTAssertEqual(spinner?.isDisplayedWhenStopped, true)
        XCTAssertFalse(spinner?.isHidden ?? true)
    }

    func testSelectedBlockActionsAreInKeyViewLoop() {
        let view = block(selected: true)
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 640, height: 120),
            styleMask: [.titled], backing: .buffered, defer: false)
        window.contentView = view.superview
        windows.append(window)
        XCTAssertTrue(view.acceptsFirstResponder)
        XCTAssertTrue(window.makeFirstResponder(view))
        let copy = descendant(view, "seyal-block-action-copy", as: NSButton.self)
        XCTAssertNotNil(copy)
        XCTAssertFalse(copy?.refusesFirstResponder ?? true)
        XCTAssertEqual(copy?.accessibilityLabel(), "Rust:Copy")
        XCTAssertTrue(view.accessibilityPerformPress())
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
        for (id, label) in [("copy", "Rust:Copy"), ("rerun", "Rust:Rerun"), ("more", "Rust:More")] {
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
        var actions: [UInt16] = []
        var selections: [Bool] = []
        view.onAction = { actions.append($0) }
        view.onSelect = { selections.append($0) }
        let rerun = descendant(view, "seyal-block-action-rerun", as: NSButton.self)
        rerun?.performClick(nil)
        XCTAssertEqual(actions, [UInt16(SEYAL_APP_BLOCK_ACTION_RERUN)])

        let bodyPoint = view.body.convert(
            NSPoint(x: view.body.bounds.midX, y: view.body.bounds.midY), to: view.superview)
        XCTAssertTrue(view.hitTest(bodyPoint) === view, "the output body selects the Block")
        let rerunPoint = rerun!.convert(
            NSPoint(x: rerun!.bounds.midX, y: rerun!.bounds.midY), to: view.superview)
        XCTAssertTrue(view.hitTest(rerunPoint) === rerun, "seam buttons keep their own clicks")
        view.mouseDown(with: click)
        XCTAssertEqual(selections, [true], "clicking the selected Block toggles it off")
    }

    func testRustShortcutHintsMapToAppKitKeyEquivalents() {
        let copy = CommandBlockView.keyEquivalent("cmd+c")
        XCTAssertEqual(copy.0, "c")
        XCTAssertEqual(copy.1, [.command])
        let both = CommandBlockView.keyEquivalent("alt+cmd+c")
        XCTAssertEqual(both.1, [.option, .command])
        XCTAssertEqual(CommandBlockView.keyEquivalent("").0, "")
    }

    /// Visual-regression matrix (M003-BLOCK-COMPONENT-DESIGN §11) for the
    /// AppKit chrome in both Rust themes. The shipping window is pinned to
    /// dark (AppDelegate), so light is evidenced here, offscreen; Metal
    /// terminal rows are not part of this render.
    func testBlockChromeStateMatrixRendersInDarkAndLight() throws {
        let appearances: [(String, NSAppearance.Name)] = [("dark", .darkAqua), ("light", .aqua)]
        let states: [(String, UInt32, Bool, Bool)] = [
            ("rest", SEYAL_APP_BLOCK_STATE_COMPLETED, false, false),
            ("hover", SEYAL_APP_BLOCK_STATE_COMPLETED, false, true),
            ("selected", SEYAL_APP_BLOCK_STATE_COMPLETED, true, false),
            ("running", SEYAL_APP_BLOCK_STATE_RUNNING, false, false),
            ("failed", SEYAL_APP_BLOCK_STATE_FAILED, false, false),
        ]
        for (appearanceName, name) in appearances {
            let appearance = try XCTUnwrap(NSAppearance(named: name))
            let theme = NativeThemeRealization.theme(for: appearance)
            var pixels: [CGColor] = []
            for (stateName, state, selected, hovered) in states {
                let view = block(state: state, selected: selected, canRerun: state != SEYAL_APP_BLOCK_STATE_RUNNING)
                let host = try XCTUnwrap(view.superview)
                // A window makes AppKit actually draw the layer tree
                // (icons, labels, border); it is never ordered on screen.
                let window = NSWindow(
                    contentRect: host.frame, styleMask: [.borderless], backing: .buffered, defer: false)
                window.appearance = appearance
                host.wantsLayer = true
                host.layer?.backgroundColor = theme.canvas.cgColor
                window.contentView = host
                view.apply(theme: theme)
                if hovered { view.mouseEntered(with: click) }
                host.layoutSubtreeIfNeeded()
                window.displayIfNeeded()
                let image = try XCTUnwrap(render(host))
                windows.append(window)
                let attachment = XCTAttachment(image: image)
                attachment.name = "1010-matrix-\(appearanceName)-\(stateName)"
                attachment.lifetime = .keepAlways
                add(attachment)
                XCTAssertEqual(view.layer?.backgroundColor, theme.canvas.cgColor)
                pixels.append(try XCTUnwrap(view.layer?.borderColor))
            }
            XCTAssertEqual(pixels[2], theme.blockFocus.cgColor, "\(appearanceName) selected border")
            XCTAssertEqual(pixels[1], theme.blockSeamHover.cgColor, "\(appearanceName) hover border")
        }
        XCTAssertNotEqual(
            NativeThemeRealization.theme(for: NSAppearance(named: .aqua)!).canvas,
            dark.canvas, "light matrix uses the light Rust palette")
    }

    private func render(_ view: NSView) -> NSImage? {
        let scale: CGFloat = 2
        let size = view.bounds.size
        guard let layer = view.layer,
            let context = CGContext(
                data: nil, width: Int(size.width * scale), height: Int(size.height * scale),
                bitsPerComponent: 8, bytesPerRow: 0, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
        else { return nil }
        context.scaleBy(x: scale, y: scale)
        view.displayIfNeeded()
        layer.render(in: context)
        guard let cgImage = context.makeImage() else { return nil }
        return NSImage(cgImage: cgImage, size: size)
    }
}
