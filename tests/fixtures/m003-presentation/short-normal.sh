#!/usr/bin/env bash
# M003 presentation fixture: short normal-screen stdout/stderr with completion marker.
# External child-process workload only. Network-free. No home/history/credential access.
set -euo pipefail

printf 'seyal-m003-short-stdout line=1\n'
printf 'seyal-m003-short-stderr line=1\n' >&2
printf 'seyal-m003-short-done\n'
exit 0
