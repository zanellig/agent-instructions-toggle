# Implementation lab

Use `./ait-lab` to exercise one candidate implementation at a time without installing it into the real home directory.

The lab requires Linux with a running user-systemd session, Bubblewrap, `xdg-dbus-proxy`, Rustup, Python 3, `jq`, and GNU tar. The interface test also requires `dbus-daemon`.

Opt-in host shortcut registration requires a live Plasma session, `qdbus6` or `qdbus`, and `kbuildsycoca6` or `kbuildsycoca5`.

## Select an implementation

```sh
./ait-lab use claude
./ait-lab use codex-nc
./ait-lab use codex-fc
```

The identifiers select these in-repository projects:

| Identifier | Directory |
| --- | --- |
| `claude` | `implementations/claude` |
| `codex-nc` | `implementations/codex-nc` |
| `codex-fc` | `implementations/codex-fc` |

`use` snapshots the selected directory, builds and installs that snapshot, and starts its tray. Tracked modifications and untracked non-ignored files inside the project participate in the snapshot. Build and installation must finish before the active tray stops. If the new tray cannot start, the previous session starts again.

Each selection starts with fresh instruction documents and Claude settings. Switching back creates another fresh session rather than reusing changed state.

## Register the shortcut in Plasma

Add `--plasma-shortcut` when selecting an implementation to register `Meta+Ctrl+Shift+A` in the live Plasma session:

```sh
./ait-lab use claude --plasma-shortcut
./ait-lab use codex-nc --plasma-shortcut
./ait-lab use codex-fc --plasma-shortcut
```

The option creates one lab-owned host desktop action. That action runs `./ait-lab shortcut`, which dispatches to the active candidate inside its runtime sandbox. It uses `toggle` for `codex-nc` and `toggle --notify` for `claude` and `codex-fc`, matching each candidate's installed shortcut behavior without running a candidate binary directly on the host.

The lab refuses to overwrite a desktop entry that it does not own and refuses to steal `Meta+Ctrl+Shift+A` from another Plasma action. Switching with the option retargets the host action to the new session. Switching without it or running `./ait-lab stop` unregisters the action and removes the generated host files. `./ait-lab show` reports `registered`, `disabled`, or `missing` for the active session.

Host registration is off by default. The option tests a real keypress through the lab bridge, but it does not prove that a candidate installer registers persistent desktop metadata correctly. Use a disposable Plasma user or virtual machine for that installer test.

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

`show` reports the selected source, revision, snapshot digest, tray state, fake home, and logs. `stop` removes the tray and D-Bus proxy by stopping their transient user-systemd unit. Session files remain available for comparison.

## Isolation

Candidate build scripts and binaries see `/usr`, `/etc`, a read-only Rust toolchain, and lab-owned writable directories. The real home and repository checkout remain absent from the sandbox. A filtered D-Bus proxy permits notifications and StatusNotifier traffic. The opt-in Plasma shortcut remains host-owned and enters the candidate through the existing runtime sandbox; candidates do not receive access to KGlobalAccel. Runtime networking is disabled except for `./ait-lab claude`.

Installers write desktop entries into the synthetic XDG directories. Desktop-cache refresh commands are stubs, so the candidate installer does not register its shortcut or autostart entry with the live desktop. The optional host action described above is the only exception. Test persistent installer integrations in a disposable Plasma user or virtual machine.

Generated homes, build outputs, Cargo downloads, credentials, and logs stay under `.ait-lab-state/`, which Git ignores. The filtered D-Bus socket lives under `$XDG_RUNTIME_DIR/ait-lab` so long checkout paths cannot exceed the Unix socket path limit; the supervisor removes it when the session stops. The runtime directory and its `ait-lab` child must be real directories owned by the current user with mode `0700`.

## Verify the lab

```sh
./tests/ait-lab/run.sh
bash -n ./ait-lab ./tests/ait-lab/run.sh
```

The interface test uses local candidate, Claude, KGlobalAccel, desktop-cache, and D-Bus stand-ins. It checks selection, switching, CLI execution, status-line rendering, filesystem isolation, opt-in host shortcut dispatch and cleanup, and process cleanup through the public commands.
