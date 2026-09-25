#!/usr/bin/env bash
# M003 presentation fixture: resize-observable full-screen text pattern for later TUI tests.
# Reports cols/rows, draws a bounded grid, then restores primary screen.
# External child-process workload only. Network-free. No home/history/credential access.
set -euo pipefail

# Hard safety caps so CI cannot emit unbounded output even on huge TTYs.
MAX_COLS=80
MAX_ROWS=24

restore_primary() {
  printf '\033[?1049l'
  printf 'seyal-m003-resize-restored\n'
}

trap restore_primary EXIT

cols=80
rows=24
if size="$(stty size 2>/dev/null)"; then
  # stty size → "<rows> <cols>"
  rows="${size%% *}"
  cols="${size##* }"
elif [[ -n "${LINES:-}" && -n "${COLUMNS:-}" ]]; then
  rows="$LINES"
  cols="$COLUMNS"
fi

# Sanitize and clamp.
if ! [[ "$rows" =~ ^[0-9]+$ ]] || [[ "$rows" -lt 1 ]]; then
  rows=24
fi
if ! [[ "$cols" =~ ^[0-9]+$ ]] || [[ "$cols" -lt 1 ]]; then
  cols=80
fi
if [[ "$rows" -gt "$MAX_ROWS" ]]; then
  rows="$MAX_ROWS"
fi
if [[ "$cols" -gt "$MAX_COLS" ]]; then
  cols="$MAX_COLS"
fi

printf 'seyal-m003-resize-begin cols=%d rows=%d\n' "$cols" "$rows"
printf '\033[?1049h'
printf 'seyal-m003-resize-active cols=%d rows=%d\n' "$cols" "$rows"

r=1
while [[ "$r" -le "$rows" ]]; do
  # Deterministic row body: marker + zero-padded index + fill to cols (clamped).
  prefix="$(printf 'R%02d-' "$r")"
  body="${prefix}"
  while [[ "${#body}" -lt "$cols" ]]; do
    body="${body}#"
  done
  body="${body:0:$cols}"
  printf 'seyal-m003-resize-row %s\n' "$body"
  r=$((r + 1))
done

printf 'seyal-m003-resize-done cols=%d rows=%d\n' "$cols" "$rows"
exit 0
