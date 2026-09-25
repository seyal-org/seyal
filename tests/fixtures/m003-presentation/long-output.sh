#!/usr/bin/env bash
# M003 presentation fixture: bounded deterministic long-output stream.
# Hard finite bound: exactly LONG_LINES lines of body plus one completion marker.
# External child-process workload only. Network-free. No home/history/credential access.
set -euo pipefail

# Hard finite bound — do not raise without updating manifest.toml and the self-test.
LONG_LINES=200

i=1
while [[ "$i" -le "$LONG_LINES" ]]; do
  printf 'seyal-m003-long line=%04d\n' "$i"
  i=$((i + 1))
done
printf 'seyal-m003-long-done lines=%d\n' "$LONG_LINES"
exit 0
