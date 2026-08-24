#!/usr/bin/env bash
# Install, verify, or remove the agent-instructions desktop integration.
#
# Every destination follows the XDG variables, so the whole script can be run
# against an isolated HOME before it is run against a real one.
set -euo pipefail

REPO=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)

BIN_DIR="${XDG_BIN_HOME:-$HOME/.local/bin}"
DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}"
APP_DIR="$DATA_DIR/applications"
SHARE_DIR="$DATA_DIR/agent-instructions"
AUTOSTART_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/autostart"
CLAUDE_DIR="${CLAUDE_CONFIG_DIR:-$HOME/.claude}"

BIN_PATH="$BIN_DIR/agent-instructions"
TOGGLE_ENTRY="$APP_DIR/agent-instructions-toggle.desktop"
TRAY_ENTRY="$AUTOSTART_DIR/agent-instructions-tray.desktop"
STATUSLINE="$SHARE_DIR/statusline.sh"
PREVIOUS_RECORD="$SHARE_DIR/claude-statusline.previous"
SETTINGS="$CLAUDE_DIR/settings.json"

# Qt keycode for Meta+Ctrl+Shift+A: 0x10000000 + 0x04000000 + 0x02000000 + 'A'.
EXPECTED_KEYCODE=369098817

say() { printf '%s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

# Placeholder substitution without sed escaping problems.
render() {
  local src=$1 dest=$2 inner=${3-} text
  text=$(<"$src")
  text=${text//@BIN@/$BIN_PATH}
  text=${text//@INNER@/$inner}
  mkdir -p "$(dirname -- "$dest")"
  printf '%s\n' "$text" >"$dest"
}

install_all() {
  say "==> building"
  cargo build --release --manifest-path "$REPO/Cargo.toml"

  say "==> installing $BIN_PATH"
  install -Dm755 "$REPO/target/release/agent-instructions" "$BIN_PATH"

  say "==> installing desktop entries"
  render "$REPO/desktop/agent-instructions-toggle.desktop.in" "$TOGGLE_ENTRY"
  render "$REPO/desktop/agent-instructions-tray.desktop.in" "$TRAY_ENTRY"
  validate_entries

  refresh_desktop_metadata
  merge_claude_statusline

  say
  say "Installed. Verify with: $REPO/install.sh --verify"
  say "The shortcut is Meta+Ctrl+Shift+A. Start the tray now with:"
  say "  $BIN_PATH tray &"
  say "It starts by itself on your next login."
}

validate_entries() {
  if ! command -v desktop-file-validate >/dev/null; then
    say "    desktop-file-validate is missing, skipping validation"
    return
  fi
  desktop-file-validate "$TOGGLE_ENTRY" || die "$TOGGLE_ENTRY is not a valid desktop entry"
  desktop-file-validate "$TRAY_ENTRY" || die "$TRAY_ENTRY is not a valid desktop entry"
}

# KDE reads X-KDE-Shortcuts out of the service metadata. Refreshing that cache is
# supported; hand-editing kglobalshortcutsrc is not.
refresh_desktop_metadata() {
  say "==> refreshing desktop metadata"
  command -v update-desktop-database >/dev/null && update-desktop-database "$APP_DIR" || true
  command -v kbuildsycoca6 >/dev/null && kbuildsycoca6 --noincremental >/dev/null 2>&1 || true
}

# Wrap whatever status-line command is configured today. Wrapping is the only
# merge that cannot drop a segment we do not understand.
merge_claude_statusline() {
  say "==> merging the Claude status line"
  mkdir -p "$SHARE_DIR" "$CLAUDE_DIR"
  [ -f "$SETTINGS" ] || printf '{}\n' >"$SETTINGS"

  local current previous
  current=$(jq -r '.statusLine.command // ""' "$SETTINGS")

  if [ -f "$PREVIOUS_RECORD" ] && [ "$current" = "bash $STATUSLINE" ]; then
    # Re-running the installer must not wrap the wrapper.
    previous=$(<"$PREVIOUS_RECORD")
  else
    previous=$current
    printf '%s' "$previous" >"$PREVIOUS_RECORD"
  fi

  render "$REPO/claude/statusline.sh.in" "$STATUSLINE" "${previous:-cat >/dev/null}"
  chmod 755 "$STATUSLINE"

  local tmp="$SETTINGS.agent-instructions.tmp"
  jq --arg cmd "bash $STATUSLINE" \
    '.statusLine = {"type": "command", "command": $cmd}' "$SETTINGS" >"$tmp"
  mv "$tmp" "$SETTINGS"
}

verify() {
  local failures=0
  check() {
    if "$@" >/dev/null 2>&1; then
      say "  ok    $*"
    else
      say "  FAIL  $*"
      failures=$((failures + 1))
    fi
  }

  say "==> installed files"
  check test -x "$BIN_PATH"
  check test -f "$TOGGLE_ENTRY"
  check test -f "$TRAY_ENTRY"
  check test -x "$STATUSLINE"

  say "==> desktop entries"
  if command -v desktop-file-validate >/dev/null; then
    check desktop-file-validate "$TOGGLE_ENTRY"
    check desktop-file-validate "$TRAY_ENTRY"
  fi

  say "==> the binary"
  check "$BIN_PATH" --version
  say "  state: $("$BIN_PATH" status --machine 2>&1)"

  verify_shortcut || failures=$((failures + 1))
  verify_statusline || failures=$((failures + 1))

  say
  if [ "$failures" -gt 0 ]; then
    die "$failures check(s) failed"
  fi
  say "All checks passed."
}

# Read the binding back out of the running KGlobalAccel, not out of a file.
verify_shortcut() {
  say "==> KGlobalAccel binding"
  if ! command -v qdbus6 >/dev/null; then
    say "  skip  qdbus6 is missing"
    return 0
  fi
  local path infos
  path=$(qdbus6 org.kde.kglobalaccel /kglobalaccel \
    org.kde.KGlobalAccel.getComponent agent-instructions-toggle.desktop 2>/dev/null) || {
    say "  FAIL  KGlobalAccel does not know agent-instructions-toggle.desktop yet"
    say "        Log out and back in, or run: kbuildsycoca6 --noincremental"
    return 1
  }
  infos=$(qdbus6 --literal org.kde.kglobalaccel "$path" \
    org.kde.kglobalaccel.Component.allShortcutInfos 2>/dev/null)
  if printf '%s' "$infos" | grep -q "$EXPECTED_KEYCODE"; then
    say "  ok    Meta+Ctrl+Shift+A is bound ($EXPECTED_KEYCODE)"
    return 0
  fi
  say "  FAIL  the component exists but Meta+Ctrl+Shift+A is not bound"
  return 1
}

# The point of the check is that nothing the user already had disappeared.
verify_statusline() {
  say "==> Claude status line"
  local input merged previous_out
  input='{"workspace":{"current_dir":"'"$REPO"'"},"cwd":"'"$REPO"'",
    "model":{"display_name":"Opus 5"},"context_window":{"used_percentage":12},
    "vim":{"mode":"NORMAL"}}'

  merged=$(printf '%s' "$input" | bash "$STATUSLINE") || {
    say "  FAIL  the merged status-line command failed"
    return 1
  }
  case "$merged" in
    *AGENTS:*) say "  ok    the AGENTS segment renders" ;;
    *) say "  FAIL  no AGENTS segment in: $merged"; return 1 ;;
  esac

  local previous
  previous=$(cat "$PREVIOUS_RECORD" 2>/dev/null || true)
  if [ -z "$previous" ]; then
    say "  skip  there was no status line before installation"
    return 0
  fi
  previous_out=$(printf '%s' "$input" | bash -c "$previous")
  if [ "${merged#"${previous_out%$'\n'}"}" != "$merged" ]; then
    say "  ok    every pre-existing segment is still there"
    return 0
  fi
  say "  FAIL  the pre-existing segments changed"
  say "        before: $previous_out"
  say "        after:  $merged"
  return 1
}

uninstall() {
  say "==> restoring the Claude status line"
  if [ -f "$SETTINGS" ] && [ -f "$PREVIOUS_RECORD" ]; then
    local previous tmp
    previous=$(<"$PREVIOUS_RECORD")
    tmp="$SETTINGS.agent-instructions.tmp"
    if [ -n "$previous" ]; then
      jq --arg cmd "$previous" \
        '.statusLine = {"type": "command", "command": $cmd}' "$SETTINGS" >"$tmp"
    else
      jq 'del(.statusLine)' "$SETTINGS" >"$tmp"
    fi
    mv "$tmp" "$SETTINGS"
  fi

  say "==> removing files owned by this project"
  rm -f "$BIN_PATH" "$TOGGLE_ENTRY" "$TRAY_ENTRY" "$STATUSLINE" "$PREVIOUS_RECORD"
  rmdir "$SHARE_DIR" 2>/dev/null || true
  refresh_desktop_metadata

  say
  say "Removed. Your instruction documents were not touched; if any of them are"
  say "still disabled, restore them with: agent-instructions enable"
}

case "${1-}" in
  "") install_all ;;
  --verify) verify ;;
  --uninstall) uninstall ;;
  *) die "usage: install.sh [--verify | --uninstall]" ;;
esac
