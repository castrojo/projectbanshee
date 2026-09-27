#!/usr/bin/env bash
# Build the Flatpak, export it with the host tools, bundle it and install it for this user.
#
# org.flatpak.Builder compiles the app, but on some systems its image loaders can't start
# their sandbox, which breaks appstream compose and export-time icon validation. So the
# builder only builds (no --repo), and the host `flatpak build-export` exports the finished
# tree. See .scratch/banshee-rust/issues/13-flatpak-bundle-compose.md.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
flatpak run org.flatpak.Builder --user --install-deps-from=flathub --force-clean \
  build io.github.castrojo.Banshee.yaml
flatpak build-export repo build
flatpak build-bundle repo io.github.castrojo.Banshee.flatpak io.github.castrojo.Banshee
flatpak install --user -y --reinstall ./io.github.castrojo.Banshee.flatpak
echo "Built and installed io.github.castrojo.Banshee.flatpak"
