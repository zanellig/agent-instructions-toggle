#!/usr/bin/env bash
# Checks the tray against the live StatusNotifier host: registration, tooltip
# state, menu availability, and an update after a rename.
#
# Runs against a throwaway home, so it never touches your real documents.
# Usage: scripts/verify-tray.sh [path-to-binary]
set -uo pipefail

REPO=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
BIN=${1:-$REPO/target/release/agent-instructions}
[ -x "$BIN" ] || { echo "no binary at $BIN, run: cargo build --release" >&2; exit 1; }
command -v qdbus6 >/dev/null || { echo "qdbus6 is required" >&2; exit 1; }

FAKE=$(mktemp -d)
trap 'kill "${TRAY_PID:-0}" 2>/dev/null; rm -rf "$FAKE"' EXIT

for dir in .codex .codex_p .codex_p2; do
  mkdir -p "$FAKE/$dir"
  printf 'instructions\n' >"$FAKE/$dir/AGENTS.md"
done
mkdir -p "$FAKE/.claude" "$FAKE/run"
printf 'instructions\n' >"$FAKE/.claude/CLAUDE.md"

export HOME="$FAKE" XDG_RUNTIME_DIR="$FAKE/run"

failures=0
check() {
  local label=$1 expected=$2 actual=$3
  if [[ $actual == *"$expected"* ]]; then
    echo "  ok    $label"
  else
    echo "  FAIL  $label"
    echo "        wanted to find: $expected"
    echo "        got:            $actual"
    failures=$((failures + 1))
  fi
}

"$BIN" tray &
TRAY_PID=$!
sleep 2

SERVICE="org.kde.StatusNotifierItem-$TRAY_PID-1"

echo "==> registration"
registered=$(qdbus6 org.kde.StatusNotifierWatcher /StatusNotifierWatcher \
  org.kde.StatusNotifierWatcher.RegisteredStatusNotifierItems 2>&1)
check "the host lists this item" "$SERVICE" "$registered"

prop() {
  qdbus6 --literal "$SERVICE" /StatusNotifierItem \
    org.freedesktop.DBus.Properties.Get org.kde.StatusNotifierItem "$1" 2>&1
}

echo "==> tooltip"
check "reads AGENTS: on" "AGENTS: on" "$(prop ToolTip)"

echo "==> menu"
menu=$(prop Menu)
check "exposes a menu object path" "MenuBar" "$menu"
layout=$(qdbus6 --literal "$SERVICE" /MenuBar com.canonical.dbusmenu.GetLayout 0 -1 label 2>&1)
for item in Enable Disable Status Quit; do
  check "the menu offers $item" "$item" "$layout"
done

echo "==> update after a rename"
mv "$FAKE/.codex/AGENTS.md" "$FAKE/.codex/AGENTS.md.no-auto-inject"
mv "$FAKE/.codex_p/AGENTS.md" "$FAKE/.codex_p/AGENTS.md.no-auto-inject"
mv "$FAKE/.codex_p2/AGENTS.md" "$FAKE/.codex_p2/AGENTS.md.no-auto-inject"
mv "$FAKE/.claude/CLAUDE.md" "$FAKE/.claude/CLAUDE.md.no-auto-inject"
sleep 2
check "the tooltip follows the disk" "AGENTS: off" "$(prop ToolTip)"

echo "==> a missing target is named"
rm "$FAKE/.codex_p2/AGENTS.md.no-auto-inject"
sleep 2
check "names the missing document" "~/.codex_p2/AGENTS.md" "$(prop ToolTip)"

echo
if [ "$failures" -gt 0 ]; then
  echo "$failures check(s) failed"
  exit 1
fi
echo "All tray checks passed."
