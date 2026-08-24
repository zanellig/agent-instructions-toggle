#!/usr/bin/env bash
set -euo pipefail

readonly app_id="io.github.zanellig.agent-instructions"
readonly desktop_filename="${app_id}.desktop"
project_directory=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
readonly project_directory

: "${HOME:?HOME must be set}"
bin_directory=${XDG_BIN_HOME:-"${HOME}/.local/bin"}
data_directory=${XDG_DATA_HOME:-"${HOME}/.local/share"}
readonly bin_directory data_directory

for directory in "$bin_directory" "$data_directory"; do
    if [[ "$directory" != /* ]]; then
        printf 'error: install destinations must be absolute paths: %s\n' "$directory" >&2
        exit 1
    fi
    if [[ "$directory" == *$'\n'* || "$directory" == *$'\r'* ]]; then
        printf 'error: install destinations cannot contain line breaks\n' >&2
        exit 1
    fi
done

readonly binary_path="${bin_directory}/agent-instructions"
readonly application_entry="${data_directory}/applications/${desktop_filename}"
readonly kglobalaccel_entry="${data_directory}/kglobalaccel/${desktop_filename}"

refresh_desktop_metadata() {
    local refresher
    if [[ -n "${KBUILDSYCOCA:-}" ]]; then
        refresher=$KBUILDSYCOCA
    elif command -v kbuildsycoca6 >/dev/null 2>&1; then
        refresher=kbuildsycoca6
    elif command -v kbuildsycoca5 >/dev/null 2>&1; then
        refresher=kbuildsycoca5
    else
        printf 'error: kbuildsycoca6 or kbuildsycoca5 is required to refresh Plasma metadata\n' >&2
        return 1
    fi
    "$refresher"
}

if [[ "${1:-}" == "--uninstall" ]]; then
    if [[ $# -ne 1 ]]; then
        printf 'usage: %s [--uninstall]\n' "$0" >&2
        exit 1
    fi
    rm -f -- "$binary_path" "$application_entry" "$kglobalaccel_entry"
    refresh_desktop_metadata
    printf 'Removed agent-instructions desktop integration.\n'
    exit 0
elif [[ $# -ne 0 ]]; then
    printf 'usage: %s [--uninstall]\n' "$0" >&2
    exit 1
fi

cargo_command=${CARGO:-cargo}
target_directory=${CARGO_TARGET_DIR:-"${project_directory}/target"}
if [[ "$target_directory" != /* ]]; then
    target_directory="${project_directory}/${target_directory}"
fi

(
    cd -- "$project_directory"
    "$cargo_command" build --release --locked
)

readonly built_binary="${target_directory}/release/agent-instructions"
if [[ ! -f "$built_binary" ]]; then
    printf 'error: release build did not produce %s\n' "$built_binary" >&2
    exit 1
fi

install -Dm755 -- "$built_binary" "$binary_path"

escaped_binary=${binary_path//\\/\\\\}
escaped_binary=${escaped_binary//\"/\\\"}
escaped_binary=${escaped_binary//\$/\\\$}
escaped_binary=${escaped_binary//\`/\\\`}

desktop_entry=$(mktemp)
trap 'rm -f -- "$desktop_entry"' EXIT
printf '%s\n' \
    '[Desktop Entry]' \
    'Type=Application' \
    'Name=Toggle Agent Instructions' \
    'Comment=Toggle global instruction documents for new coding-agent contexts' \
    "Exec=\"${escaped_binary}\" toggle --notify" \
    'Icon=preferences-system' \
    'Terminal=false' \
    'NoDisplay=true' \
    'Categories=Utility;' \
    'X-KDE-Shortcuts=Meta+Ctrl+Shift+A' \
    > "$desktop_entry"

install -Dm644 -- "$desktop_entry" "$application_entry"
install -Dm644 -- "$desktop_entry" "$kglobalaccel_entry"
refresh_desktop_metadata

printf 'Installed %s\n' "$binary_path"
printf 'Registered Plasma shortcut Meta+Ctrl+Shift+A\n'
