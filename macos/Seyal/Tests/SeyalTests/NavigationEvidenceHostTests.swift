import XCTest

@testable import Seyal

/// N6 headed host evidence (SPEC-022 §12 items 31–33) using the existing
/// SeyalTests FFI harness. Default M001 shell disallows SplitFocused/CreateTab,
/// so a second Pane comes from CreateWindow (already on this branch). Real PTY
/// multi-pane executions remain harness-limited; see
/// docs/evidence/m003-n6-navigation-1156.md.
final class NavigationEvidenceHostTests: XCTestCase {
    private func makeHandle() -> UInt64 {
        let handle = seyal_app_create()
        XCTAssertNotEqual(handle, 0)
        return handle
    }

    private func bindSyntheticExecution(_ handle: UInt64, tag: UInt64 = 0x1156) {
        let snap = seyal_app_snapshot(handle)
        var bind = SeyalAppAction()
        bind.version = UInt16(SEYAL_APP_ABI_VERSION)
        bind.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        bind.kind = UInt16(SEYAL_APP_ACTION_BIND.rawValue)
        bind.flags = UInt16(SEYAL_APP_FLAG_TARGET_CONTROLLER)
        bind.fence_pane_lo = snap.pane_lo
        bind.fence_pane_hi = snap.pane_hi
        bind.fence_epoch = snap.epoch
        bind.target_execution_lo = tag
        bind.target_execution_hi = tag
        bind.target_attachment_lo = tag &+ 1
        bind.target_attachment_hi = tag &+ 1
        bind.target_pty_generation = 1
        XCTAssertEqual(seyal_app_apply(handle, &bind), 0)
    }

    private func apply(
        _ handle: UInt64,
        kind: UInt16,
        reserved: UInt32 = 0,
        payload: Data? = nil,
        targetLo: UInt64 = 0,
        targetHi: UInt64 = 0
    ) -> Int32 {
        let snap = seyal_app_snapshot(handle)
        var action = SeyalAppAction()
        action.version = UInt16(SEYAL_APP_ABI_VERSION)
        action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
        action.kind = kind
        action.applySnapshotFence(snap)
        action.reserved = reserved
        action.target_execution_lo = targetLo
        action.target_execution_hi = targetHi
        guard let payload else {
            return seyal_app_apply(handle, &action)
        }
        return payload.withUnsafeBytes { buffer in
            action.payload = buffer.bindMemory(to: UInt8.self).baseAddress
            action.payload_len = UInt32(payload.count)
            return seyal_app_apply(handle, &action)
        }
    }

    private func ackPendingEffects(_ handle: UInt64) {
        for _ in 0..<16 {
            let snap = seyal_app_snapshot(handle)
            guard snap.pending_effect != 0 else { return }
            XCTAssertEqual(apply(handle, kind: UInt16(SEYAL_APP_ACTION_ACK_EFFECT.rawValue)), 0)
        }
    }

    private func focusedPane(_ handle: UInt64) -> (lo: UInt64, hi: UInt64) {
        let shell = seyal_app_shell(handle)
        return (shell.focused_pane_lo, shell.focused_pane_hi)
    }

    /// Second Pane via CreateWindow — SplitFocused is disallowed on M001 default.
    /// Focus identity is on `seyal_app_shell` (snapshot.pane is the fence/bound Pane).
    private func createSecondWindowPane(_ handle: UInt64) -> (lo: UInt64, hi: UInt64) {
        let before = focusedPane(handle)
        let shell = seyal_app_shell(handle)
        XCTAssertEqual(
            apply(
                handle,
                kind: UInt16(SEYAL_APP_ACTION_CREATE_WINDOW.rawValue),
                targetLo: shell.last_active_workspace_lo,
                targetHi: shell.last_active_workspace_hi
            ),
            0
        )
        ackPendingEffects(handle)
        let after = focusedPane(handle)
        XCTAssertTrue(
            after.lo != before.lo || after.hi != before.hi,
            "CreateWindow must focus a new Pane"
        )
        XCTAssertEqual(seyal_app_shell(handle).window_count, 2)
        return after
    }

    private func packAddress(from row: SeyalAppRow) -> Data {
        var payload = Data()
        var version = row.address_version.littleEndian
        var kind = row.address_kind.littleEndian
        payload.append(Data(bytes: &version, count: 2))
        payload.append(Data(bytes: &kind, count: 2))
        withUnsafeBytes(of: row.address_bytes) { bytes in
            payload.append(contentsOf: bytes.prefix(Int(row.address_len)))
        }
        return payload
    }

    private func paneWords(from row: SeyalAppRow) -> (UInt64, UInt64)? {
        guard row.address_len >= 48 else { return nil }
        let bytes = withUnsafeBytes(of: row.address_bytes) { Array($0.prefix(Int(row.address_len))) }
        var lo: UInt64 = 0
        var hi: UInt64 = 0
        withUnsafeMutableBytes(of: &lo) { dest in
            dest.copyBytes(from: bytes[32..<40])
        }
        withUnsafeMutableBytes(of: &hi) { dest in
            dest.copyBytes(from: bytes[40..<48])
        }
        return (UInt64(littleEndian: lo), UInt64(littleEndian: hi))
    }

    private func focusSeqPayload(_ raw: UInt64) -> Data {
        var le = raw.littleEndian
        return Data(bytes: &le, count: 8)
    }

    private func navigateGotoToPane(_ handle: UInt64, paneLo: UInt64, paneHi: UInt64) {
        XCTAssertEqual(
            apply(
                handle,
                kind: UInt16(SEYAL_APP_ACTION_OPEN_GOTO.rawValue),
                reserved: UInt32(SEYAL_APP_GOTO_PANES.rawValue)
            ),
            0
        )
        let palette = seyal_app_palette(handle)
        for index in 0..<Int(palette.row_count) {
            let row = seyal_app_palette_row(handle, UInt32(index))
            guard let words = paneWords(from: row), words.0 == paneLo, words.1 == paneHi else {
                continue
            }
            let delta = Int32(index) - Int32(seyal_app_palette(handle).selected)
            if delta != 0 {
                XCTAssertEqual(
                    apply(
                        handle,
                        kind: UInt16(SEYAL_APP_ACTION_MOVE_PALETTE_SELECTION.rawValue),
                        reserved: UInt32(bitPattern: delta)
                    ),
                    0
                )
            }
            XCTAssertEqual(
                apply(
                    handle,
                    kind: UInt16(SEYAL_APP_ACTION_RUN_PALETTE.rawValue),
                    payload: packAddress(from: row)
                ),
                0
            )
            ackPendingEffects(handle)
            return
        }
        XCTFail("goto panes scope missing Pane \(paneLo)/\(paneHi)")
    }

    /// Item 31 (host path): OpenGoto → run address-carrying row focuses that Pane.
    func testGotoSelectionFocusesIntendedPane() {
        let handle = makeHandle()
        defer { XCTAssertEqual(seyal_app_destroy(handle), 0) }
        bindSyntheticExecution(handle)
        let paneA = focusedPane(handle)
        let paneB = createSecondWindowPane(handle)

        navigateGotoToPane(handle, paneLo: paneA.lo, paneHi: paneA.hi)
        let focused = focusedPane(handle)
        XCTAssertEqual(focused.lo, paneA.lo)
        XCTAssertEqual(focused.hi, paneA.hi)
        let snap = seyal_app_snapshot(handle)
        XCTAssertEqual(
            snap.flags & UInt16(SEYAL_APP_SNAP_HAS_EXECUTION),
            UInt16(SEYAL_APP_SNAP_HAS_EXECUTION)
        )
        XCTAssertEqual(snap.execution_lo, 0x1156)
        XCTAssertEqual(snap.execution_hi, 0x1156)
        XCTAssertTrue(paneB.lo != paneA.lo || paneB.hi != paneA.hi)
    }

    /// Item 32 (host path): Navigate away and back keeps the same ExecutionId.
    func testNavigateAwayAndBackPreservesExecutionIdentity() {
        let handle = makeHandle()
        defer { XCTAssertEqual(seyal_app_destroy(handle), 0) }
        bindSyntheticExecution(handle, tag: 0xA11E)
        let bound = seyal_app_snapshot(handle)
        XCTAssertEqual(bound.execution_lo, 0xA11E)
        XCTAssertEqual(bound.execution_hi, 0xA11E)
        let paneA = focusedPane(handle)

        let paneB = createSecondWindowPane(handle)
        XCTAssertTrue(paneB.lo != paneA.lo || paneB.hi != paneA.hi)

        navigateGotoToPane(handle, paneLo: paneA.lo, paneHi: paneA.hi)
        let backFocus = focusedPane(handle)
        XCTAssertEqual(backFocus.lo, paneA.lo)
        XCTAssertEqual(backFocus.hi, paneA.hi)
        let back = seyal_app_snapshot(handle)
        XCTAssertEqual(back.execution_lo, 0xA11E)
        XCTAssertEqual(back.execution_hi, 0xA11E)
        XCTAssertEqual(back.epoch, bound.epoch, "Navigate must not restart presentation identity")
    }

    /// Item 33 (host path): HistoryBack/Forward use N3 actions (codes 65/66).
    /// FocusSeq starts at 1 on a fresh handle; cursor is not yet on the FFI
    /// snapshot, so this test uses the deterministic empty-store sequence.
    func testHistoryBackForwardTraverseRecordedTargets() {
        let handle = makeHandle()
        defer { XCTAssertEqual(seyal_app_destroy(handle), 0) }
        bindSyntheticExecution(handle)
        let paneA = focusedPane(handle)
        let paneB = createSecondWindowPane(handle)

        // CreateWindow focused B without recording history. Record A then B.
        navigateGotoToPane(handle, paneLo: paneA.lo, paneHi: paneA.hi) // FocusSeq 1
        navigateGotoToPane(handle, paneLo: paneB.lo, paneHi: paneB.hi) // FocusSeq 2
        XCTAssertEqual(focusedPane(handle).lo, paneB.lo)
        XCTAssertEqual(focusedPane(handle).hi, paneB.hi)

        XCTAssertEqual(
            apply(
                handle,
                kind: UInt16(SEYAL_APP_ACTION_HISTORY_BACK.rawValue),
                payload: focusSeqPayload(2)
            ),
            0
        )
        ackPendingEffects(handle)
        let afterBack = focusedPane(handle)
        XCTAssertEqual(afterBack.lo, paneA.lo)
        XCTAssertEqual(afterBack.hi, paneA.hi)

        XCTAssertEqual(
            apply(
                handle,
                kind: UInt16(SEYAL_APP_ACTION_HISTORY_FORWARD.rawValue),
                payload: focusSeqPayload(1)
            ),
            0
        )
        ackPendingEffects(handle)
        let afterForward = focusedPane(handle)
        XCTAssertEqual(afterForward.lo, paneB.lo)
        XCTAssertEqual(afterForward.hi, paneB.hi)

        XCTAssertEqual(
            apply(
                handle,
                kind: UInt16(SEYAL_APP_ACTION_HISTORY_BACK.rawValue),
                payload: focusSeqPayload(999)
            ),
            -4
        )
        XCTAssertEqual(seyal_app_last_error(handle), 48)
        XCTAssertEqual(focusedPane(handle).lo, paneB.lo)
    }
}
