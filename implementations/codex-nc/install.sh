#!/usr/bin/env bash
set -euo pipefail

source_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
binary_source=""
refresh_desktop=true

while (($# > 0)); do
  case "$1" in
    --binary)
      if (($# < 2)); then
        echo "install.sh: --binary needs a path" >&2
        exit 2
      fi
      binary_source=$2
      shift 2
      ;;
    --no-refresh)
      refresh_desktop=false
      shift
      ;;
    *)
      echo "usage: ./install.sh [--binary PATH] [--no-refresh]" >&2
      exit 2
      ;;
  esac
done

if [[ -z "$binary_source" ]]; then
  cargo build --release --locked --manifest-path "$source_dir/Cargo.toml"
  binary_source="$source_dir/target/release/agent-instructions"
fi
if [[ ! -f "$binary_source" ]]; then
  echo "install.sh: binary not found" >&2
  exit 1
fi

home_dir=${HOME:?HOME is not set}
bin_dir="$home_dir/.local/bin"
data_home=${XDG_DATA_HOME:-"$home_dir/.local/share"}
config_home=${XDG_CONFIG_HOME:-"$home_dir/.config"}
installed_binary="$bin_dir/agent-instructions"

install -Dm755 "$binary_source" "$installed_binary"
python3 "$source_dir/scripts/install-assets.py" \
  --source "$source_dir" \
  --home "$home_dir" \
  --data-home "$data_home" \
  --config-home "$config_home" \
  --binary "$installed_binary"

application="$data_home/applications/agent-instructions-toggle.desktop"
autostart="$config_home/autostart/agent-instructions-tray.desktop"
if command -v desktop-file-validate >/dev/null 2>&1; then
  desktop-file-validate "$application"
  desktop-file-validate "$autostart"
fi

if [[ "$refresh_desktop" == true ]]; then
  if command -v kbuildsycoca6 >/dev/null 2>&1; then
    kbuildsycoca6
  elif command -v kbuildsycoca5 >/dev/null 2>&1; then
    kbuildsycoca5
  fi
fi

echo "Installed agent-instructions."
echo "The KDE shortcut is Meta+Ctrl+Shift+A. It affects new agent contexts only."
