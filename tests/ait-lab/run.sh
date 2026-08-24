#!/usr/bin/env bash

set -euo pipefail

readonly TEST_ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
readonly LAB="$TEST_ROOT/ait-lab"
readonly TEST_TEMP=$(mktemp -d)
readonly TEST_STATE="$TEST_TEMP/playground/agent-instructions-toggle-with-a-deliberately-long-checkout-name-for-unix-socket-regression/.ait-lab-state"
readonly TEST_RUNTIME="$TEST_TEMP/runtime"
readonly TEST_HOST_DATA="$TEST_TEMP/host-data"
readonly TEST_HOST_DESKTOP="$TEST_HOST_DATA/applications/ait-lab-agent-instructions-toggle.desktop"
readonly TEST_HOST_REFRESH_LOG="$TEST_TEMP/host-refresh.log"
readonly TEST_KGLOBALACCEL_LOG="$TEST_TEMP/kglobalaccel.log"
readonly TEST_KGLOBALACCEL_OWNER="$TEST_TEMP/kglobalaccel-owner"
readonly FAKE_CANDIDATE="$TEST_ROOT/tests/ait-lab/fake-candidate"
readonly FAKE_CLAUDE="$TEST_ROOT/tests/ait-lab/fake-claude"
readonly FAKE_KGLOBALACCEL="$TEST_ROOT/tests/ait-lab/fake-kglobalaccel"
readonly FAKE_HOST_REFRESH="$TEST_ROOT/tests/ait-lab/stubs/kbuildsycoca"
LAB_BUS_PID=""
LAB_BUS_ADDRESS=""

cleanup() {
    run_lab stop >/dev/null 2>&1 || true
    if [[ -n "$LAB_BUS_PID" ]]; then
        kill "$LAB_BUS_PID" >/dev/null 2>&1 || true
    fi
    rm -rf -- "$TEST_TEMP"
}
trap cleanup EXIT

fail() {
    printf 'FAIL: %s\n' "$*" >&2
    exit 1
}

assert_equal() {
    local expected=$1
    local actual=$2
    local message=$3

    if [[ "$actual" != "$expected" ]]; then
        printf 'expected: %q\n' "$expected" >&2
        printf 'actual:   %q\n' "$actual" >&2
        fail "$message"
    fi
}

assert_contains() {
    local expected=$1
    local actual=$2
    local message=$3

    if [[ "$actual" != *"$expected"* ]]; then
        printf 'missing: %q\n' "$expected" >&2
        printf 'actual:  %q\n' "$actual" >&2
        fail "$message"
    fi
}

session_process_exists() {
    local session=$1
    local command_file command_line

    for command_file in /proc/[0-9]*/cmdline; do
        [[ -r "$command_file" ]] || continue
        command_line=$(tr '\0' ' ' < "$command_file") || continue
        [[ "$command_line" == *"$session"* ]] && return 0
    done
    return 1
}

assert_session_stopped() {
    local session=$1

    for _ in {1..100}; do
        session_process_exists "$session" || return 0
        sleep 0.05
    done
    fail "stop left a process running for $session"
}

run_lab() {
    AIT_LAB_STATE_ROOT="$TEST_STATE" \
    AIT_LAB_SOCKET_ROOT="$TEST_RUNTIME/ait-lab" \
    AIT_LAB_SOURCE_CLAUDE="$FAKE_CANDIDATE" \
    AIT_LAB_SOURCE_CODEX_NC="$FAKE_CANDIDATE" \
    AIT_LAB_SOURCE_CODEX_FC="$FAKE_CANDIDATE" \
    AIT_LAB_CLAUDE_EXECUTABLE="$FAKE_CLAUDE" \
    AIT_LAB_SESSION_BUS_ADDRESS="$LAB_BUS_ADDRESS" \
    AIT_LAB_HOST_DATA_HOME="$TEST_HOST_DATA" \
    AIT_LAB_QDBUS="$FAKE_KGLOBALACCEL" \
    AIT_LAB_KBUILDSYCOCA="$FAKE_HOST_REFRESH" \
    AIT_LAB_STUB_LOG="$TEST_HOST_REFRESH_LOG" \
    AIT_LAB_FAKE_KGLOBALACCEL_LOG="$TEST_KGLOBALACCEL_LOG" \
    AIT_LAB_FAKE_KGLOBALACCEL_OWNER_FILE="$TEST_KGLOBALACCEL_OWNER" \
        "$LAB" "$@"
}

start_test_bus() {
    local -a details

    mapfile -t details < <(dbus-daemon --session --fork --print-address=1 --print-pid=1)
    LAB_BUS_ADDRESS=${details[0]}
    LAB_BUS_PID=${details[1]}
}

assert_socket_root_rejected() {
    local socket_directory=$1
    local expected_error=$2
    local lab=$3
    local state=$4
    local output status

    set +e
    output=$(AIT_LAB_STATE_ROOT="$state" \
        AIT_LAB_SOCKET_ROOT="$socket_directory" \
        AIT_LAB_CLAUDE_EXECUTABLE="$FAKE_CLAUDE" \
        AIT_LAB_SESSION_BUS_ADDRESS="$LAB_BUS_ADDRESS" \
        "$lab" use claude 2>&1)
    status=$?
    set -e
    assert_equal "1" "$status" "use should reject an unsafe socket directory"
    assert_contains "$expected_error" "$output" "use should explain why the socket directory is unsafe"
}

test_show_reports_no_active_session() {
    local output

    output=$(AIT_LAB_STATE_ROOT="$TEST_STATE" "$LAB" show)
    assert_equal "No active lab session." "$output" "show should explain that no implementation is active"
}

test_use_starts_an_isolated_candidate() {
    local output claude_session final_session local_lab local_project local_state probe_status
    local private_runtime public_runtime runtime_link

    mkdir -p "$TEST_RUNTIME"
    chmod 700 "$TEST_RUNTIME"
    start_test_bus
    local_project="$TEST_TEMP/local-project"
    local_lab="$local_project/ait-lab"
    local_state="$TEST_TEMP/local-state"
    mkdir -p "$local_project/implementations/claude" "$local_project/tests"
    cp "$LAB" "$local_lab"
    cp "$FAKE_CANDIDATE/Cargo.lock" "$FAKE_CANDIDATE/Cargo.toml" \
        "$FAKE_CANDIDATE/install.sh" "$local_project/implementations/claude/"
    cp -a "$FAKE_CANDIDATE/src" "$local_project/implementations/claude/src"
    ln -s "$TEST_ROOT/tests/ait-lab" "$local_project/tests/ait-lab"
    git -C "$local_project" init -q
    git -C "$local_project" add .
    git -C "$local_project" -c user.name=ait-lab -c user.email=ait-lab.invalid \
        -c commit.gpgsign=false commit -qm "test fixture"
    printf 'unrelated change\n' > "$local_project/unrelated.txt"

    assert_socket_root_rejected "relative-runtime/ait-lab" "socket directory must be an absolute path" "$local_lab" "$local_state"
    private_runtime="$TEST_TEMP/private-runtime"
    runtime_link="$private_runtime/ait-lab"
    mkdir "$private_runtime"
    chmod 700 "$private_runtime"
    mkdir "$TEST_TEMP/private-runtime-target"
    chmod 700 "$TEST_TEMP/private-runtime-target"
    ln -s "$TEST_TEMP/private-runtime-target" "$runtime_link"
    assert_socket_root_rejected "$runtime_link" "socket directory must not be a symbolic link" "$local_lab" "$local_state"
    public_runtime="$TEST_TEMP/public-runtime"
    mkdir "$public_runtime"
    chmod 755 "$public_runtime"
    assert_socket_root_rejected "$public_runtime/ait-lab" "socket parent directory must have mode 700" "$local_lab" "$local_state"

    output=$(AIT_LAB_STATE_ROOT="$local_state" \
        AIT_LAB_SOCKET_ROOT="$TEST_RUNTIME/ait-lab" \
        AIT_LAB_CLAUDE_EXECUTABLE="$FAKE_CLAUDE" \
        AIT_LAB_SESSION_BUS_ADDRESS="$LAB_BUS_ADDRESS" \
        "$local_lab" use claude)
    assert_contains "from implementations/claude" "$output" "use should select an in-repo implementation"
    output=$(AIT_LAB_STATE_ROOT="$local_state" AIT_LAB_SOCKET_ROOT="$TEST_RUNTIME/ait-lab" "$local_lab" show)
    AIT_LAB_STATE_ROOT="$local_state" AIT_LAB_SOCKET_ROOT="$TEST_RUNTIME/ait-lab" "$local_lab" stop >/dev/null
    assert_contains "Source: implementations/claude" "$output" "show should report the in-repo source"
    assert_contains "Source tree: clean" "$output" "show should scope dirtiness to the selected project"

    output=$(run_lab use claude)
    assert_contains "Using claude" "$output" "use should report the selected implementation"
    [[ ! -e "$TEST_HOST_DESKTOP" ]] || fail "default use registered a host shortcut"

    output=$(run_lab show)
    assert_contains "Implementation: claude" "$output" "show should name the active implementation"
    claude_session=$(awk -F ': ' '$1 == "Session" { print $2 }' <<< "$output")

    output=$(run_lab app status)
    assert_equal "on" "$output" "app should run the selected implementation"

    output=$(run_lab statusline normal)
    assert_contains "BASE AGENTS:on" "$output" "statusline should run the command installed into the fake Claude profile"

    run_lab app toggle --notify >/dev/null
    output=$(run_lab statusline normal)
    assert_contains "BASE AGENTS:off" "$output" "statusline should observe instruction-state changes"

    output=$(run_lab claude)
    assert_equal "/lab/session/home" "$output" "claude should run with the synthetic home"

    set +e
    output=$(run_lab app probe-write "$TEST_TEMP/outside-session" 2>&1)
    probe_status=$?
    set -e
    assert_equal "23" "$probe_status" "the candidate should observe a blocked host write"
    assert_contains "blocked:" "$output" "the candidate should report the sandbox denial"
    [[ ! -e "$TEST_TEMP/outside-session" ]] || fail "the sandbox allowed a write outside its session"

    set +e
    output=$(run_lab app probe-read "$TEST_ROOT/AGENTS.md" 2>&1)
    probe_status=$?
    set -e
    assert_equal "23" "$probe_status" "the candidate should not read files from the repository checkout"
    assert_contains "blocked:" "$output" "the candidate should observe that the checkout is absent"

    set +e
    output=$(run_lab app probe-write /lab/session/manifest.json 2>&1)
    probe_status=$?
    set -e
    assert_equal "23" "$probe_status" "the candidate should not write lab control files"
    assert_contains "blocked:" "$output" "the candidate should observe read-only lab metadata"

    output=$(run_lab use codex-nc)
    assert_contains "Using codex-nc" "$output" "use should switch installer adapters"
    assert_session_stopped "$claude_session"
    output=$(run_lab show)
    assert_contains "Implementation: codex-nc" "$output" "show should report the switched implementation"

    output=$(run_lab use codex-fc)
    assert_contains "Using codex-fc" "$output" "use should select the full-context implementation"

    output=$(run_lab show)
    final_session=$(awk -F ': ' '$1 == "Session" { print $2 }' <<< "$output")
    run_lab stop >/dev/null
    assert_session_stopped "$final_session"
    output=$(run_lab show)
    assert_equal "No active lab session." "$output" "stop should clear the active session"
}

test_plasma_shortcut_registration() {
    local output shortcut_status

    mkdir -p "$(dirname -- "$TEST_HOST_DESKTOP")"
    printf '[Desktop Entry]\nName=Not owned by the lab\n' > "$TEST_HOST_DESKTOP"
    output=$(run_lab use claude)
    assert_contains "Using claude" "$output" "default use should ignore unrelated host shortcut files"
    [[ -f "$TEST_HOST_DESKTOP" ]] || fail "default use removed an unrelated host desktop entry"
    run_lab stop >/dev/null
    [[ -f "$TEST_HOST_DESKTOP" ]] || fail "stop removed an unrelated host desktop entry"
    set +e
    output=$(run_lab use claude --plasma-shortcut 2>&1)
    shortcut_status=$?
    set -e
    assert_equal "1" "$shortcut_status" "use should reject a host desktop-file ownership conflict"
    assert_contains "will not overwrite" "$output" "use should explain the host desktop-file conflict"
    rm -f -- "$TEST_HOST_DESKTOP"

    printf '%s\n' org.example.existing.desktop _launch Existing 'Existing shortcut' > "$TEST_KGLOBALACCEL_OWNER"
    set +e
    output=$(run_lab use claude --plasma-shortcut 2>&1)
    shortcut_status=$?
    set -e
    assert_equal "1" "$shortcut_status" "use should reject a live shortcut conflict"
    assert_contains "already belongs to Existing" "$output" "use should name the conflicting shortcut owner"
    rm -f -- "$TEST_KGLOBALACCEL_OWNER"

    output=$(run_lab use claude --plasma-shortcut)
    assert_contains "Plasma shortcut: registered" "$output" "use should report host shortcut registration"
    [[ -f "$TEST_HOST_DESKTOP" ]] || fail "use did not create the host desktop entry"
    output=$(<"$TEST_HOST_DESKTOP")
    assert_contains "X-KDE-Shortcuts=Meta+Ctrl+Shift+A" "$output" "host desktop entry should register the candidate shortcut"
    assert_contains "X-AIT-Lab-Project=$TEST_ROOT" "$output" "host desktop entry should record its owner"
    output=$(run_lab show)
    assert_contains "Plasma shortcut: registered" "$output" "show should report host shortcut registration"
    output=$(run_lab shortcut)
    assert_contains "notification: requested" "$output" "the claude shortcut should request a notification"
    output=$(run_lab app status)
    assert_equal "off" "$output" "the host shortcut bridge should toggle the active candidate"

    run_lab use codex-nc --plasma-shortcut >/dev/null
    output=$(run_lab shortcut)
    assert_contains "notification: implicit" "$output" "the codex-nc shortcut should use its implicit notification"

    run_lab use codex-fc --plasma-shortcut >/dev/null
    output=$(run_lab shortcut)
    assert_contains "notification: requested" "$output" "the codex-fc shortcut should request a notification"

    run_lab use claude >/dev/null
    [[ ! -e "$TEST_HOST_DESKTOP" ]] || fail "switching without the option left the host desktop entry installed"
    output=$(run_lab show)
    assert_contains "Plasma shortcut: disabled" "$output" "show should report that host shortcut registration is disabled"

    run_lab use codex-nc --plasma-shortcut >/dev/null
    run_lab stop >/dev/null
    [[ ! -e "$TEST_HOST_DESKTOP" ]] || fail "stop left the host desktop entry installed"
    output=$(<"$TEST_KGLOBALACCEL_LOG")
    assert_contains "unregister ait-lab-agent-instructions-toggle.desktop _launch" "$output" "cleanup should unregister the KGlobalAccel action"
    output=$(<"$TEST_HOST_REFRESH_LOG")
    assert_contains "--noincremental" "$output" "registration and cleanup should refresh host desktop metadata"
}

test_show_reports_no_active_session
test_use_starts_an_isolated_candidate
test_plasma_shortcut_registration
printf 'PASS: ait-lab interface tests\n'
