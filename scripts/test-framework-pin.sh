#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp="$(mktemp -d)"
tmp="$(cd "$tmp" && pwd -P)"
trap 'rm -rf "$tmp"' EXIT
remote="$tmp/remote"
project="$tmp/project"
mkdir -p "$remote/tools" "$project/scripts"
git -C "$remote" init -q
git -C "$remote" config user.name 'Fixture'
git -C "$remote" config user.email 'fixture@example.invalid'
git -C "$remote" config commit.gpgsign false
mkdir -p "$tmp/empty-hooks"
git -C "$remote" config core.hooksPath "$tmp/empty-hooks"
for skill in project-context create-issue implement-issue verification pr-review process-review pr-merge; do
  mkdir -p "$remote/skills/$skill"
  printf 'old\n' > "$remote/skills/$skill/SKILL.md"
done
printf 'import sys\nassert sys.argv[1] == "--root" and sys.argv[-1] == "validate"\n' > "$remote/tools/project_context.py"
git -C "$remote" add .
git -C "$remote" commit -qm old
old="$(git -C "$remote" rev-parse HEAD)"
printf 'new\n' > "$remote/skills/implement-issue/SKILL.md"
git -C "$remote" commit -qam new
new="$(git -C "$remote" rev-parse HEAD)"
# Exercise the real bootstrap logic using a disposable local origin and known pin.
sed -e "s|^AI_SDLC_REPO=.*|AI_SDLC_REPO=\"file://$remote\"|" \
    -e "s|^AI_SDLC_COMMIT=.*|AI_SDLC_COMMIT=\"$new\"|" \
    "$ROOT/scripts/bootstrap-dev.sh" > "$project/scripts/bootstrap-dev.sh"
bootstrap="$project/scripts/bootstrap-dev.sh"
framework="$project/.sdlc/framework"
expect_fail() {
  if bash "$bootstrap" "$1"; then
    echo "unexpected success: $1" >&2
    exit 1
  fi
}
expect_fail --check-framework
bash "$bootstrap" --framework-only
bash "$bootstrap" --check-framework
[[ "$(git -C "$framework" rev-parse HEAD)" == "$new" ]]
git -C "$framework" fetch -q origin "$old"
git -C "$framework" checkout -q --detach "$old"
expect_fail --check-framework
bash "$bootstrap" --framework-only
bash "$bootstrap" --check-framework
[[ "$(git -C "$framework" rev-parse HEAD)" == "$new" ]]
# Fresh check and repeat refresh are non-mutating on a valid checkout.
bash "$bootstrap" --framework-only
printf 'local work\n' > "$framework/skills/implement-issue/SKILL.md"
expect_fail --check-framework
expect_fail --framework-only
grep -qx 'local work' "$framework/skills/implement-issue/SKILL.md"
git -C "$framework" checkout -- skills/implement-issue/SKILL.md
git -C "$framework" remote set-url origin "file://$tmp/wrong"
expect_fail --check-framework
expect_fail --framework-only
[[ "$(git -C "$framework" remote get-url origin)" == "file://$tmp/wrong" ]]
git -C "$framework" remote set-url origin "file://$remote"
bash "$bootstrap" --check-framework
# An unexpected directory is never deleted to make bootstrap pass.
mv "$framework" "$tmp/saved-framework"
mkdir "$framework"
printf 'preserve\n' > "$framework/sentinel"
expect_fail --framework-only
grep -qx preserve "$framework/sentinel"
printf '[seyal framework pin] missing, clean, stale, dirty, wrong-origin and unexpected-directory cases passed\n'
