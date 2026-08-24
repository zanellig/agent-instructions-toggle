#!/usr/bin/env bash

set -euo pipefail

readonly TEST_ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
readonly LAB="$TEST_ROOT/ait-lab"
readonly TEST_TEMP=$(mktemp -d)
readonly TEST_STATE="$TEST_TEMP/playground/agent-instructions-toggle/.ait-lab-state"
readonly FAKE_CANDIDATE="$TEST_ROOT/tests/ait-lab/fake-candidate"
readonly FAKE_CLAUDE="$TEST_ROOT/tests/ait-lab/fake-claude"
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
    AIT_LAB_SOURCE_CLAUDE="$FAKE_CANDIDATE" \
    AIT_LAB_SOURCE_CODEX_NC="$FAKE_CANDIDATE" \
    AIT_LAB_SOURCE_CODEX_FC="$FAKE_CANDIDATE" \
    AIT_LAB_CLAUDE_EXECUTABLE="$FAKE_CLAUDE" \
    AIT_LAB_SESSION_BUS_ADDRESS="$LAB_BUS_ADDRESS" \
        "$LAB" "$@"
}

start_test_bus() {
    local -a details

    mapfile -t details < <(dbus-daemon --session --fork --print-address=1 --print-pid=1)
    LAB_BUS_ADDRESS=${details[0]}
    LAB_BUS_PID=${details[1]}
}

test_show_reports_no_active_session() {
    local output

    output=$(AIT_LAB_STATE_ROOT="$TEST_STATE" "$LAB" show)
    assert_equal "No active lab session." "$output" "show should explain that no implementation is active"
}

test_use_starts_an_isolated_candidate() {
    local output claude_session final_session local_lab local_project local_state

    start_test_bus
    local_project="$TEST_TEMP/local-project"
    local_lab="$local_project/ait-lab"
    local_state="$TEST_TEMP/local-state"
    mkdir -p "$local_project/implementations" "$local_project/tests"
    cp "$LAB" "$local_lab"
    ln -s "$FAKE_CANDIDATE" "$local_project/implementations/claude"
    ln -s "$TEST_ROOT/tests/ait-lab" "$local_project/tests/ait-lab"

    output=$(AIT_LAB_STATE_ROOT="$local_state" \
        AIT_LAB_CLAUDE_EXECUTABLE="$FAKE_CLAUDE" \
        AIT_LAB_SESSION_BUS_ADDRESS="$LAB_BUS_ADDRESS" \
        "$local_lab" use claude)
    assert_contains "from implementations/claude" "$output" "use should select an in-repo implementation"
    output=$(AIT_LAB_STATE_ROOT="$local_state" "$local_lab" show)
    assert_contains "Source: implementations/claude" "$output" "show should report the in-repo source"
    AIT_LAB_STATE_ROOT="$local_state" "$local_lab" stop >/dev/null

    output=$(run_lab use claude)
    assert_contains "Using claude" "$output" "use should report the selected implementation"

    output=$(run_lab show)
    assert_contains "Implementation: claude" "$output" "show should name the active implementation"
    claude_session=$(awk -F ': ' '$1 == "Session" { print $2 }' <<< "$output")

    output=$(run_lab app status)
    assert_equal "on" "$output" "app should run the selected implementation"

    output=$(run_lab statusline normal)
    assert_contains "BASE AGENTS:on" "$output" "statusline should run the command installed into the fake Claude profile"

    run_lab app toggle --notify
    output=$(run_lab statusline normal)
    assert_contains "BASE AGENTS:off" "$output" "statusline should observe instruction-state changes"

    output=$(run_lab claude)
    assert_equal "/lab/session/home" "$output" "claude should run with the synthetic home"

    local probe_status
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

test_show_reports_no_active_session
test_use_starts_an_isolated_candidate
printf 'PASS: ait-lab interface tests\n'
