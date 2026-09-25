//! #824 retained deterministic workload fixtures.
//!
//! These sequences stand in for shells, SSH, TUIs, DevOps CLIs, high-volume
//! logs, and agent-style TUIs without headed GUI, a second VT engine, or
//! fabricating native/IME/visual evidence. Headed/manual rows live in
//! `docs/evidence/m002-824-headed-manual.md`.

use seyal_terminal::{
    encode_mouse_report, encode_paste, mouse_takes_host_override, CellRole, Color,
    HostPresentationEvent, LineId, MouseEventKind, MouseReport, MouseReporting, PasteError,
    TerminalState, VisualPos, HISTORY_PER_EXECUTION_BYTE_CAP, MAX_HOST_PRESENTATION_EVENTS,
    MAX_PASTE_BYTES, MAX_PRESENTATION_PAYLOAD_BYTES, MAX_PROTOCOL_REPLIES,
};

fn feed(terminal: &mut TerminalState, bytes: &[u8]) {
    terminal.feed(bytes).expect("feed succeeds");
}

fn visible(terminal: &TerminalState) -> String {
    let mut out = String::new();
    for row in 0..terminal.rows() {
        out.push_str(terminal.row_text(row).expect("row").trim_end());
        out.push('\n');
    }
    out
}

fn contains_visible(terminal: &TerminalState, needle: &str) -> bool {
    visible(terminal).contains(needle)
}

#[test]
fn zsh_bash_fish_prompt_color_clear_and_multiline_stay_one_authority() {
    let mut terminal = TerminalState::new(40, 6).unwrap();
    // Representative zsh/bash/fish OSC title + CWD + colored prompt + clear +
    // multiline continuation. One TerminalState; no extra pane identity.
    feed(
        &mut terminal,
        b"\x1b]0;user@host:~\x07\x1b]7;file:///Users/dev\x07",
    );
    feed(&mut terminal, b"\x1b[32m% \x1b[0mecho hello\r\nhello\r\n");
    feed(&mut terminal, b"\x1b[34m$ \x1b[0mprintf 'ok'\r\nok\r\n");
    feed(
        &mut terminal,
        b"\x1b[36m> \x1b[0mline-one\\\r\nline-two\r\n",
    );
    assert!(contains_visible(&terminal, "hello"));
    assert!(contains_visible(&terminal, "ok"));
    match terminal.take_host_presentation_event() {
        Some(HostPresentationEvent::IconAndWindowTitle(payload)) => {
            assert_eq!(payload.as_bytes(), b"user@host:~");
        }
        other => panic!("expected title, got {other:?}"),
    }
    match terminal.take_host_presentation_event() {
        Some(HostPresentationEvent::WorkingDirectory(payload)) => {
            assert_eq!(payload.as_bytes(), b"file:///Users/dev");
        }
        other => panic!("expected cwd, got {other:?}"),
    }
    feed(&mut terminal, b"\x1b[2J\x1b[Hcleared\r\n");
    assert!(contains_visible(&terminal, "cleared"));
    assert!(!terminal.modes().alternate_screen);
}

#[test]
fn ssh_and_nested_ssh_keep_a_single_terminal_state() {
    let mut terminal = TerminalState::new(40, 4).unwrap();
    // Stand-in, not a live `ssh(1)` child: local DA plus stacked OSC titles on
    // one TerminalState. Interactive remote/nested SSH remains headed/manual.
    feed(&mut terminal, b"\x1b[c");
    assert_eq!(
        terminal.take_protocol_reply().unwrap().as_bytes(),
        b"\x1b[?1;2c"
    );
    feed(&mut terminal, b"\x1b]0;user@jump:~\x07ssh-1\r\n");
    feed(&mut terminal, b"\x1b[c\x1b]0;user@inner:/var\x07ssh-2\r\n");
    assert_eq!(
        terminal.take_protocol_reply().unwrap().as_bytes(),
        b"\x1b[?1;2c"
    );
    match terminal.take_host_presentation_event() {
        Some(HostPresentationEvent::IconAndWindowTitle(payload)) => {
            assert_eq!(payload.as_bytes(), b"user@jump:~");
        }
        other => panic!("expected jump title, got {other:?}"),
    }
    match terminal.take_host_presentation_event() {
        Some(HostPresentationEvent::IconAndWindowTitle(payload)) => {
            assert_eq!(payload.as_bytes(), b"user@inner:/var");
        }
        other => panic!("expected nested title, got {other:?}"),
    }
    assert!(contains_visible(&terminal, "ssh-1"));
    assert!(contains_visible(&terminal, "ssh-2"));
    assert!(!terminal.modes().alternate_screen);
}

#[test]
fn vim_neovim_alternate_screen_unicode_mouse_and_keys_restore_primary() {
    let mut terminal = TerminalState::new(20, 4).unwrap();
    feed(&mut terminal, "primary 日本語\r\n".as_bytes());
    feed(
        &mut terminal,
        b"\x1b[?1049h\x1b[?1h\x1b[?25h\x1b[?1000;1006h\x1b[H\x1b[2J",
    );
    assert!(terminal.modes().alternate_screen);
    assert!(terminal.modes().application_cursor);
    assert_eq!(terminal.modes().mouse_reporting, MouseReporting::Button);
    feed(&mut terminal, "VIM 👩‍💻\x1b[1;1H".as_bytes());
    assert!(contains_visible(&terminal, "VIM"));
    feed(&mut terminal, b"\x1b[?1000;1006l\x1b[?1l\x1b[?1049l");
    assert!(!terminal.modes().alternate_screen);
    assert!(!terminal.modes().application_cursor);
    assert_eq!(terminal.modes().mouse_reporting, MouseReporting::Off);
    assert!(contains_visible(&terminal, "primary"));
    let mut restored = String::new();
    for row in 0..terminal.rows() {
        for col in 0..terminal.cols() {
            if let Some(cell) = terminal.cell(col, row)
                && cell.role != CellRole::Continuation
            {
                restored.push(cell.character);
            }
        }
    }
    assert!(
        restored.contains("日本語") || contains_visible(&terminal, "日本語"),
        "primary CJK must remain on the restored live grid after vim-like alternate-screen; visible={restored:?} grid={:?}",
        visible(&terminal)
    );
    assert!(!contains_visible(&terminal, "VIM"));
}

#[test]
fn tmux_as_child_is_vt_bytes_not_seyal_panes() {
    let mut terminal = TerminalState::new(24, 6).unwrap();
    // tmux client sequences (status, nested 1049, copy-mode styling) stay
    // inside this TerminalState. There is no pane/window registry here.
    feed(&mut terminal, b"before-tmux\r\n");
    feed(
        &mut terminal,
        b"\x1b[?1049h\x1b[?1h\x1b[?1000;1006h\x1b[H\x1b[2J",
    );
    // tmux client status/window labels are VT bytes on this one screen. Seyal
    // has no pane registry to populate from that hierarchy.
    feed(&mut terminal, b"[0] 0:bash* 1:vim-\r\ninner-pane\r\n");
    assert!(terminal.modes().alternate_screen);
    feed(&mut terminal, b"\x1b[?1000;1006l\x1b[?1l\x1b[?1049l");
    assert!(!terminal.modes().alternate_screen);
    assert!(contains_visible(&terminal, "before-tmux"));
    assert!(!contains_visible(&terminal, "inner-pane"));
}

#[test]
fn htop_watch_ncurses_restore_primary_after_alt_screen() {
    let mut terminal = TerminalState::new(32, 5).unwrap();
    feed(&mut terminal, b"shell-prompt\r\n");
    feed(
        &mut terminal,
        b"\x1b[?1049h\x1b[?25l\x1b[?1000;1006h\x1b[H\x1b[2J",
    );
    feed(&mut terminal, b"\x1b[7m  PID USER\x1b[0m\r\nhtop-row\r\n");
    feed(&mut terminal, b"Every 2.0s: date\r\nwatch-row\r\n");
    assert!(terminal.modes().alternate_screen);
    assert!(!terminal.modes().cursor_visible);
    feed(&mut terminal, b"\x1b[?1000;1006l\x1b[?25h\x1b[?1049l");
    assert!(!terminal.modes().alternate_screen);
    assert!(terminal.modes().cursor_visible);
    assert!(contains_visible(&terminal, "shell-prompt"));
    assert!(!contains_visible(&terminal, "htop-row"));
}

#[test]
fn git_docker_kubectl_terraform_ansi_progress_and_long_output() {
    let mut terminal = TerminalState::new(48, 8).unwrap();
    feed(
        &mut terminal,
        b"\x1b[32mcommit\x1b[0m abc123 \x1b[33m(HEAD)\x1b[0m fix\r\n",
    );
    feed(&mut terminal, b"\x1b[34mdocker\x1b[0m build  12.4s\r\n");
    feed(&mut terminal, b"\x1b[36mNAME\x1b[0m  READY  STATUS\r\n");
    feed(&mut terminal, b"nginx  1/1    Running\r\n");
    feed(
        &mut terminal,
        b"\x1b[1mPlan:\x1b[0m 1 to add, 0 to change\r\n",
    );
    for i in 0..20 {
        feed(
            &mut terminal,
            format!("progress {i}/20 \x1b[K\r").as_bytes(),
        );
    }
    feed(&mut terminal, b"\nlong-cli-done\r\n");
    assert!(contains_visible(&terminal, "long-cli-done"));
    let styled = terminal.cell(0, 0).unwrap();
    assert_eq!(styled.style.fg, Color::Indexed(2));
}

#[test]
fn high_volume_logs_remain_bounded_and_searchable() {
    let mut terminal = TerminalState::new(24, 4).unwrap();
    for i in 0..400 {
        feed(
            &mut terminal,
            format!("log-{i:04} stream-line\r\n").as_bytes(),
        );
        if i == 80 {
            // Mid-flood type + resize must remain coherent on the live grid.
            feed(&mut terminal, b"typed-during-flood");
            terminal.resize(20, 6).unwrap();
            assert!(
                contains_visible(&terminal, "typed-during-flood"),
                "input must remain visible while high-volume output continues"
            );
            feed(&mut terminal, b"\r\n");
        }
        if i == 200 {
            feed(&mut terminal, b"\x1b[5~");
        }
    }
    assert!(terminal.primary_history_resident_bytes() <= HISTORY_PER_EXECUTION_BYTE_CAP);
    let matches = terminal.primary_history_search("log-0000", 8);
    assert!(
        !matches.is_empty(),
        "high-volume primary history must retain early log lines"
    );
    // Host-search covers retained history + active grid (composer Ctrl-R does not).
    let found = terminal
        .search_and_select("typed-during-flood", true)
        .expect("host search must find text typed during the flood");
    let copied = terminal
        .copy_selection_text()
        .expect("host copy must return the mid-flood typed needle");
    assert_eq!(
        copied, "typed-during-flood",
        "mid-flood typed input must remain searchable/copyable, anchors={found:?}"
    );
}

#[test]
fn cli_agent_tui_equivalent_negotiates_supported_subset_only() {
    let mut terminal = TerminalState::new(40, 8).unwrap();
    feed(&mut terminal, b"composer-ready\r\n");
    // Deterministic stand-in for Claude Code / Codex TUIs: alt-screen, kitty
    // flags 1|2, SGR mouse, title. Graphics/image protocols stay deferred.
    feed(
        &mut terminal,
        b"\x1b[?1049h\x1b[=3;1u\x1b[?1000;1006h\x1b]0;agent\x07\x1b[H\x1b[2J",
    );
    assert!(terminal.modes().alternate_screen);
    assert_eq!(terminal.modes().keyboard_flags, 3);
    assert_eq!(terminal.modes().mouse_reporting, MouseReporting::Button);
    feed(&mut terminal, b"agent-prompt>\r\n");
    assert!(contains_visible(&terminal, "agent-prompt>"));
    match terminal.take_host_presentation_event() {
        Some(HostPresentationEvent::IconAndWindowTitle(payload)) => {
            assert_eq!(payload.as_bytes(), b"agent");
        }
        other => panic!("expected agent title, got {other:?}"),
    }
    feed(&mut terminal, b"\x1b[?1000;1006l\x1b[<1u\x1b[?1049l");
    assert!(!terminal.modes().alternate_screen);
    assert!(contains_visible(&terminal, "composer-ready"));
}

#[test]
fn alt_screen_unicode_history_selection_mouse_and_keyboard_stay_coherent() {
    let mut terminal = TerminalState::new(16, 3).unwrap();
    feed(&mut terminal, "keep 日本語 👩‍💻 wrap-aaaaaaaa\r\n".as_bytes());
    feed(&mut terminal, b"second-line\r\n");
    // Keep the Unicode source in retained history across later widen/reflow so
    // this is a history-search fixture, not a live-grid-only observation.
    for i in 0..12 {
        feed(&mut terminal, format!("pad-{i:02}\r\n").as_bytes());
    }
    let history_before = terminal.primary_history_search("日本語", 4);
    assert_eq!(history_before.len(), 1);

    feed(&mut terminal, b"\x1b[=3;1u");
    assert_eq!(terminal.modes().keyboard_flags, 3);
    feed(
        &mut terminal,
        b"\x1b[?1049h\x1b[?1h\x1b[?1000;1006h\x1b[H\x1b[2Jtui\r\n",
    );
    assert!(terminal.modes().alternate_screen);
    assert_eq!(terminal.modes().keyboard_flags, 0);
    assert!(terminal.modes().application_cursor);

    let press = MouseReport {
        kind: MouseEventKind::Press,
        button: 0,
        shift: false,
        alt: false,
        control: false,
        col: 1,
        row: 0,
    };
    assert_eq!(
        encode_mouse_report(press, terminal.modes()).unwrap(),
        b"\x1b[<0;2;1M"
    );
    assert!(!mouse_takes_host_override(
        terminal.modes().mouse_reporting,
        false,
        None,
        MouseEventKind::Press
    ));
    assert!(mouse_takes_host_override(
        terminal.modes().mouse_reporting,
        true,
        None,
        MouseEventKind::Press
    ));

    terminal.set_linear_selection(VisualPos { col: 0, row: 0 }, VisualPos { col: 2, row: 0 });
    feed(&mut terminal, b"\x1b[?1000;1006l\x1b[?1l\x1b[?1049l");
    assert!(!terminal.modes().alternate_screen);
    assert_eq!(terminal.modes().keyboard_flags, 3);
    assert_eq!(terminal.modes().mouse_reporting, MouseReporting::Off);

    let matches = terminal.primary_history_search("日本語", 4);
    assert_eq!(matches.len(), 1);
    let copied = terminal
        .copy_history_range(matches[0].start, matches[0].end)
        .unwrap();
    assert!(copied.contains("日本語"));

    let after_leave = terminal.primary_history_search("日本語", 4);
    assert_eq!(
        after_leave.len(),
        1,
        "CJK search vanished on alternate-screen leave; units={:?}",
        terminal
            .primary_history_units_range(LineId(1), LineId(u64::MAX), 64)
            .iter()
            .map(|u| u.text.clone())
            .collect::<Vec<_>>()
    );
    terminal.resize(10, 4).unwrap();
    terminal.resize(20, 4).unwrap();
    let after_reflow = terminal.primary_history_search("日本語", 4);
    assert_eq!(
        after_reflow.len(),
        1,
        "CJK search vanished after reflow; units={:?}",
        terminal
            .primary_history_units_range(LineId(1), LineId(u64::MAX), 64)
            .iter()
            .map(|u| u.text.clone())
            .collect::<Vec<_>>()
    );
    assert!(
        visible(&terminal).contains("keep")
            || !terminal.primary_history_search("keep", 4).is_empty()
    );
}

#[test]
fn hostile_osc_query_paste_and_mouse_stay_bounded_and_non_executing() {
    let mut terminal = TerminalState::new(20, 3).unwrap();
    let deferred_before = terminal.diagnostics().deferred_sequences;

    feed(&mut terminal, b"\x1b]52;c;YWJj\x07SAFE\r\n");
    assert!(contains_visible(&terminal, "SAFE"));
    assert!(terminal.take_host_presentation_event().is_none());
    assert!(terminal.diagnostics().deferred_sequences > deferred_before);

    let huge = format!(
        "\x1b]2;{}\x07X",
        "A".repeat(MAX_PRESENTATION_PAYLOAD_BYTES + 64)
    );
    feed(&mut terminal, huge.as_bytes());
    match terminal.take_host_presentation_event() {
        Some(HostPresentationEvent::WindowTitle(payload)) => {
            assert_eq!(payload.as_bytes().len(), MAX_PRESENTATION_PAYLOAD_BYTES);
        }
        other => panic!("expected truncated title, got {other:?}"),
    }

    let truncated = format!("\x1b]2;{}\x07Y", "B".repeat(5000));
    feed(&mut terminal, truncated.as_bytes());
    assert!(terminal.take_host_presentation_event().is_none());

    feed(
        &mut terminal,
        b"\x1b]8;id=x;javascript:alert(1)\x07link\x1b]8;;\x07",
    );
    match terminal.take_host_presentation_event() {
        Some(HostPresentationEvent::Hyperlink { uri, .. }) => {
            assert_eq!(uri.as_bytes(), b"javascript:alert(1)");
        }
        other => panic!("hyperlink must remain untrusted presentation, got {other:?}"),
    }

    feed(&mut terminal, b"\x1b[6n");
    assert!(terminal.take_protocol_reply().is_some());
    for _ in 0..(MAX_PROTOCOL_REPLIES + 4) {
        feed(&mut terminal, b"\x1b[6n");
    }
    let mut replies = 0;
    while terminal.take_protocol_reply().is_some() {
        replies += 1;
    }
    assert_eq!(replies, MAX_PROTOCOL_REPLIES);

    for _ in 0..(MAX_HOST_PRESENTATION_EVENTS + 2) {
        feed(&mut terminal, b"\x1b]2;t\x07");
    }
    let mut events = 0;
    while terminal.take_host_presentation_event().is_some() {
        events += 1;
    }
    assert_eq!(events, MAX_HOST_PRESENTATION_EVENTS);

    assert_eq!(
        encode_paste(&[0; MAX_PASTE_BYTES + 1], false),
        Err(PasteError::TooLarge)
    );
    let pasted = encode_paste(b"ok", true).unwrap();
    assert!(pasted.starts_with(b"\x1b[200~"));
    assert!(pasted.ends_with(b"\x1b[201~"));

    let off = TerminalState::new(8, 2).unwrap();
    let report = MouseReport {
        kind: MouseEventKind::Press,
        button: 0,
        shift: false,
        alt: false,
        control: false,
        col: 0,
        row: 0,
    };
    assert!(encode_mouse_report(report, off.modes()).is_none());
    assert!(contains_visible(&terminal, "SAFE"));
}

#[test]
fn retained_unicode_history_host_search_and_copy_after_resize() {
    // Production host-search/copy path used by Runtime `HostSearch` /
    // `search_and_select`. Composer Ctrl-R is command history, not this route.
    let mut terminal = TerminalState::new(20, 4).unwrap();
    feed(
        &mut terminal,
        "needle-日本語-👩‍💻-softwrap-aaaaaaaaaaaa\r\n".as_bytes(),
    );
    for i in 0..16 {
        feed(&mut terminal, format!("pad-{i:02}\r\n").as_bytes());
    }
    terminal.resize(12, 4).unwrap();
    terminal.resize(28, 6).unwrap();

    let found = terminal
        .search_and_select("日本語", true)
        .expect("host search must find retained CJK after resize");
    let copied = terminal
        .copy_selection_text()
        .expect("host copy must use the selected history anchors");
    assert_eq!(
        copied, "日本語",
        "host-search copy must return the selected CJK needle, anchors={found:?}"
    );

    let emoji = terminal
        .search_and_select("👩‍💻", true)
        .expect("host search must find retained ZWJ emoji after resize");
    let copied_emoji = terminal
        .copy_selection_text()
        .expect("host copy must keep the ZWJ emoji grapheme");
    assert_eq!(
        copied_emoji, "👩‍💻",
        "host-search copy must return the selected ZWJ emoji, anchors={emoji:?}"
    );
}

#[test]
fn row_grow_resize_keeps_full_screen_history_seal_for_typed_line() {
    // Regression: growing rows must not leave a stale DECSTBM region that
    // discards scrolled primary rows instead of sealing them into HistoryStore.
    let mut terminal = TerminalState::new(24, 4).unwrap();
    feed(&mut terminal, b"pad-01\r\npad-02\r\n");
    feed(&mut terminal, b"typed-during-flood");
    terminal.resize(20, 6).unwrap();
    assert!(contains_visible(&terminal, "typed-during-flood"));
    feed(&mut terminal, b"\r\n");
    for _ in 0..20 {
        feed(&mut terminal, b"more\r\n");
    }
    assert!(
        !terminal
            .primary_history_search("typed-during-flood", 8)
            .is_empty(),
        "typed line must seal into primary history after row-grow resize"
    );
}

#[test]
fn explicit_partial_decstbm_still_skips_history_seal_after_row_grow() {
    // Inverse of the full-screen margin fix: an intentional partial DECSTBM must
    // remain a non-sealing region after row growth.
    let mut terminal = TerminalState::new(24, 6).unwrap();
    feed(&mut terminal, b"\x1b[2;4r"); // DECSTBM rows 2..4 (1-based)
    feed(&mut terminal, b"\x1b[2;1Hregion-needle\r\n");
    terminal.resize(24, 10).unwrap();
    for _ in 0..30 {
        feed(&mut terminal, b"flood\r\n");
    }
    assert!(
        terminal
            .primary_history_search("region-needle", 8)
            .is_empty(),
        "intentional partial DECSTBM must still skip HistoryStore seal after row growth"
    );
}
