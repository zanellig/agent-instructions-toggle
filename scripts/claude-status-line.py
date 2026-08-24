#!/usr/bin/env python3
import json
import subprocess
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


def main() -> int:
    config_path = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).with_name(
        "claude-status-line.json"
    )
    try:
        config = json.loads(config_path.read_text(encoding="utf-8"))
        binary = config["binary"]
        base_command = config["base_command"]
        if not isinstance(binary, str) or not binary:
            raise TypeError("binary path must be a string")
        if not isinstance(base_command, str):
            raise TypeError("base command must be a string")
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(
            f"agent-instructions status line: invalid installation: {safe_message(error)}",
            file=sys.stderr,
        )
        return 1

    payload = sys.stdin.buffer.read()
    base_output = b""
    if base_command:
        try:
            # This is the command Claude was already configured to execute through a shell.
            base = subprocess.run(
                ["/bin/sh", "-c", base_command],
                input=payload,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )
            base_output = base.stdout.rstrip(b"\n")
            sys.stderr.buffer.write(base.stderr)
        except OSError as error:
            print(f"existing status line failed: {safe_message(error)}", file=sys.stderr)

    try:
        status = subprocess.run(
            [binary, "status"],
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            check=False,
        )
        token = status.stdout.strip().decode("ascii")
    except (OSError, UnicodeError):
        token = "conflict"
    if token not in {"on", "off", "mixed", "conflict"}:
        token = "conflict"

    if base_output:
        sys.stdout.buffer.write(base_output + b" ")
    sys.stdout.buffer.write(f"AGENTS:{token}\n".encode("ascii"))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
