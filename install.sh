#!/usr/bin/env bash

set -euo pipefail

readonly app_id="io.github.zanellig.agent-instructions"
readonly desktop_filename="${app_id}.desktop"
readonly status_wrapper_name="agent-instructions-statusline.sh"
readonly status_metadata_name=".agent-instructions-statusline.json"
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
readonly autostart_entry="${data_directory}/autostart/${desktop_filename}"

declare -a temporary_files=()

cleanup_temporary_files() {
    if ((${#temporary_files[@]})); then
        rm -f -- "${temporary_files[@]}"
    fi
}

trap cleanup_temporary_files EXIT

create_temporary_file() {
    local output_variable=$1
    local temporary_file
    temporary_file=$(mktemp)
    temporary_files+=("$temporary_file")
    printf -v "$output_variable" '%s' "$temporary_file"
}

create_sibling_temporary_file() {
    local destination=$1
    local output_variable=$2
    local temporary_file
    temporary_file=$(mktemp -- "${destination}.tmp.XXXXXXXXXX")
    temporary_files+=("$temporary_file")
    printf -v "$output_variable" '%s' "$temporary_file"
}

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

require_jq() {
    if ! command -v jq >/dev/null 2>&1; then
        printf 'error: jq is required to update Claude Code settings safely\n' >&2
        return 1
    fi
}

list_claude_profiles() {
    local discovery_binary=$1
    local output_file=$2
    "$discovery_binary" profiles --claude --null > "$output_file"
}

status_wrapper_command() {
    local wrapper=$1
    printf '%q' "$wrapper"
}

declare -a parsed_status_argv=()

parse_status_command() {
    local status_command=$1
    local profile=$2
    local normalized first resolved word

    parsed_status_argv=()
    if [[ -z "$status_command" || "$status_command" == *$'\n'* || "$status_command" == *$'\r'* ]]; then
        printf 'error: cannot safely extend Claude status command in %s: command is empty or contains a line break\n' "$profile" >&2
        return 1
    fi
    read -r -a parsed_status_argv <<< "$status_command"
    normalized=${parsed_status_argv[*]}
    if [[ "$normalized" != "$status_command" ]]; then
        printf 'error: cannot safely extend Claude status command in %s: quoting or non-canonical whitespace is unsupported\n' "$profile" >&2
        return 1
    fi
    for word in "${parsed_status_argv[@]}"; do
        if [[ ! "$word" =~ ^[[:alnum:]_./:@%+=,~-]+$ ]]; then
            printf 'error: cannot safely extend Claude status command in %s: shell operators and expansion are unsupported\n' "$profile" >&2
            return 1
        fi
    done

    first=${parsed_status_argv[0]}
    if [[ "$first" == "~/"* ]]; then
        resolved="${HOME}/${first#~/}"
    elif [[ "$first" == /* ]]; then
        resolved=$first
    elif [[ "$first" != */* ]]; then
        resolved=$(command -v -- "$first" 2>/dev/null || true)
    else
        resolved=""
    fi
    if [[ "$resolved" != /* || ! -f "$resolved" || ! -x "$resolved" ]]; then
        printf 'error: cannot safely extend Claude status command in %s: executable is not an absolute executable file\n' "$profile" >&2
        return 1
    fi
    parsed_status_argv[0]=$resolved
}

validate_settings_file() {
    local settings=$1
    if [[ ! -e "$settings" ]]; then
        return 0
    fi
    if [[ ! -f "$settings" || -L "$settings" ]]; then
        printf 'error: Claude settings must be a regular file: %s\n' "$settings" >&2
        return 1
    fi
    if ! jq -e 'type == "object"' "$settings" >/dev/null; then
        printf 'error: Claude settings are not a valid JSON object: %s\n' "$settings" >&2
        return 1
    fi
}

validate_status_metadata() {
    local metadata=$1
    local profile=$2
    if [[ ! -f "$metadata" || -L "$metadata" ]] || ! jq -e '
        .version == 1 and
        (.hadSettingsFile | type == "boolean") and
        (.hadStatusLine | type == "boolean") and
        has("previousStatusLine") and
        (if .hadStatusLine then
            (.previousStatusLine | type == "object") and
            .previousStatusLine.type == "command" and
            (.previousStatusLine.command | type == "string")
         else
            .previousStatusLine == null
         end)
    ' "$metadata" >/dev/null; then
        printf 'error: invalid Claude status integration metadata in %s\n' "$profile" >&2
        return 1
    fi
}

validate_claude_profile() {
    local profile=$1
    local settings="${profile}/settings.json"
    local wrapper="${profile}/${status_wrapper_name}"
    local metadata="${profile}/${status_metadata_name}"
    local expected_command current_command had_status previous_command

    validate_settings_file "$settings"
    expected_command=$(status_wrapper_command "$wrapper")
    current_command=$(jq -r 'if .statusLine.command? | type == "string" then .statusLine.command else "" end' "$settings" 2>/dev/null || printf '')

    if [[ -e "$metadata" || -e "$wrapper" ]]; then
        if [[ ! -f "$metadata" || -L "$metadata" || ! -f "$wrapper" || -L "$wrapper" ]]; then
            printf 'error: incomplete Claude status integration in %s; refusing to replace existing files\n' "$profile" >&2
            return 1
        fi
        validate_status_metadata "$metadata" "$profile"
        if [[ "$current_command" != "$expected_command" ]]; then
            printf 'error: Claude status command changed after installation in %s; refusing to overwrite it\n' "$profile" >&2
            return 1
        fi
        had_status=$(jq -r '.hadStatusLine' "$metadata")
        if [[ "$had_status" == "true" ]]; then
            previous_command=$(jq -r '.previousStatusLine.command // ""' "$metadata")
            parse_status_command "$previous_command" "$profile"
        fi
        return 0
    fi

    if [[ -e "$settings" ]] && jq -e 'has("statusLine")' "$settings" >/dev/null; then
        if ! jq -e '.statusLine | type == "object" and .type == "command" and (.command | type == "string")' "$settings" >/dev/null; then
            printf 'error: cannot safely extend non-command Claude statusLine in %s\n' "$profile" >&2
            return 1
        fi
        parse_status_command "$(jq -r '.statusLine.command' "$settings")" "$profile"
    fi
}

write_status_wrapper() {
    local profile=$1
    local wrapper="${profile}/${status_wrapper_name}"
    local metadata="${profile}/${status_metadata_name}"
    local wrapper_temp
    local had_status previous_command

    had_status=$(jq -r '.hadStatusLine' "$metadata")
    parsed_status_argv=()
    if [[ "$had_status" == "true" ]]; then
        previous_command=$(jq -r '.previousStatusLine.command' "$metadata")
        parse_status_command "$previous_command" "$profile"
    fi

    create_sibling_temporary_file "$wrapper" wrapper_temp

    {
        printf '%s\n' '#!/usr/bin/env bash' 'set -uo pipefail'
        printf 'readonly agent_instructions_binary=%q\n' "$binary_path"
        printf 'base_command=('
        if ((${#parsed_status_argv[@]})); then
            printf ' %q' "${parsed_status_argv[@]}"
        fi
        printf ' )\n'
        printf '%s\n' \
            'base_output=""' \
            'if ((${#base_command[@]})); then' \
            '    base_output=$("${base_command[@]}") || true' \
            'else' \
            '    cat >/dev/null' \
            'fi' \
            'warning_file=$(mktemp)' \
            'trap '\''rm -f -- "$warning_file"'\'' EXIT' \
            'if ! segment=$("$agent_instructions_binary" status --segment 2>"$warning_file"); then' \
            '    segment=AGENTS:conflict' \
            '    printf '\''status unavailable\n'\'' > "$warning_file"' \
            'fi' \
            'details=$(<"$warning_file")' \
            'details=${details#Warning: }' \
            'details=${details//$'\''\n'\''/; }' \
            'if [[ -n "$base_output" ]]; then' \
            '    printf '\''%s | '\'' "$base_output"' \
            'fi' \
            'printf '\''%s'\'' "$segment"' \
            'if [[ -n "$details" ]]; then' \
            '    printf '\'' \033[33m[%s]\033[0m'\'' "$details"' \
            'fi' \
            'printf '\''\n'\'''
    } > "$wrapper_temp"
    chmod 755 "$wrapper_temp"
    mv -f -- "$wrapper_temp" "$wrapper"
}

install_claude_profile() {
    local profile=$1
    local settings="${profile}/settings.json"
    local wrapper="${profile}/${status_wrapper_name}"
    local metadata="${profile}/${status_metadata_name}"
    local settings_temp metadata_temp
    local wrapper_command had_settings had_status previous_status

    wrapper_command=$(status_wrapper_command "$wrapper")
    if [[ -e "$metadata" ]]; then
        write_status_wrapper "$profile"
    else
        had_settings=false
        [[ -e "$settings" ]] && had_settings=true
        had_status=false
        previous_status=null
        if [[ "$had_settings" == "true" ]] && jq -e 'has("statusLine")' "$settings" >/dev/null; then
            had_status=true
            previous_status=$(jq -c '.statusLine' "$settings")
        fi
        create_sibling_temporary_file "$metadata" metadata_temp
        jq -n \
            --argjson hadSettingsFile "$had_settings" \
            --argjson hadStatusLine "$had_status" \
            --argjson previousStatusLine "$previous_status" \
            '{version: 1, hadSettingsFile: $hadSettingsFile, hadStatusLine: $hadStatusLine, previousStatusLine: $previousStatusLine}' \
            > "$metadata_temp"
        mv -- "$metadata_temp" "$metadata"
        write_status_wrapper "$profile"
    fi

    create_sibling_temporary_file "$settings" settings_temp
    if [[ -e "$settings" ]]; then
        jq --arg command "$wrapper_command" \
            'if has("statusLine") then .statusLine.type = "command" | .statusLine.command = $command else .statusLine = {type: "command", command: $command} end' \
            "$settings" > "$settings_temp"
    else
        jq -n --arg command "$wrapper_command" \
            '{statusLine: {type: "command", command: $command}}' > "$settings_temp"
    fi
    mv -f -- "$settings_temp" "$settings"
}

remove_claude_profile() {
    local profile=$1
    local settings="${profile}/settings.json"
    local wrapper="${profile}/${status_wrapper_name}"
    local metadata="${profile}/${status_metadata_name}"
    local expected_command current_command had_status had_settings settings_temp

    if [[ ! -e "$metadata" && ! -e "$wrapper" ]]; then
        return 0
    fi
    require_jq
    validate_status_metadata "$metadata" "$profile"
    validate_settings_file "$settings"
    expected_command=$(status_wrapper_command "$wrapper")
    if [[ -e "$settings" ]]; then
        current_command=$(jq -r '.statusLine.command // ""' "$settings")
    else
        current_command=""
    fi
    if [[ "$current_command" == "$expected_command" ]]; then
        create_sibling_temporary_file "$settings" settings_temp
        had_status=$(jq -r '.hadStatusLine' "$metadata")
        had_settings=$(jq -r '.hadSettingsFile' "$metadata")
        if [[ "$had_status" == "true" ]]; then
            jq --slurpfile metadata "$metadata" '
                .statusLine.type = $metadata[0].previousStatusLine.type |
                .statusLine.command = $metadata[0].previousStatusLine.command
            ' "$settings" > "$settings_temp"
        else
            jq '
                del(.statusLine.type, .statusLine.command) |
                if .statusLine == {} then del(.statusLine) else . end
            ' "$settings" > "$settings_temp"
        fi
        if [[ "$had_settings" == "false" ]] && jq -e 'length == 0' "$settings_temp" >/dev/null; then
            rm -f -- "$settings" "$settings_temp"
        else
            mv -f -- "$settings_temp" "$settings"
        fi
    else
        printf 'Claude status command in %s is no longer project-owned; leaving settings unchanged.\n' "$profile" >&2
    fi
    rm -f -- "$wrapper" "$metadata"
}

cargo_command=${CARGO:-cargo}
target_directory=${CARGO_TARGET_DIR:-"${project_directory}/target"}
if [[ "$target_directory" != /* ]]; then
    target_directory="${project_directory}/${target_directory}"
fi
readonly built_binary="${target_directory}/release/agent-instructions"

if [[ "${1:-}" == "--uninstall" ]]; then
    if [[ $# -ne 1 ]]; then
        printf 'usage: %s [--uninstall]\n' "$0" >&2
        exit 1
    fi
    discovery_binary=$binary_path
    if [[ ! -x "$discovery_binary" ]]; then
        discovery_binary=$built_binary
    fi
    if [[ ! -x "$discovery_binary" ]]; then
        printf 'error: installed or built agent-instructions binary is required for safe Claude profile discovery\n' >&2
        exit 1
    fi
    create_temporary_file profiles_file
    list_claude_profiles "$discovery_binary" "$profiles_file"
    mapfile -d '' -t claude_profiles < "$profiles_file"
    for profile in "${claude_profiles[@]}"; do
        remove_claude_profile "$profile"
    done
    rm -f -- "$binary_path" "$application_entry" "$kglobalaccel_entry" "$autostart_entry"
    refresh_desktop_metadata
    printf 'Removed agent-instructions desktop and Claude Code integration.\n'
    exit 0
elif [[ $# -ne 0 ]]; then
    printf 'usage: %s [--uninstall]\n' "$0" >&2
    exit 1
fi

cargo_command=${CARGO:-cargo}
(
    cd -- "$project_directory"
    "$cargo_command" build --release --locked
)

if [[ ! -f "$built_binary" ]]; then
    printf 'error: release build did not produce %s\n' "$built_binary" >&2
    exit 1
fi

create_temporary_file profiles_file
create_temporary_file desktop_entry
create_temporary_file autostart_desktop_entry
list_claude_profiles "$built_binary" "$profiles_file"
mapfile -d '' -t claude_profiles < "$profiles_file"
if ((${#claude_profiles[@]})); then
    require_jq
fi
for profile in "${claude_profiles[@]}"; do
    validate_claude_profile "$profile"
done

install -Dm755 -- "$built_binary" "$binary_path"

escaped_binary=${binary_path//\\/\\\\}
escaped_binary=${escaped_binary//\"/\\\"}
escaped_binary=${escaped_binary//\$/\\\$}
escaped_binary=${escaped_binary//\`/\\\`}

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

printf '%s\n' \
    '[Desktop Entry]' \
    'Type=Application' \
    'Name=Agent Instructions Tray' \
    'Comment=Show global instruction document state' \
    "Exec=\"${escaped_binary}\" tray" \
    'Icon=preferences-system' \
    'Terminal=false' \
    'NoDisplay=true' \
    'X-GNOME-Autostart-enabled=true' \
    > "$autostart_desktop_entry"

install -Dm644 -- "$desktop_entry" "$application_entry"
install -Dm644 -- "$desktop_entry" "$kglobalaccel_entry"
install -Dm644 -- "$autostart_desktop_entry" "$autostart_entry"
for profile in "${claude_profiles[@]}"; do
    install_claude_profile "$profile"
done
refresh_desktop_metadata

printf 'Installed %s\n' "$binary_path"
printf 'Registered Plasma shortcut Meta+Ctrl+Shift+A\n'
printf 'Installed tray autostart entry\n'
printf 'Integrated %d Claude Code profile(s)\n' "${#claude_profiles[@]}"
