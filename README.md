# Agent instruction toggle

`agent-instructions` controls whether global `AGENTS.md` and `CLAUDE.md` documents load into new coding-agent contexts. Disabling renames each document by appending `.no-auto-inject`. Enabling restores the recognized filename. It never deletes or empties a document.

The change applies when a new context starts. An existing Codex terminal session, app-server thread, ACP thread, or Claude Code context keeps the instructions it already loaded.

## Managed profiles

The utility discovers immediate child directories of `$HOME` whose names use these forms:

- `.codex`, `.codex_*`, `.codex-*`, and `.codex.*` contain `AGENTS.md`.
- `.claude`, `.claude_*`, `.claude-*`, and `.claude.*` contain `CLAUDE.md`.

Names that look archived are excluded. The ignored markers are `bak`, `backup`, `archive`, `old`, and `copy`, including common variants such as `.codex_personal.bak`, `.codex_backup2`, and an editor `~` suffix. Discovery happens on every operation, so adding an active profile does not require a configuration change.

An existing profile directory with neither the recognized nor disabled filename is reported as missing. A directory that does not exist is not treated as a target.

## States and recovery

| State | Meaning |
| --- | --- |
| `on` | Every present target uses its recognized filename. |
| `off` | Every present target uses its `.no-auto-inject` filename. |
| `mixed` | Present targets use both forms, with no same-target collision. |
| `conflict` | A target has both filenames, or no targets can establish a state. |

Missing documents do not block healthy targets. A collision blocks every mutation and preserves both copies. A mutation requested in `mixed` state first recovers every present target to `on`, reports the recovery, and stops. Run the command again if disabling was still the intent.

Mutations share one filesystem lock. The utility validates every planned rename before starting. If a later rename fails, it attempts to roll back earlier renames before returning an error.

## CLI

```sh
agent-instructions status
agent-instructions enable
agent-instructions disable
agent-instructions toggle
agent-instructions tray
```

`status` writes one token to standard output: `on`, `off`, `mixed`, or `conflict`. Missing-target and collision details go to standard error, so status-line scripts can consume the token safely. Every observable state returns success from `status`. Conflicts and filesystem errors return failure from mutating commands.

The `tray` command starts a single windowless StatusNotifier indicator. Its icon is green for `on`, gray for `off`, amber for `mixed`, and red for `conflict`. Missing targets add an amber marker. The tooltip includes `AGENTS: <state>` and names missing targets. Primary click opens a menu with Enable, Disable, Status, and Quit. Quitting the tray does not stop the CLI or desktop shortcut.

## Install on KDE Plasma

The installer requires a stable Rust toolchain and Python 3.

```sh
./install.sh
```

It builds the locked release, copies the binary to `~/.local/bin`, installs application and autostart desktop entries, registers `Meta+Ctrl+Shift+A` through the desktop entry, refreshes KDE service metadata, and validates the entries when `desktop-file-validate` is available.

If `~/.claude` exists, the installer extends its configured status-line command. It preserves the previous command and appends `AGENTS:on`, `AGENTS:off`, `AGENTS:mixed`, or `AGENTS:conflict` to its output. The wrapper passes Claude's session input to the existing command and does not add Git polling.

Notification delivery uses the desktop notification service and is best effort. A missing notification service does not change the result of a successful rename.

## Another window manager

The state operation has no KDE dependency. Bind this installed command in the window manager:

```sh
$HOME/.local/bin/agent-instructions toggle
```

Start `$HOME/.local/bin/agent-instructions tray` from the window manager's autostart mechanism and provide a StatusNotifier host if an indicator is wanted. The CLI and shortcut operation work without the tray.

## Verification

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release --locked
desktop-file-validate ~/.local/share/applications/agent-instructions-toggle.desktop
desktop-file-validate ~/.config/autostart/agent-instructions-tray.desktop
```

On Plasma, inspect the registered shortcut in System Settings under Shortcuts, then confirm the tray tooltip, its four menu actions, and state updates after a CLI transition.
