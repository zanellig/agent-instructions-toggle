#!/usr/bin/env bash

set -euo pipefail

binary_source="${CARGO_TARGET_DIR:?}/release/agent-instructions"
while (($#)); do
    case "$1" in
        --binary)
            binary_source=$2
            shift 2
            ;;
        --no-refresh)
            shift
            ;;
        *)
            printf 'unexpected installer argument: %s\n' "$1" >&2
            exit 2
            ;;
    esac
done

if [[ "${AIT_LAB_IMPLEMENTATION:?}" == "codex-nc" ]]; then
    installed_binary="${HOME:?}/.local/bin/agent-instructions"
else
    installed_binary="${XDG_BIN_HOME:?}/agent-instructions"
fi
install -Dm755 -- "$binary_source" "$installed_binary"
printf '%s\n' "$AIT_LAB_IMPLEMENTATION" > "${HOME}/.ait-lab-implementation"

status_wrapper="${HOME}/.claude/fake-statusline.sh"
cat > "$status_wrapper" <<EOF
#!/usr/bin/env bash
set -euo pipefail
payload=\$(cat)
base=\$(printf '%s' "\$payload" | /lab/session/home/.claude/base-statusline.sh)
state=\$("$installed_binary" status --machine)
printf '%s AGENTS:%s\\n' "\$base" "\$state"
EOF
chmod 755 "$status_wrapper"
jq --arg command "$status_wrapper" \
    '.statusLine = {"type": "command", "command": $command}' \
    "${HOME}/.claude/settings.json" > "${HOME}/.claude/settings.json.tmp"
mv "${HOME}/.claude/settings.json.tmp" "${HOME}/.claude/settings.json"
