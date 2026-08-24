#!/usr/bin/env python3
import json
import subprocess
import sys
from pathlib import Path


def main() -> int:
    config_path = Path(__file__).with_name("claude-status-line.json")
    try:
        config = json.loads(config_path.read_text(encoding="utf-8"))
        binary = config["binary"]
        base_command = config["base_command"]
        if not isinstance(binary, str) or not binary:
            raise TypeError("binary path must be a string")
        if not isinstance(base_command, list) or not all(
            isinstance(argument, str) and argument for argument in base_command
        ):
            raise TypeError("base command must be a list of strings")
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(f"agent-instructions status line: invalid installation: {error}", file=sys.stderr)
        return 1

    payload = sys.stdin.buffer.read()
    base_output = b""
    if base_command:
        try:
            base = subprocess.run(
                base_command,
                input=payload,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )
            base_output = base.stdout.rstrip(b"\n")
            sys.stderr.buffer.write(base.stderr)
        except OSError as error:
            print(f"existing status line failed: {error}", file=sys.stderr)

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
