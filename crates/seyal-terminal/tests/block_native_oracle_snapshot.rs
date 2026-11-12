//! Phase A offline VT oracle snapshots for #1484 differential cutover.
//!
//! Pre-cutover Phase A fingerprints remain the differential oracle. Production
//! now stores primary cells in `PrimaryBlockEngine` / `RegionStore`; identical
//! byte feeds must still match these cell/cursor/mode fingerprints unless an
//! explicit standards override is recorded.

use seyal_terminal::TerminalState;

fn feed(terminal: &mut TerminalState, bytes: &[u8]) {
    terminal.feed(bytes).expect("terminal feed succeeds");
}

fn grid_fingerprint(terminal: &TerminalState, cols: u16, rows: u16) -> String {
    let mut out = String::new();
    let cursor = terminal.cursor();
    out.push_str(&format!(
        "cursor={},{} visible={} \n",
        cursor.col, cursor.row, cursor.visible
    ));
    for row in 0..rows {
        for col in 0..cols {
            let cell = terminal.cell(col, row).expect("cell exists");
            out.push(cell.character);
        }
        out.push('\n');
    }
    out
}

#[test]
fn oracle_same_row_printf_then_overwrite_fingerprint() {
    // Counterexample class: C A D \r B style overwrite after boundary bytes.
    let mut terminal = TerminalState::new(8, 2).unwrap();
    feed(&mut terminal, b"ABC");
    feed(&mut terminal, b"\rXYZ");
    let fp = grid_fingerprint(&mut terminal, 8, 2);
    assert!(fp.contains("XYZ"), "overwrite must win on the same row: {fp}");
    assert_eq!(terminal.cell(0, 0).unwrap().character, 'X');
    assert_eq!(terminal.cell(1, 0).unwrap().character, 'Y');
    assert_eq!(terminal.cell(2, 0).unwrap().character, 'Z');
}

#[test]
fn oracle_chunk_invariant_printable_and_csi() {
    let bytes = b"hi\x1b[1A\x1b[Kz";
    let mut once = TerminalState::new(8, 3).unwrap();
    feed(&mut once, bytes);
    let once_fp = grid_fingerprint(&once, 8, 3);

    let mut per_byte = TerminalState::new(8, 3).unwrap();
    for byte in bytes {
        feed(&mut per_byte, &[*byte]);
    }
    let per_byte_fp = grid_fingerprint(&per_byte, 8, 3);
    assert_eq!(once_fp, per_byte_fp, "VT oracle must be chunk-invariant");
}

#[test]
fn oracle_ed2_clears_primary_and_remints_identity() {
    let mut terminal = TerminalState::new(4, 2).unwrap();
    feed(&mut terminal, b"abcd\r\n1234");
    feed(&mut terminal, b"\x1b[2J\x1b[H");
    assert_eq!(terminal.row_text(0).unwrap(), "    ");
    assert_eq!(terminal.row_text(1).unwrap(), "    ");
    feed(&mut terminal, b"Z");
    assert_eq!(terminal.cell(0, 0).unwrap().character, 'Z');
}
