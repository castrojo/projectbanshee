#!/usr/bin/env bash
# Build the Flatpak, export it with the host tools, bundle it and install it for this user.
#
# org.flatpak.Builder compiles the app, but on some systems its image loaders can't start
# their sandbox, which breaks appstream compose and export-time icon validation. So the
# builder only builds (no --repo), and the host `flatpak build-export` exports the finished
# tree. See .scratch/banshee-rust/issues/13-flatpak-bundle-compose.md.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

# Calendar version YY.MM.patch, as freedesktop-sdk (25.08.x). Cargo.toml holds the SemVer form
# without the month's leading zero (25.9.1); re-pad it like build.rs does, then require the
# metainfo's newest <release> to match. See docs/research/calendar-versioning.md.
cargo_version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n1)
IFS=. read -r yy mm patch <<<"$cargo_version"
version=$(printf '%s.%02d.%s' "$yy" "$mm" "$patch")
newest=$(grep -o -m1 '<release version="[^"]*"' data/io.github.castrojo.Banshee.metainfo.xml | cut -d'"' -f2)
[ "$newest" = "$version" ] ||
  { echo "metainfo newest release is $newest, not $version" >&2; exit 1; }

flatpak run org.flatpak.Builder --user --install-deps-from=flathub --force-clean \
  build io.github.castrojo.Banshee.yaml
flatpak build-export --subject="Banshee $version" repo build
flatpak build-bundle repo io.github.castrojo.Banshee.flatpak io.github.castrojo.Banshee
flatpak install --user -y --reinstall ./io.github.castrojo.Banshee.flatpak
echo "Built and installed io.github.castrojo.Banshee.flatpak $version"
