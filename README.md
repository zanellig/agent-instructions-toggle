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
agent-instructions toggle --notify
```

`status` prints the aggregate instruction state and exits successfully for every observable state. `status --machine` prints only `on`, `off`, `mixed`, or `conflict` to standard output. Missing-target and collision details go to standard error, so command substitution receives one token.

`enable` restores recognized filenames. `disable` appends `.no-auto-inject`. `toggle` selects the opposite of a uniform `on` or `off` state. Repeating `enable` or `disable` is harmless.

Pass `--notify` to `enable`, `disable`, or `toggle` to request a desktop notification. Notification delivery is best effort. A missing notification daemon or a failed notification does not change the command result or roll back a completed instruction-state transition.

All mutations share one filesystem lock under `$XDG_RUNTIME_DIR`, with `$XDG_STATE_HOME` as the fallback. Before renaming anything, the command validates every source and destination. It never replaces an existing destination. If a later rename fails, it attempts to restore the renames already completed and returns an error.

## States

- `on`: every present target has its recognized filename.
- `off`: every present target has its disabled filename.
- `mixed`: present targets are split between recognized and disabled names.
- `conflict`: a target has both names, or every managed target is missing.

Missing targets produce a warning but do not block changes to healthy targets. A collision blocks mutation and preserves both files. Total absence also blocks mutation.

A mutation requested from `mixed` performs conservative recovery: it restores all present targets to `on`, reports the recovery, and stops. Run a second command if you still want to disable the documents.

Changes apply only when a coding agent creates a new context. Existing Codex, T3 Code, and Claude Code contexts retain the instructions they already loaded.

## KDE Plasma shortcut

Install the release binary and the Plasma shortcut for the current user:

```sh
./install.sh
```

The installer runs `cargo build --release --locked`, then copies the binary to `${XDG_BIN_HOME:-$HOME/.local/bin}/agent-instructions`. It does not create a link to this checkout. It installs the same project-owned desktop entry under `${XDG_DATA_HOME:-$HOME/.local/share}/applications` and `kglobalaccel`, with `Meta+Ctrl+Shift+A` registered through `X-KDE-Shortcuts`. Finally, it runs `kbuildsycoca6`, or `kbuildsycoca5` on Plasma 5, to refresh desktop service metadata. It does not edit `kglobalshortcutsrc` or restart KGlobalAccel.

Re-run `./install.sh` after updating the checkout. The installer replaces only the copied binary and the two `io.github.zanellig.agent-instructions.desktop` files. Other files in those directories remain untouched.

Press `Meta+Ctrl+Shift+A`, then inspect the resulting state:

```sh
"${XDG_BIN_HOME:-$HOME/.local/bin}/agent-instructions" status
```

Plasma exposes the installed launch action as `_launch`. These commands inspect it through the live KGlobalAccel D-Bus interface:

```sh
qdbus6 org.kde.kglobalaccel \
  /component/io_github_zanellig_agent_instructions_desktop \
  org.kde.kglobalaccel.Component.shortcutNames

qdbus6 --literal org.kde.kglobalaccel \
  /component/io_github_zanellig_agent_instructions_desktop \
  org.kde.kglobalaccel.Component.allShortcutInfos

qdbus6 org.kde.kglobalaccel /kglobalaccel \
  org.kde.KGlobalAccel.action 369098817
```

The first command prints `_launch`. The second includes the active and default shortcut values. The last command asks which action owns the Qt key value for `Meta+Ctrl+Shift+A`; it should print the desktop entry ID followed by `_launch`.

Remove the copied binary and both desktop entries safely, then refresh Plasma metadata:

```sh
./install.sh --uninstall
```

Removal does not alter any instruction document or unrelated desktop entry.

On another desktop or window manager, bind `${XDG_BIN_HOME:-$HOME/.local/bin}/agent-instructions toggle --notify` with that environment's shortcut settings. The instruction-state operation itself has no KDE dependency.

## Build and test

```sh
cargo build --release
cargo test
```
