# agent-instructions

Turns your global agent instruction documents on and off for **new** agent
contexts, without deleting or editing them.

Disabling renames each managed document to a name no coding agent recognizes:

```
~/.codex/AGENTS.md   ->  ~/.codex/AGENTS.md.no-auto-inject
~/.claude/CLAUDE.md  ->  ~/.claude/CLAUDE.md.no-auto-inject
```

Contents and permissions are untouched. Enabling renames them back.

Linux only.

## What gets managed

Codex profiles are discovered under `$HOME` on every run. Nothing is
hard-coded and there is no configuration file, so a profile you add later is
picked up without touching this tool.

| Target | How it is found | Document |
|---|---|---|
| Codex profiles | every directory under `$HOME` named `.codex` or `.codex` plus a separator or a digit | `AGENTS.md` |
| Claude Code | the default home, `~/.claude` | `CLAUDE.md` |

So `.codex`, `.codex2`, `.codex_p2` and `.codex-work` are all profiles, while
`.codexrc` is an unrelated dotfile: the suffix has to start at a separator or a
digit. A symlink to a directory counts as a profile. Plain files never do, so
`.claude.json` is not a target.

Claude Code reads one global document, so `~/.claude` is fixed rather than
discovered. It stays a managed target even if the directory is missing, which
is what turns an absent `CLAUDE.md` into a reported warning.

### Backup profiles are left alone

A profile whose name carries one of these words is treated as an archive and
skipped: `archive`, `archived`, `backup`, `backups`, `bak`, `copy`, `disabled`,
`old`, `orig`, `original`, `save`, `saved`. A trailing `~` counts too.

Matching is on whole words, so `.codex_old` is a backup and `.codex_bold` is a
real profile. `.codex.bak`, `.codex_backup2`, `.codex-OLD` and `.codex_p~` are
all skipped.

Skipped profiles are listed by `agent-instructions status`:

```
AGENTS: on
New agent contexts load your global instructions.
ignored: ~/.codex_backup (looks like a backup)
```

That line is there so a profile the tool declines to manage is never a silent
omission. If one of your real profiles shows up in it, rename the directory.

## Install

```sh
./install.sh
```

That builds a release binary, copies it to `~/.local/bin/agent-instructions`,
installs the application and autostart desktop entries, binds
`Meta+Ctrl+Shift+A`, refreshes KDE's service metadata, and merges an `AGENTS`
segment into your existing Claude Code status line.

The installed binary does not depend on this checkout. You can move or delete
the source afterwards.

Re-running the installer is safe. It only touches its own artifacts, and it
will not wrap your status line twice.

The KDE binding is read out of the service cache, which is rebuilt at login.
If `--verify` reports the shortcut as unbound right after installing, log out
and back in.

## Commands

| Command | What it does |
|---|---|
| `agent-instructions status` | Prints the state, what it means, and any warnings |
| `agent-instructions status --machine` | Prints one token: `on`, `off`, `mixed`, `conflict` |
| `agent-instructions enable` | Restores the recognized names |
| `agent-instructions disable` | Renames to the disabled names |
| `agent-instructions toggle` | Flips between the two |
| `agent-instructions tray` | Runs the tray indicator in the foreground |

`enable`, `disable`, and `toggle` accept `--notify` to send a desktop
notification. The shortcut and the tray use it; a terminal run stays quiet.

`status` succeeds for every state it can observe, including `conflict`, so
scripts can read it without special-casing failure. The mutating commands fail
on a conflict or an operational error.

## States

| State | Meaning |
|---|---|
| `on` | Every present document uses its recognized name |
| `off` | Every present document uses its disabled name |
| `mixed` | Present documents are split between the two names |
| `conflict` | Some target has both names, or no managed document exists at all |

A document that is missing entirely does not block the others. It is reported
as a warning and skipped, so deleting an unused Codex profile does not break
the tool.

### How the tool recovers

**Mixed** means something went wrong outside the tool, so it reconciles to `on`
— the conservative direction, where all your configured guidance loads — tells
you it did, and stops there. The mutation you asked for is not carried out.
Run the command again if you still want it.

**Conflict** refuses every mutation and changes nothing. Both copies survive.
Decide which one you want and remove or rename the other by hand.

## Safety

Every mutating command takes one lock under `$XDG_RUNTIME_DIR`, so two
shortcut presses cannot interleave their renames. Each rename is preflighted
and never overwrites an existing file. If one rename fails after others
succeeded, the completed ones are rolled back and the command reports a
failure.

Individual renames are atomic; the set of them is not. An agent launched during
the fraction of a millisecond between two renames could see a split state. That
race is accepted, and `status` will show it as `mixed` if it ever matters.

## What changes, and when

The toggle affects **new** contexts only. A context that has already loaded
your instructions keeps them for its whole life.

| Client | Reads the new state | Keeps the old state |
|---|---|---|
| Codex CLI | A new `codex` invocation, or a cold resume | The TUI session you are in now, on every later turn |
| T3 Code (Codex app-server) | A newly created provider session or root thread | Live threads, on every later turn |
| ACP clients | A newly created root thread | Threads already open |
| Claude Code | A new context | The context you are in now, including after `/clear` in the same process |

There is no way to make a live agent forget what it already read. Ending the
session and starting a new one is the only way to pick up a change.

## The tray

The tray is a windowless StatusNotifier item. Native filesystem events watch
`$HOME`, so a profile you add or remove shows up right away, and each managed
directory, for changes to the documents. It costs nothing while idle and never
reconciles anything on its own.

| Color | State |
|---|---|
| Green | `on` |
| Gray | `off` |
| Amber | `mixed` |
| Red | `conflict` |

An amber dot in the corner means a managed document is missing. The base color
still shows the real state.

The tooltip never relies on color. Its first line is exactly `AGENTS: <state>`,
and any missing document is named on the lines below.

Left-clicking opens the menu instead of toggling, so a stray click cannot
change global behavior. The menu has Enable, Disable, Status, and Quit. Quit
stops the tray process only; the command and the shortcut keep working, and the
tray comes back at your next login.

Only one tray runs per session. Starting a second one exits with a message.

### Making the icon always visible

Plasma files new tray items under the overflow arrow. To pin it: right-click
the system tray, choose **Configure System Tray…**, open **Entries**, find
**Agent instructions tray**, and set it to **Shown**.

## The Claude Code status line

Installation wraps whatever status-line command you already had. Your existing
segments are produced by your original command and passed through unchanged;
the `AGENTS` segment is appended after them.

```
[developer@host repo] main ctx:12% Opus 5 AGENTS:on
```

It is colored with the same four state colors as the tray, and a missing
document adds an amber `+1!` after the state without hiding it. The segment
reads the machine-readable status once per status-line refresh — Claude's
normal cycle. Nothing polls while you are idle.

## Verifying

```sh
./install.sh --verify        # files, desktop entries, KGlobalAccel binding, status line
./scripts/verify-tray.sh     # live StatusNotifier registration, tooltip, menu, updates
cargo test                   # CLI behavior against real files in isolated homes
```

`--verify` reads the shortcut back out of the running KGlobalAccel service
rather than trusting a config file, and it re-runs your original status-line
command to confirm every pre-existing segment is still in the merged output.

`scripts/verify-tray.sh` runs against a throwaway home. It never touches your
real documents.

## Removing

```sh
./install.sh --uninstall
```

That restores your previous status-line command, deletes the binary, both
desktop entries, and the wrapper, and refreshes the desktop caches. Your
instruction documents are left exactly as they are. If they are currently
disabled and you want them back, run `agent-instructions enable` before you
uninstall, or rename them by hand afterwards.

## Another window manager

The operation itself has no KDE dependency. Only the shortcut binding and the
autostart entry do. To move to another Linux window manager:

1. **Rebind the key.** Point your compositor's shortcut configuration at
   `~/.local/bin/agent-instructions toggle --notify`. In Hyprland that is a
   `bind` line, in Sway a `bindsym`, in i3 a `bindsym`. KDE's
   `X-KDE-Shortcuts` field does nothing outside Plasma; ignore it.
2. **Arrange autostart.** Run `~/.local/bin/agent-instructions tray` from your
   compositor's autostart, or keep the XDG entry at
   `~/.config/autostart/agent-instructions-tray.desktop` if your session honors
   XDG autostart.
3. **Provide a StatusNotifier host.** The tray needs one on the session bus.
   Waybar, Polybar with its tray module, and `snixembed` all work. Without a
   host the tray exits with an error, and the command, the shortcut, and the
   Claude status line all keep working without it.

Notifications go through `notify-send`, so any freedesktop notification daemon
will do. If there is none, transitions still succeed; you just do not see a
popup.

## Out of scope

- Removing instructions from a Codex, T3 Code, ACP, or Claude Code context that
  is already live.
- Toggling project-level instruction documents. Only the global ones are
  managed.
- A compositor-independent privileged key daemon. Key binding stays your
  desktop's job.
- Launch wrappers around agent binaries.
- Anything other than Linux.
