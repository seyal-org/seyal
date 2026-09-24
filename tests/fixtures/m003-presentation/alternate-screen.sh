#!/usr/bin/env bash
# M003 presentation fixture: alternate-screen enter → bounded input wait → restore.
# Restores primary screen on normal exit and on INT/TERM/HUP via EXIT trap.
# External child-process workload only. Network-free. No home/history/credential access.
set -euo pipefail

INPUT_TIMEOUT_SECONDS="${SEYAL_M003_ALTSCREEN_TIMEOUT:-5}"

restore_primary() {
  # DECSET 1049 leave (restore primary screen / buffer).
  printf '\033[?1049l'
  printf 'seyal-m003-altscreen-restored\n'
}

trap restore_primary EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
trap 'exit 129' HUP

printf 'seyal-m003-altscreen-enter\n'
# DECSET 1049 enter alternate screen.
printf '\033[?1049h'
printf 'seyal-m003-altscreen-active\n'

# Bounded wait for one line of input; timeout is success for automated self-test.
set +e
read -r -t "${INPUT_TIMEOUT_SECONDS}" _answer
read_status=$?
set -e

if [[ "$read_status" -eq 0 ]]; then
  printf 'seyal-m003-altscreen-input-received\n'
else
  printf 'seyal-m003-altscreen-input-timeout\n'
fi

printf 'seyal-m003-altscreen-exit\n'
exit 0
