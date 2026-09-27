#!/usr/bin/env bash
# Run a command inside the GNOME 50 SDK with the Rust extension, sharing the
# session so the app can reach Wayland, PipeWire/Pulse, D-Bus (MPRIS) and the network.
# Usage: build-aux/sdk-run.sh cargo test
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
extra_path=""
if [ -d /home/linuxbrew/.linuxbrew/bin ]; then extra_path=":/home/linuxbrew/.linuxbrew/bin"; fi
exec flatpak run --devel --share=network --share=ipc --socket=wayland --socket=fallback-x11 \
  --socket=pulseaudio --socket=session-bus --device=dri --filesystem=home \
  --filesystem=/home/linuxbrew:ro --filesystem=xdg-run/app/com.discordapp.Discord:ro \
  --env=CARGO_HOME="$HOME/.cargo" --env=CARGO_TARGET_DIR="$here/target" \
  --env=RUST_LOG="${RUST_LOG:-banshee=info}" \
  --cwd="$here" --command=bash org.gnome.Sdk//50 \
  -c "source /usr/lib/sdk/rust-stable/enable.sh; export PATH=\"\$PATH$extra_path\"; exec \"\$@\"" bash "$@"
