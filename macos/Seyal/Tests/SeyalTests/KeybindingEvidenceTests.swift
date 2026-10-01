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

    /// §14.4: unmatched Command is ApplicationCommand-consumed (zero PTY, no
    /// menu key-equivalent steal); reserved stays on the native-Command path.
    func testUnmatchedAndReservedCommandAreNativeCommandNotFallthrough() {
        let handle = seyal_app_create()
        defer { XCTAssertEqual(seyal_app_destroy(handle), 0) }

        let unmatched = seyal_app_route_keystroke(
            handle, 1, 0, keyU, 0, 0, 0
        )
        XCTAssertEqual(unmatched, 1, "unmatched Command → consumed (R6.2.1, no menu steal)")

        let reserved = seyal_app_route_keystroke(
            handle, 1, 0, keyQ, 0, 0, 0
        )
        XCTAssertEqual(reserved, 2, "reserved cmd+q → native Command handling")
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
}
