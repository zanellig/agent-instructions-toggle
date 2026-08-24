#!/usr/bin/env python3
import argparse
import hashlib
import json
import os
import shlex
import shutil
import sys
from pathlib import Path


def safe_message(value: object) -> str:
    rendered = []
    for character in str(value):
        if not character.isprintable() or character in "<>&":
            rendered.append(f"\\u{{{ord(character):x}}}")
        else:
            rendered.append(character)
    return "".join(rendered)


def atomic_write(path: Path, contents: str, mode: int) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp-{os.getpid()}")
    temporary.write_text(contents, encoding="utf-8")
    temporary.chmod(mode)
    os.replace(temporary, path)


def desktop_argument(value: str) -> str:
    if any(ord(character) < 0x20 or ord(character) == 0x7F for character in value):
        raise ValueError("the installation path contains unsupported control characters")
    escaped = value.replace("\\", "\\\\").replace('"', '\\"').replace("`", "\\`")
    escaped = escaped.replace("$", "\\$").replace("%", "%%")
    return f'"{escaped}"'


def render_desktop(template: Path, destination: Path, binary: Path) -> None:
    contents = template.read_text(encoding="utf-8")
    rendered = contents.replace("@BINARY@", desktop_argument(str(binary)))
    atomic_write(destination, rendered, 0o644)


def merge_claude_status(
    claude_home: Path, binary: Path, wrapper: Path, config_path: Path
) -> None:
    settings_path = claude_home / "settings.json"
    if settings_path.exists():
        settings = json.loads(settings_path.read_text(encoding="utf-8"))
        if not isinstance(settings, dict):
            raise ValueError("Claude settings must contain a JSON object")
    else:
        settings = {}

    wrapper_command = shlex.join(["python3", str(wrapper), str(config_path)])
    existing = settings.get("statusLine")
    existing_command = existing.get("command") if isinstance(existing, dict) else None
    if existing_command == wrapper_command and config_path.exists():
        installed_config = json.loads(config_path.read_text(encoding="utf-8"))
        base_command = installed_config.get("base_command", "")
    elif isinstance(existing_command, str) and existing_command.strip():
        base_command = existing_command
    else:
        base_command = ""
    if not isinstance(base_command, str):
        raise ValueError("the saved status-line command is invalid")

    atomic_write(
        config_path,
        json.dumps(
            {"binary": str(binary), "base_command": base_command},
            indent=2,
        )
        + "\n",
        0o600,
    )
    status_line = dict(existing) if isinstance(existing, dict) else {}
    status_line.update({"type": "command", "command": wrapper_command})
    settings["statusLine"] = status_line
    atomic_write(settings_path, json.dumps(settings, indent=2) + "\n", 0o600)


def discovered_claude_homes(home: Path, source: Path) -> list[Path]:
    markers = [
        marker
        for marker in (source / "assets" / "archive-markers.txt")
        .read_text(encoding="utf-8")
        .splitlines()
        if marker
    ]
    profiles = []
    for candidate in home.iterdir():
        name = candidate.name
        if not candidate.is_dir() or not name.startswith(".claude"):
            continue
        suffix = name[len(".claude") :].lower()
        if suffix.endswith("~") or any(marker in suffix for marker in markers):
            continue
        profiles.append(candidate)
    return sorted(profiles)


def status_config_path(data_home: Path, claude_home: Path) -> Path:
    identifier = hashlib.sha256(os.fsencode(claude_home)).hexdigest()[:16]
    return data_home / "agent-instructions" / f"claude-status-line-{identifier}.json"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--home", type=Path, required=True)
    parser.add_argument("--data-home", type=Path, required=True)
    parser.add_argument("--config-home", type=Path, required=True)
    parser.add_argument("--binary", type=Path, required=True)
    return parser.parse_args()


def main() -> int:
    try:
        args = parse_args()
        application = args.data_home / "applications" / "agent-instructions-toggle.desktop"
        autostart = args.config_home / "autostart" / "agent-instructions-tray.desktop"
        render_desktop(
            args.source / "assets" / "agent-instructions-toggle.desktop.in",
            application,
            args.binary,
        )
        render_desktop(
            args.source / "assets" / "agent-instructions-tray.desktop.in",
            autostart,
            args.binary,
        )

        wrapper = args.data_home / "agent-instructions" / "claude-status-line.py"
        wrapper.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(args.source / "scripts" / "claude-status-line.py", wrapper)
        wrapper.chmod(0o755)
        for claude_home in discovered_claude_homes(args.home, args.source):
            merge_claude_status(
                claude_home,
                args.binary,
                wrapper,
                status_config_path(args.data_home, claude_home),
            )
    except (OSError, ValueError, TypeError) as error:
        print(
            f"install.sh: could not install desktop integration: {safe_message(error)}",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
