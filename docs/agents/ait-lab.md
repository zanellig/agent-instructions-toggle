# Implementation lab

Use `./ait-lab` to exercise one candidate implementation at a time without installing it into the real home directory.

The lab requires Linux with a running user-systemd session, Bubblewrap, `xdg-dbus-proxy`, Rustup, `jq`, and GNU tar. The interface test also requires `dbus-daemon`.

## Select an implementation

```sh
./ait-lab use claude
./ait-lab use codex-nc
./ait-lab use codex-fc
```

The identifiers select these worktree branches:

| Identifier | Branch |
| --- | --- |
| `claude` | `spec-1-agent-instructions` |
| `codex-nc` | `issue-1-agent-instructions` |
| `codex-fc` | `codex/full-context-implementation` |

`use` discovers the branch through `git worktree list`, snapshots its current files, builds and installs that snapshot, and starts its tray. Tracked modifications and untracked non-ignored files participate in the snapshot. Build and installation must finish before the active tray stops. If the new tray cannot start, the previous session starts again.

Each selection starts with fresh instruction documents and Claude settings. Switching back creates another fresh session rather than reusing changed state.

## Exercise the active implementation

Pass CLI arguments through `app`:

```sh
./ait-lab app status
./ait-lab app toggle --notify
./ait-lab app enable --notify
```

Notifications appear in the real desktop session. Instruction-document changes remain in the synthetic home.

Run the status-line command installed into the default synthetic Claude profile:

```sh
./ait-lab statusline normal
```

This feeds `tests/ait-lab/fixtures/statusline-inputs/normal.json` to the installed command. The `BASE` prefix confirms that the installer preserved the pre-existing status line.

Launch Claude against the same synthetic home:

```sh
./ait-lab claude
```

The Claude process has network access and an empty project directory. It cannot read the real home directory or inherit authentication variables. Complete a separate login if needed. Credentials created there remain under `.ait-lab-state/`.

## Inspect and stop

```sh
./ait-lab show
./ait-lab stop
```

`show` reports the selected branch, revision, snapshot digest, tray state, fake home, and logs. `stop` removes the tray and D-Bus proxy by stopping their transient user-systemd unit. Session files remain available for comparison.

## Isolation

Candidate build scripts and binaries see `/usr`, `/etc`, a read-only Rust toolchain, and lab-owned writable directories. The real home and every worktree remain absent from the sandbox. A filtered D-Bus proxy permits notifications and StatusNotifier traffic. Runtime networking is disabled except for `./ait-lab claude`.

Installers write desktop entries into the synthetic XDG directories. Desktop-cache refresh commands are stubs, so the lab does not register the candidate shortcut or autostart entry with the live desktop. Test those persistent integrations in a disposable Plasma user or virtual machine.

Generated homes, build outputs, Cargo downloads, sockets, credentials, and logs stay under `.ait-lab-state/`, which Git ignores. `git clean -fdx` can still remove the locked Claude worktree under `.claude/worktrees`, so do not run it in the main checkout.

## Verify the lab

```sh
./tests/ait-lab/run.sh
bash -n ./ait-lab ./tests/ait-lab/run.sh
```

The interface test uses local candidate, Claude, and D-Bus stand-ins. It checks selection, switching, CLI execution, status-line rendering, filesystem isolation, and process cleanup through the public commands.
