import AppKit
import XCTest

@testable import Seyal

/// SPEC-024 K6 / #1138: native component evidence the headed host can drive
/// without a full XCUI session (shortcut projection + route FFI).
final class KeybindingEvidenceTests: XCTestCase {
    private let keyK: UInt32 = 0x6b // 'k'
    private let keyT: UInt32 = 0x74 // 't'
    private let keyC: UInt32 = 0x63 // 'c'
    private let keyU: UInt32 = 0x75 // 'u'
    private let keyQ: UInt32 = 0x71 // 'q'

    /// §14.12 / acceptance: menu equivalents come from the Rust projection.
    func testMenuEquivalentsComeFromRustProjection() throws {
        let palette = try XCTUnwrap(
            KeybindingShortcutRealization.item(
                commandId: KeybindingShortcutRealization.commandPaletteOpen
            ),
            "cold projection must expose Command Palette"
        )
        XCTAssertEqual(palette.has_key_equivalent, 1)
        XCTAssertEqual(palette.modifier_bits & 1, 1, "CMD bit")
        XCTAssertEqual(palette.key_is_named, 0)
        XCTAssertEqual(palette.key_base, keyK)

        let newTab = try XCTUnwrap(
            KeybindingShortcutRealization.item(
                commandId: KeybindingShortcutRealization.tabCreate
            )
        )
        XCTAssertEqual(newTab.has_key_equivalent, 1)
        XCTAssertEqual(newTab.key_base, keyT)

        let item = NSMenuItem(title: "", action: nil, keyEquivalent: "")
        KeybindingShortcutRealization.realize(
            item, commandId: KeybindingShortcutRealization.commandPaletteOpen
        )
        XCTAssertEqual(item.keyEquivalent, "k")
        XCTAssertTrue(item.keyEquivalentModifierMask.contains(.command))
        XCTAssertEqual(item.title, "Command Palette")
    }

    /// §14.4 / acceptance: ApplicationCommand match is consumed (zero PTY path).
    func testApplicationCommandRouteConsumesWithoutFallthrough() {
        let handle = seyal_app_create()
        defer { XCTAssertEqual(seyal_app_destroy(handle), 0) }
        let code = seyal_app_route_keystroke(
            handle,
            1, // CMD
            0,
            keyK,
            0,
            0,
            0
        )
        XCTAssertEqual(code, 1, "matched ApplicationCommand must return consumed")
    }

    /// §14.7 / adversarial: composition skips non-Command; Command still matches.
    func testCompositionSkipsNonCommandButCommandStillMatches() {
        let handle = seyal_app_create()
        defer { XCTAssertEqual(seyal_app_destroy(handle), 0) }

        let escapeWhileComposing = seyal_app_route_keystroke(
            handle,
            0,
            1, // named
            3, // Escape
            0,
            0,
            1 // composition_active
        )
        XCTAssertEqual(
            escapeWhileComposing, 0,
            "composition owns Escape → fallthrough to IME, not a binding fire"
        )

        let cmdWhileComposing = seyal_app_route_keystroke(
            handle,
            1,
            0,
            keyK,
            0,
            0,
            1
        )
        XCTAssertEqual(
            cmdWhileComposing, 1,
            "Command binding still resolves while composition is active"
        )
    }

    /// §14.4 / §6.2 step 2c: ordinary unmatched Command is native (composer
    /// editing); reserved stays native; inactive projected menu equivalents
    /// still consume so AppKit cannot steal across contexts (R6.2.1).
    func testUnmatchedAndReservedCommandAreNativeCommandNotFallthrough() {
        let handle = seyal_app_create()
        defer { XCTAssertEqual(seyal_app_destroy(handle), 0) }

        let unmatched = seyal_app_route_keystroke(
            handle, 1, 0, keyU, 0, 0, 0
        )
        XCTAssertEqual(unmatched, 2, "ordinary unmatched Command → native (§6.2 step 2c)")

        let cmdLeft = seyal_app_route_keystroke(
            handle, 1, 1, 7, 0, 0, 0 // named Left
        )
        XCTAssertEqual(cmdLeft, 2, "unbound Cmd+Left → native text editing")

        let reserved = seyal_app_route_keystroke(
            handle, 1, 0, keyQ, 0, 0, 0
        )
        XCTAssertEqual(reserved, 2, "reserved cmd+q → native Command handling")

        // Palette owns the route: cmd+t misses, but New Tab's projected ⌘T
        // would still fire if we returned native — consume that cross-context
        // steal only.
        XCTAssertEqual(
            seyal_app_route_keystroke(handle, 1, 0, keyK, 0, 0, 0),
            1,
            "cmd+k opens palette"
        )
        let menuSteal = seyal_app_route_keystroke(handle, 1, 0, keyT, 0, 0, 0)
        XCTAssertEqual(
            menuSteal, 1,
            "palette-open cmd+t → consumed (inactive New Tab menu equivalent)"
        )
        XCTAssertEqual(
            seyal_app_shell(handle).tab_count, 1,
            "menu-steal consume must not create a tab"
        )
    }

    /// §14.5 / Raw-TUI forwarding: Control-C and arrows fall through to the terminal path.
    func testTuiPassthroughControlCAndArrowsFallThrough() {
        let handle = seyal_app_create()
        defer { XCTAssertEqual(seyal_app_destroy(handle), 0) }

        let snap = seyal_app_snapshot(handle)
        var bind = SeyalAppAction()
        bind.version = UInt16(SEYAL_APP_ABI_VERSION)
        bind.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        bind.kind = UInt16(SEYAL_APP_ACTION_BIND.rawValue)
        bind.flags = UInt16(SEYAL_APP_FLAG_TARGET_CONTROLLER)
        bind.fence_pane_lo = snap.pane_lo
        bind.fence_pane_hi = snap.pane_hi
        bind.fence_epoch = snap.epoch
        bind.target_execution_lo = 1
        bind.target_attachment_lo = 2
        bind.target_pty_generation = 1
        XCTAssertEqual(seyal_app_apply(handle, &bind), 0)

        var refresh = SeyalAppAction()
        refresh.version = bind.version
        refresh.size = bind.size
        refresh.kind = UInt16(SEYAL_APP_ACTION_REFRESH.rawValue)
        refresh.applySnapshotFence(seyal_app_snapshot(handle))
        refresh.flags |= UInt16(SEYAL_APP_FLAG_ALTERNATE_SCREEN)
        XCTAssertEqual(seyal_app_apply(handle, &refresh), 0)
        XCTAssertEqual(
            seyal_app_snapshot(handle).eligibility,
            UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue)
        )

        let ctrlC = seyal_app_route_keystroke(
            handle, 2, 0, keyC, 0, 0, 0
        )
        XCTAssertEqual(ctrlC, 0, "Control-C must fall through in TUI")

        for named: UInt32 in [5, 6, 7, 8] { // Up Down Left Right
            let code = seyal_app_route_keystroke(handle, 0, 1, named, 0, 0, 0)
            XCTAssertEqual(code, 0, "arrow named=\(named) must fall through")
        }
    }

    // MARK: - SPEC-024 §5.6 chrome visibility toggles (#922)

    private func commandKey(_ flags: NSEvent.ModifierFlags) throws -> NSEvent {
        try XCTUnwrap(NSEvent.keyEvent(
            with: .keyDown,
            location: .zero,
            modifierFlags: flags,
            timestamp: 0,
            windowNumber: 0,
            context: nil,
            characters: flags.contains(.option) ? "º" : "0",
            charactersIgnoringModifiers: "0",
            isARepeat: false,
            keyCode: 29 // kVK_ANSI_0
        ))
    }

    /// §14 item 23 / R11.2: the View-menu toggles carry the projected keys.
    func testChromeToggleMenuItemsRealizeProjectedKeyEquivalents() throws {
        let rows: [(UInt16, NSEvent.ModifierFlags)] = [
            (KeybindingShortcutRealization.chromeToggleLeftPanel, [.command]),
            (KeybindingShortcutRealization.chromeToggleInspector, [.command, .option]),
        ]
        for (commandId, mask) in rows {
            let row = try XCTUnwrap(KeybindingShortcutRealization.item(commandId: commandId))
            XCTAssertEqual(row.has_key_equivalent, 1)
            XCTAssertEqual(row.key_base, UInt32(UnicodeScalar("0").value))
            let item = NSMenuItem(title: "", action: nil, keyEquivalent: "")
            KeybindingShortcutRealization.realize(item, commandId: commandId)
            XCTAssertEqual(item.keyEquivalent, "0")
            XCTAssertEqual(item.keyEquivalentModifierMask, mask)
        }
    }

    /// R5.6.2: titles come from Rust visibility, refreshed at menu validation.
    @MainActor
    func testChromeToggleMenuTitlesFollowRustVisibility() {
        let view = ProductChromeHostView(frame: NSRect(x: 0, y: 0, width: 1200, height: 760))
        let sidebar = NSMenuItem(
            title: "",
            action: #selector(ProductChromeHostView.invokeProjectedWorkspaceCommand(_:)),
            keyEquivalent: ""
        )
        sidebar.target = view
        KeybindingShortcutRealization.realize(
            sidebar, commandId: KeybindingShortcutRealization.chromeToggleLeftPanel
        )
        let inspector = NSMenuItem(title: "", action: nil, keyEquivalent: "")
        KeybindingShortcutRealization.realize(
            inspector, commandId: KeybindingShortcutRealization.chromeToggleInspector
        )
        XCTAssertTrue(view.validateMenuItem(sidebar))
        XCTAssertTrue(view.validateMenuItem(inspector))
        XCTAssertEqual(sidebar.title, "Hide Sidebar")
        XCTAssertEqual(inspector.title, "Hide Inspector")

        view.invokeProjectedWorkspaceCommand(sidebar)
        XCTAssertTrue(view.validateMenuItem(sidebar))
        XCTAssertTrue(view.validateMenuItem(inspector))
        XCTAssertEqual(sidebar.title, "Show Sidebar")
        XCTAssertEqual(inspector.title, "Hide Inspector")
        XCTAssertTrue(view.left.isHidden)
    }

    /// #922 "visibility toggles reclaim layout": ⌘0 and ⌥⌘0 through the real
    /// key path hide one column each, the center column takes its space, and a
    /// second press restores the original layout.
    @MainActor
    func testCommandZeroTogglesReclaimCenterLayoutAndRestore() throws {
        let view = ProductChromeHostView(frame: NSRect(x: 0, y: 0, width: 1200, height: 760))
        view.reconcileChrome()
        view.layoutSubtreeIfNeeded()
        let original = view.centerColumn.frame
        XCTAssertFalse(view.left.isHidden)
        XCTAssertFalse(view.inspectorColumn.isHidden)

        XCTAssertTrue(view.performKeyEquivalent(with: try commandKey(.command)))
        view.layoutSubtreeIfNeeded()
        let withoutLeft = view.centerColumn.frame
        XCTAssertTrue(view.left.isHidden)
        XCTAssertFalse(view.inspectorColumn.isHidden)
        XCTAssertLessThan(withoutLeft.minX, original.minX, "center takes the sidebar space")
        XCTAssertEqual(withoutLeft.maxX, original.maxX, accuracy: 0.5)

        XCTAssertTrue(view.performKeyEquivalent(with: try commandKey([.command, .option])))
        view.layoutSubtreeIfNeeded()
        let centerOnly = view.centerColumn.frame
        XCTAssertTrue(view.inspectorColumn.isHidden)
        XCTAssertGreaterThan(centerOnly.maxX, withoutLeft.maxX, "center takes the inspector space")
        XCTAssertEqual(centerOnly.minX, withoutLeft.minX, accuracy: 0.5)

        XCTAssertTrue(view.performKeyEquivalent(with: try commandKey(.command)))
        XCTAssertTrue(view.performKeyEquivalent(with: try commandKey([.command, .option])))
        view.layoutSubtreeIfNeeded()
        XCTAssertFalse(view.left.isHidden)
        XCTAssertFalse(view.inspectorColumn.isHidden)
        XCTAssertEqual(view.centerColumn.frame, original, "second presses restore the layout")
        XCTAssertEqual(seyal_app_shell(view.pane.appHandle).tab_count, 1, "no tab side effect")
    }
}
