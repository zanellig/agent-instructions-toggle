# agent-instructions

`agent-instructions` controls whether global instruction documents are discoverable when a coding agent creates a context. It renames documents in place. It does not delete or edit their contents.

## Profile discovery

Every inspection scans the immediate children of `$HOME`. Existing directories and directory symlinks named `.codex*` manage `AGENTS.md`; those named `.claude*` manage `CLAUDE.md`. The scan happens again for every command, so profiles added after installation take part in the next status check or transition. Names are sorted before inspection.

Examples of active profiles include `$HOME/.codex`, `$HOME/.codex-work`, `$HOME/.claude`, and `$HOME/.claude-team`. The tool does not invent a default profile when a directory does not exist. An existing profile directory with neither recognized nor disabled instruction filename remains visible as a missing target.

Backup-like profiles are excluded. The tool inspects the part after `.codex` or `.claude`, ignoring ASCII case, and excludes a name if that suffix contains `bak`, `backup`, `archive`, `old`, `orig`, `copy`, `save`, or `disabled`. It also excludes names ending in `~`. For example, `$HOME/.codex-backup2`, `$HOME/.claude.old`, and `$HOME/.codex-session~` are left untouched. Regular files and broken directory symlinks are not profiles.

Disabled documents append `.no-auto-inject` to the recognized filename.

## Commands

```sh
agent-instructions status
agent-instructions status --machine
agent-instructions status --segment
agent-instructions enable
agent-instructions disable
agent-instructions toggle
agent-instructions toggle --notify
agent-instructions tray
```

`status` prints the aggregate instruction state and exits successfully for every observable state. `status --machine` prints only `on`, `off`, `mixed`, or `conflict` to standard output. `status --segment` prints the colored `AGENTS:<state>` status-line segment. Missing-target and collision details go to standard error, so command substitution receives one token or one segment.

`enable` restores recognized filenames. `disable` appends `.no-auto-inject`. `toggle` selects the opposite of a uniform `on` or `off` state. Repeating `enable` or `disable` is harmless.

Pass `--notify` to `enable`, `disable`, or `toggle` to request a desktop notification. Notification delivery is best effort. A missing notification daemon or failed notification does not change the command result or roll back a completed instruction-state transition.

All mutations share one filesystem lock under `$XDG_RUNTIME_DIR`, with `$XDG_STATE_HOME` as the fallback. Before renaming anything, the command validates every source and destination. It never replaces an existing destination. If a later rename fails, it attempts to restore completed renames and returns an error. A notified failure says the documents are unchanged only when no rename completed or rollback restored every completed rename. If rollback also fails, the notification says the state is uncertain and directs the user to inspect it.

## Instruction states

- `on`: every present target has its recognized filename.
- `off`: every present target has its disabled filename.
- `mixed`: present targets are split between recognized and disabled names.
- `conflict`: a target has both names, or every discovered target is missing, including when no active profiles exist.

Missing targets produce a warning but do not block changes to healthy targets. A collision blocks mutation and preserves both files. Total absence also blocks mutation.

A mutation requested from `mixed` performs conservative recovery. It restores all present targets to `on`, reports the recovery, and stops. Run a second command if you still want to disable the documents.

## When a changed state applies

An instruction-state transition affects contexts that load global instructions afterward. It does not rewrite the prompt of a context that is already live.

| Client case | Reads the current files? |
|---|---|
| New direct Codex context | Yes |
| Later turn in an already-live Codex context | No; it keeps the instructions loaded at context creation |
| New T3 Code Codex app-server provider session | Yes |
| Later turn in an already-live T3 provider context | No |
| New ACP-created root thread | Yes |
| Later turn while the ACP context remains live | No |
| Cold resume that creates a fresh agent process and context | Yes |
| Reconnection to a context that is still live | No |
| New Claude Code context | Yes |
| Later turn in an already-live Claude Code context | No |

If a client calls a reconnect a "resume," the process lifetime is what matters. A cold resume reloads global files. A live context retains what it already loaded.

## Tray indicator

`agent-instructions tray` runs a windowless status indicator. Only one tray process runs per user session. Quitting it stops the indicator only; the installed CLI and desktop shortcut remain available.

The icon is green for `on`, gray for `off`, amber for `mixed`, and red for `conflict`. Its tooltip begins with `AGENTS: <state>` and lists missing or conflicting targets on following lines. A missing target adds an amber warning overlay without replacing the state color.

Primary activation opens the menu. Enable and Disable make an explicit instruction-state request, Status reports the current state, and Quit stops the tray. The tray uses native filesystem events for profile and instruction-document changes. It does not poll or rename files automatically.

Plasma may initially put the indicator in the tray overflow. To keep it visible, open **Configure System Tray**, select **Entries**, find **Agent Instructions**, set its visibility to **Always shown**, and apply the change.

## Install on KDE Plasma

Install the release binary and desktop integrations for the current user:

```sh
./install.sh
```

The installer runs `cargo build --release --locked` and copies the binary to `${XDG_BIN_HOME:-$HOME/.local/bin}/agent-instructions`. It installs application, KGlobalAccel, and XDG autostart desktop entries under `${XDG_DATA_HOME:-$HOME/.local/share}`. The Plasma shortcut is `Meta+Ctrl+Shift+A`. The installer refreshes service metadata with `kbuildsycoca6`, or `kbuildsycoca5` on Plasma 5. It does not edit `kglobalshortcutsrc` or restart KGlobalAccel.

When active Claude profiles exist, installation requires `jq`. For each profile, the installer parses `settings.json`, preserves its existing command status line, and points Claude Code at a project-owned wrapper in that profile. The wrapper forwards Claude's session JSON to the prior command unchanged, then appends the installed binary's `status --segment` output. The binary owns the shared green, gray, amber, and red state appearance used by both the Claude segment and tray icon. Missing-target details remain visible in amber beside the base state.

Claude Code runs the wrapper on its normal status-line refresh after an interaction. The integration does not add an idle timer or polling loop.

For safety, the installer directly invokes a prior status command as an executable plus literal arguments. It refuses settings with shell operators, expansion, quoting, or invalid JSON before changing installed files. If the current command is complex, put that logic in an executable script and configure `statusLine.command` as the script's absolute path before installing.

Re-running `./install.sh` updates the copied binary, desktop entries, and wrappers. It keeps the original per-profile status command recorded by the first installation and integrates any active Claude profiles discovered since the previous run.

## Verify the installation

Press `Meta+Ctrl+Shift+A`, then inspect the state:

```sh
"${XDG_BIN_HOME:-$HOME/.local/bin}/agent-instructions" status
```

Plasma exposes the installed launch action as `_launch`. Inspect it through KGlobalAccel:

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

Run every installed Claude wrapper with representative session input:

```sh
binary="${XDG_BIN_HOME:-$HOME/.local/bin}/agent-instructions"
while IFS= read -r -d '' profile; do
  printf '%s' '{"cwd":"/tmp/status-check","model":{"display_name":"Test"}}' |
    "$profile/agent-instructions-statusline.sh"
done < <("$binary" profiles --claude --null)
```

Each line should retain the segments from that profile's prior command and add the colored `AGENTS:<state>` segment. Profiles with missing targets also show their missing-target detail.

## Remove the integration

```sh
./install.sh --uninstall
```

Removal restores each recorded Claude status line when the current command still points to the project wrapper. If the user changed that command after installation, removal leaves the new setting alone. It removes only the project-owned wrappers, metadata, copied binary, and desktop entries, then refreshes Plasma metadata. It does not alter instruction documents or unrelated settings and desktop entries.

After removal, the instruction documents stay in their current `on`, `off`, `mixed`, or `conflict` arrangement. Run `enable` before uninstalling if you want recognized filenames restored.

## Other Linux desktops and window managers

The instruction-state operation does not depend on KDE. Bind this command with the shortcut configuration for the new desktop or window manager:

```sh
"${XDG_BIN_HOME:-$HOME/.local/bin}/agent-instructions" toggle --notify
```

Arrange login startup for `agent-instructions tray` through that environment's autostart mechanism. An XDG-compliant desktop may use the installed autostart entry; a window manager may need its native startup configuration.

The tray implements StatusNotifier. The desktop session must run a StatusNotifier host, such as a compatible panel or tray service, for the icon and menu to appear. Notifications also require a freedesktop-compatible notification daemon. Rebinding, autostart, and the host are desktop configuration; the binary and instruction-state rules stay the same.

## Out of scope

The project does not remove instructions from already-live Codex, T3 Code, ACP, or Claude Code contexts. It does not toggle project-level instruction documents, manage user-defined discovery patterns or backup directories, wrap agent launches, or run a background renamer for newly created documents.

Compositor-independent privileged key daemons are outside scope, as are custom Codex status fields, Codex forks, terminal-title workarounds, T3 Code UI changes, and non-Linux support. The tool also does not promise one atomic transaction across every managed file; it preflights, uses atomic per-file renames, and rolls back ordinary later failures.

## Build and test

```sh
cargo build --release
cargo test
```
