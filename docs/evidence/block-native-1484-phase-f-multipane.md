# Phase F — Multipane / apps / recovery (#1484)

- **Branch:** `mahboobmonnamd/issue/1484`

## Automated evidence

1. Nested interactive zsh bootstrap trusts session Blocks; host vs session native RegionIds stay distinct.
2. Composer SSH→bash trusts session Blocks; host/session RegionIds distinct; host active region restores after session Complete.
3. Unsupported remote shell: exit returns to local AtPrompt; new local Block opens a different output RegionId than the SSH host.
4. Remote `python3 -c` + interactive `python3 -i` session Blocks keep distinct RegionIds from the SSH host; host restores after REPL exit.
5. Local `nvim` alternate-screen: native regions open; composer Busy; after `:q!` primary restores; next local Block uses fresh RegionIds.
6. Local `tmux -V` Block opens distinct RegionIds from the following local Block (attach/detach remains headed).

## Still required (G7/G8)

- Headed: remote nvim/tmux attach, htop, supported CLI agents; combined multipane SSH+REPL+AI CLI with scroll/composer/clear/mouse/focus/resize/reconnect.
- Pane-local tab isolation and adoption/reconnect InputEpoch proofs on native RegionIds.
- Exact-head headed ledger — no GO from live PTY unit suites alone.
