# agent-instructions

`agent-instructions` controls whether global instruction documents are discoverable when a coding agent creates a new context. It renames documents in place. It does not delete or edit their contents.

The CLI always manages `$HOME/.claude/CLAUDE.md`. It also discovers active Codex profiles under `$HOME` on every command. Each discovered profile contributes an `AGENTS.md` managed target.

An active Codex profile is an immediate `$HOME` child whose name starts with `.codex` and which is a directory or a symlink to a directory. Accepted profiles are processed in sorted order. A profile created after one command is discovered by the next command.

Backup-like profiles are not managed. Discovery excludes names ending in `~` and names with a separator-delimited `bak`, `backup`, `old`, `orig`, `copy`, `archive`, `save`, or `disabled` token. Matching ignores ASCII case, and the token may end in digits, such as `backup2`. A longer word such as `backupish` is not a backup token.

Disabled documents append `.no-auto-inject` to the recognized filename.

## Commands

```sh
agent-instructions status
agent-instructions status --machine
agent-instructions enable
agent-instructions disable
agent-instructions toggle
```

`status` prints the aggregate instruction state and exits successfully for every observable state. `status --machine` prints only `on`, `off`, `mixed`, or `conflict` to standard output. Missing-target and collision details go to standard error, so command substitution receives one token.

`enable` restores recognized filenames. `disable` appends `.no-auto-inject`. `toggle` selects the opposite of a uniform `on` or `off` state. Repeating `enable` or `disable` is harmless.

All mutations share one filesystem lock under `$XDG_RUNTIME_DIR`, with `$XDG_STATE_HOME` as the fallback. Before renaming anything, the command validates every source and destination. It never replaces an existing destination. If a later rename fails, it attempts to restore the renames already completed and returns an error.

## States

- `on`: every present target has its recognized filename.
- `off`: every present target has its disabled filename.
- `mixed`: present targets are split between recognized and disabled names.
- `conflict`: a target has both names, or every managed target is missing.

Missing targets produce a warning but do not block changes to healthy targets. A collision blocks mutation and preserves both files. Total absence also blocks mutation.

A mutation requested from `mixed` performs conservative recovery: it restores all present targets to `on`, reports the recovery, and stops. Run a second command if you still want to disable the documents.

Changes apply only when a coding agent creates a new context. Existing Codex, T3 Code, and Claude Code contexts retain the instructions they already loaded.

## Build and test

```sh
cargo build --release
cargo test
```
