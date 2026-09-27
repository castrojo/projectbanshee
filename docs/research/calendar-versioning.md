# CalVer (`YY.MM.patch`) for Banshee: research notes

Checked 2026-09-27. Links are pinned to commits or tags where possible. "Verified locally" means I ran the tool myself, using `org.gnome.Sdk//50` with `org.freedesktop.Sdk.Extension.rust-stable` (cargo 1.98.1, appstreamcli 1.0.6).

## Recommended form for each file

| Surface | Value | Why |
|---|---|---|
| `Cargo.toml` `version` | `25.9.1` | Cargo requires SemVer and rejects leading zeros (§2). |
| `Cargo.lock` | `25.9.1`, regenerated | Follows from `Cargo.toml`. |
| User-facing string (`AdwAboutDialog::version`, and `User-Agent` if you want the same string) | `25.09.1` | Re-pad the minor field at build time, the way Helix does (§2). |
| `data/*.metainfo.xml` `<release version=…>` | `25.09.1`, as the first entry above `0.2.2` | Passes validation and sorts newer than 0.x (§3). |
| Git tag / GitHub release | `25.09.1` (or `v25.09.1`, matching the existing tag style) | freedesktop-sdk and Helix keep the zero pad in tags (§1, §2). |
| Flatpak manifest (`io.github.castrojo.Banshee.yaml`) | no change | The manifest has no app version field. `flatpak info` reads the version from composed AppStream (§4). |

Side note: freedesktop-sdk's `YY.MM` is the date the branch was cut, not the date of the patch release. For example, `freedesktop-sdk-25.08.17` shipped on 2026-09-14 (§1). If Banshee's `25.09` is meant as "release month", September 2026 would be `26.09`. Decide which meaning `YY.MM` has before you tag.

---

## 1. freedesktop-sdk: where the version is defined and how it is split

**Branch (ABI series) = `YY.MM`, defined in one YAML include.**
- `include/repo_branches.yml` on `release/25.08` defines `freedesktop-sdk-flatpak-branch: '25.08'`, `…-branch-extra: '25.08-extra'` and `freedesktop-sdk-snap-branch: '2508'`: https://gitlab.com/freedesktop-sdk/freedesktop-sdk/-/blob/freedesktop-sdk-25.08.17/include/repo_branches.yml#L4-6. On master it is `'27.08beta'`: https://gitlab.com/freedesktop-sdk/freedesktop-sdk/-/blob/master/include/repo_branches.yml
- `project.conf` includes that file and maps it to the BuildStream variables `branch` and `branch-extra`, which become the Flatpak ref branch: https://gitlab.com/freedesktop-sdk/freedesktop-sdk/-/blob/freedesktop-sdk-25.08.17/project.conf#L21-25
- `os-release` uses only the branch: `VERSION_ID=__RUNTIME_BRANCH__`, filled in by m4 from `%{freedesktop-sdk-flatpak-branch}`.
  - https://gitlab.com/freedesktop-sdk/freedesktop-sdk/-/blob/freedesktop-sdk-25.08.17/files/os-release/os-release.in#L2-5
  - https://gitlab.com/freedesktop-sdk/freedesktop-sdk/-/blob/freedesktop-sdk-25.08.17/elements/components/os-release.bst#L15-19

**Release (patch) version = `freedesktop-sdk-YY.MM.N`. It lives in git tags and `NEWS.yml`, not in any config variable.**
- Tags: `freedesktop-sdk-25.08.8` through `freedesktop-sdk-25.08.17`, taken from the GitLab tags API (`/projects/freedesktop-sdk%2Ffreedesktop-sdk/repository/tags`). `25.08.17` was tagged 2026-09-15 and `25.08.16` on 2026-08-16: https://gitlab.com/freedesktop-sdk/freedesktop-sdk/-/tags
- `NEWS.yml` has one YAML document per release, e.g. `Version: freedesktop-sdk-25.08.17` / `Date: '2026-09-14'`: https://gitlab.com/freedesktop-sdk/freedesktop-sdk/-/blob/freedesktop-sdk-25.08.17/NEWS.yml#L1-4
- Release process: `./utils/release.py prepare release/23.08 freedesktop-sdk-23.08.1` writes the NEWS entry, then `release.py publish` pushes a signed tag: https://gitlab.com/freedesktop-sdk/freedesktop-sdk/-/blob/freedesktop-sdk-25.08.17/RELEASE.md#L3-39
- `release.py` enforces the zero-padded month with the regex `^freedesktop-sdk-\d{2}\.08(?:beta|rc)?\.\d+(?:\.\d+)?$`. It also requires the tag to start with the branch name (`input_version.startswith(branch_version)`), and orders releases with Python `packaging.version.Version`, which compares numerically.
  - https://gitlab.com/freedesktop-sdk/freedesktop-sdk/-/blob/freedesktop-sdk-25.08.17/utils/release.py#L26-32
  - https://gitlab.com/freedesktop-sdk/freedesktop-sdk/-/blob/freedesktop-sdk-25.08.17/utils/release.py#L97-105
  - https://gitlab.com/freedesktop-sdk/freedesktop-sdk/-/blob/freedesktop-sdk-25.08.17/utils/release.py#L139-147

**The runtime's AppStream `<release>` comes from `NEWS.yml`.**
- The source metainfo `files/os-release/org.freedesktop.Platform.metainfo.xml` has no `<releases>` block: https://gitlab.com/freedesktop-sdk/freedesktop-sdk/-/blob/freedesktop-sdk-25.08.17/files/os-release/org.freedesktop.Platform.metainfo.xml
- `os-release.bst` runs `appstreamcli news-to-metainfo --limit 1 NEWS.yml "${metainfo}"`, then `appstreamcli validate` and `appstreamcli compose`: https://gitlab.com/freedesktop-sdk/freedesktop-sdk/-/blob/freedesktop-sdk-25.08.17/elements/components/os-release.bst#L20-38
- Verified locally against installed runtimes:
  - `org.freedesktop.Platform/x86_64/25.08/active/files/share/metainfo/org.freedesktop.Platform.metainfo.xml` contains `<release type="stable" version="freedesktop-sdk-25.08.17" date="2026-09-14T00:00:00Z">`. The 26.08 runtime contains `version="freedesktop-sdk-26.08.1"`.
  - `flatpak info org.freedesktop.Platform//25.08` prints `Branch: 25.08` and `Version: freedesktop-sdk-25.08.17`.

**Conclusion:** the zero-padded month is kept everywhere: branch `25.08`, snap branch `2508`, tags, `NEWS.yml`, AppStream `release version`, and `flatpak info`. The AppStream version keeps the `freedesktop-sdk-` prefix. Its leading non-digit part is identical across releases, so vercmp still orders them correctly (§3). freedesktop-sdk has no Cargo.toml, so the leading-zero problem never comes up for it.

## 2. Rust / Cargo with zero-padded CalVer

**The rules**
- SemVer §2: "X, Y, and Z are non-negative integers, and MUST NOT contain leading zeroes."
  - https://semver.org/#spec-item-2
  - source: https://github.com/semver/semver/blob/master/semver.md?plain=1#L62-65
- Cargo reference: "The `version` field is formatted according to the SemVer specification: Versions must have three numeric parts…": https://doc.rust-lang.org/cargo/reference/manifest.html#the-version-field
- Verified locally: with `version="25.09.1"`, `cargo run` fails with `error: invalid leading zero in minor version number --> Cargo.toml:3:9`. With `version="25.9.1"` it builds, and `env!("CARGO_PKG_VERSION")` prints `25.9.1`.
- Python works the same way. PEP 440 "Integer Normalization": "an integer version of `00` would normalize to `0`": https://peps.python.org/pep-0440/#integer-normalization
  - yt-dlp's source has `__version__ = '2026.08.19'` (https://github.com/yt-dlp/yt-dlp/blob/master/yt_dlp/version.py#L3).
  - PyPI serves it as `2026.8.19` (https://pypi.org/pypi/yt-dlp/json → `info.version`).

**Closest match for Banshee: Helix (Rust, no meson, `YY.0M(.MICRO)`)**
- Policy, from `docs/releases.md`: "Cargo only accepts SemVer versions so a CalVer version of `22.07` for example must be formatted as `22.7.0`… A patch release for 22.07 would be `22.7.1`." https://github.com/helix-editor/helix/blob/079a789e8cb08ead67f19e1971a1b7438b37354b/docs/releases.md?plain=1#L3-16
- `Cargo.toml` has `[workspace.package] version = "25.7.1"`: https://github.com/helix-editor/helix/blob/079a789e8cb08ead67f19e1971a1b7438b37354b/Cargo.toml#L66-67
- `helix-loader/build.rs` rebuilds the display string from `CARGO_PKG_VERSION_{MAJOR,MINOR,PATCH}`:
  - it zero-pads a single-digit minor ("Print single-digit months in '0M' format")
  - it leaves out `.0` patches
  - it exports the result as `cargo:rustc-env=VERSION_AND_GIT_HASH`
  - https://github.com/helix-editor/helix/blob/079a789e8cb08ead67f19e1971a1b7438b37354b/helix-loader/build.rs#L5-39
  - consumed at https://github.com/helix-editor/helix/blob/079a789e8cb08ead67f19e1971a1b7438b37354b/helix-loader/src/lib.rs#L10
- Tags and releases keep the pad: `25.07.1`, `25.07`, `25.01.1`, `24.07` (https://github.com/helix-editor/helix/releases).
- AppStream keeps the pad: `<release version="25.07.1" …>` … `<release version="22.03" …>`: https://github.com/helix-editor/helix/blob/079a789e8cb08ead67f19e1971a1b7438b37354b/contrib/Helix.appdata.xml#L50-86

**GNOME Rust apps built with meson: meson is the user-facing source, Cargo is normalised or ignored**

These are default-branch HEADs as of 2026-09-27.

| App | meson `version:` | Cargo.toml `version` | VERSION path |
|---|---|---|---|
| Fractal @e97ffc53 | `'14.1'` ([meson.build#L4](https://gitlab.gnome.org/World/fractal/-/blob/e97ffc533583d727e4fcdf8404013124a01e9be5/meson.build#L4)) | `"14.1.0"` ([Cargo.toml#L3](https://gitlab.gnome.org/World/fractal/-/blob/e97ffc533583d727e4fcdf8404013124a01e9be5/Cargo.toml#L3)) | `set_quoted('VERSION', full_version)` ([src/meson.build#L74](https://gitlab.gnome.org/World/fractal/-/blob/e97ffc533583d727e4fcdf8404013124a01e9be5/src/meson.build#L74)) → `config.rs.in` `VERSION = @VERSION@` ([#L11](https://gitlab.gnome.org/World/fractal/-/blob/e97ffc533583d727e4fcdf8404013124a01e9be5/src/config.rs.in#L11)) → `.version(config::VERSION)` ([application.rs#L249](https://gitlab.gnome.org/World/fractal/-/blob/e97ffc533583d727e4fcdf8404013124a01e9be5/src/application.rs#L249)) |
| Loupe @228003cb | `'51.0'` ([#L4](https://gitlab.gnome.org/GNOME/loupe/-/blob/228003cb8755098bcb9859b3c8f2ded618633d47/meson.build#L4)) | `"51.0.0"` ([#L3](https://gitlab.gnome.org/GNOME/loupe/-/blob/228003cb8755098bcb9859b3c8f2ded618633d47/Cargo.toml#L3)) | `version = meson.project_version() + version_suffix` ([meson.build#L41](https://gitlab.gnome.org/GNOME/loupe/-/blob/228003cb8755098bcb9859b3c8f2ded618633d47/meson.build#L41)) → env `VERSION` ([src/meson.build#L21](https://gitlab.gnome.org/GNOME/loupe/-/blob/228003cb8755098bcb9859b3c8f2ded618633d47/src/meson.build#L21)) → `option_env!("VERSION")` ([config.rs#L19](https://gitlab.gnome.org/GNOME/loupe/-/blob/228003cb8755098bcb9859b3c8f2ded618633d47/src/config.rs#L19)) → `.version(config::VERSION)` ([about.rs#L34](https://gitlab.gnome.org/GNOME/loupe/-/blob/228003cb8755098bcb9859b3c8f2ded618633d47/src/about.rs#L34)) |
| Snapshot @77eb1674 | `'51.0'` ([#L4](https://gitlab.gnome.org/GNOME/snapshot/-/blob/77eb167485ef94512c1f3fc58cda1433f698e38d/meson.build#L4)) | `"46.0.0"`, stale ([#L21](https://gitlab.gnome.org/GNOME/snapshot/-/blob/77eb167485ef94512c1f3fc58cda1433f698e38d/Cargo.toml#L21)) | `config.rs.in` `@VERSION@` ([#L8](https://gitlab.gnome.org/GNOME/snapshot/-/blob/77eb167485ef94512c1f3fc58cda1433f698e38d/src/config.rs.in#L8)) |
| Amberol @3830fbb3 (CalVer `YYYY.N`) | `'2026.2'` ([#L5](https://gitlab.gnome.org/World/amberol/-/blob/3830fbb311088543d33e1f4145cea75d4def23d9/meson.build#L5)) | `"0.1.0"`, placeholder ([#L7](https://gitlab.gnome.org/World/amberol/-/blob/3830fbb311088543d33e1f4145cea75d4def23d9/Cargo.toml#L7)) | `set_quoted('VERSION', …project_version()…)` ([src/meson.build#L17](https://gitlab.gnome.org/World/amberol/-/blob/3830fbb311088543d33e1f4145cea75d4def23d9/src/meson.build#L17)) → `.version(VERSION)` ([application.rs#L236](https://gitlab.gnome.org/World/amberol/-/blob/3830fbb311088543d33e1f4145cea75d4def23d9/src/application.rs#L236)) |
| Shortwave @da98607c | `'5.1.0'` ([#L2](https://gitlab.gnome.org/World/Shortwave/-/blob/da98607c4495a3a0cece2a60a30ec89955c0529f/meson.build#L2)) | `"0.0.0"`, placeholder ([#L3](https://gitlab.gnome.org/World/Shortwave/-/blob/da98607c4495a3a0cece2a60a30ec89955c0529f/Cargo.toml#L3)) | `option_env!("MESON_VERSION")` via `config_var!(VERSION)` ([config.rs#L10,L49](https://gitlab.gnome.org/World/Shortwave/-/blob/da98607c4495a3a0cece2a60a30ec89955c0529f/src/config.rs#L49)) |
| Podcasts @a6f1ea2f (CalVer `YY.N`) | `'25.4'` ([#L3](https://gitlab.gnome.org/World/podcasts/-/blob/a6f1ea2f491c5da2e603103760eee02128ccbe01/meson.build#L3)) | `"0.1.0"`, placeholder ([podcasts-gtk/Cargo.toml#L4](https://gitlab.gnome.org/World/podcasts/-/blob/a6f1ea2f491c5da2e603103760eee02128ccbe01/podcasts-gtk/Cargo.toml#L4)) | `config.rs.in` `@VERSION@` ([#L22](https://gitlab.gnome.org/World/podcasts/-/blob/a6f1ea2f491c5da2e603103760eee02128ccbe01/podcasts-gtk/src/config.rs.in#L22)) |

None of these GNOME apps uses a zero-padded month, but they all separate "Cargo version" from "displayed version". Where they fill in Cargo properly, they use its normalised SemVer form (`14.1` → `14.1.0`, `51.0` → `51.0.0`). The others use a placeholder (`0.1.0`, `0.0.0`) or let it go stale (Snapshot `46.0.0` vs `51.0`).

**Prevailing practice:** Cargo.toml carries the SemVer-normalised form, and the user-facing string is supplied separately. Meson apps pass it through `config.rs`. Helix, which has no meson, uses `build.rs` to re-pad from `CARGO_PKG_VERSION_*`. Banshee has no meson either, so the Helix pattern is the direct precedent. It means one `build.rs` that emits e.g. `cargo:rustc-env=BANSHEE_VERSION=25.09.1`, plus `main.rs:135` `.version(env!("BANSHEE_VERSION"))`. Cargo's `+build` metadata (`25.9.1+…`) would push the suffix into `CARGO_PKG_VERSION`, and none of the projects surveyed do that.

## 3. AppStream: does `<release version="25.09.1">` validate and sort above `0.2.2`?

**Spec**
- "The `release` children must be sorted in a latest-to-oldest order", and "The algorithm used for comparing release version numbers is described at sect-AppStream-Misc-VerCmp."
  - https://www.freedesktop.org/software/appstream/docs/sect-Metadata-Releases.html
  - source: https://github.com/ximion/appstream/blob/f981c6c558da6dad2d507a8bbabee187793f23a1/docs/xml/releases-data.xml#L81-93
- Version comparison is dpkg/rpm style. Non-digit parts are compared lexically. Digit parts are compared by numeric value, and an empty part counts as zero.
  - https://www.freedesktop.org/software/appstream/docs/sect-AppStream-Misc-VerCmp.html
  - source: https://github.com/ximion/appstream/blob/f981c6c558da6dad2d507a8bbabee187793f23a1/docs/xml/misc-vercmp.xml#L24-40
- The spec recommends SemVer and starting with a digit, but does not require either: [misc-vercmp.xml#L43-60](https://github.com/ximion/appstream/blob/f981c6c558da6dad2d507a8bbabee187793f23a1/docs/xml/misc-vercmp.xml#L43-60)
- Implementation: `cmp_number()` strips leading `0`s before comparing digit runs (`for (; *a == '0'; a++)`), so `09` equals `9`: https://github.com/ximion/appstream/blob/f981c6c558da6dad2d507a8bbabee187793f23a1/src/as-vercmp.c#L80-106

**Verified locally (appstreamcli 1.0.6)**
- `vercmp` results:
  - `25.09.1 >> 0.2.2`
  - `25.09.1 == 25.9.1`
  - `25.09.1 << 25.10`
  - `25.09.1 >> 25.09`
  - `25.09.1 << 25.09.10`
- `appstreamcli validate --no-net` on a desktop-application metainfo with `<release version="25.09.1"/>` above `<release version="0.2.2"/>`: "✔ Validation was successful" (only an unrelated description-length info).
- With the order reversed, validation fails: `W: releases-not-in-order 0.2.2 << 25.09.1`, exit 3.

**Precedent for moving from 0.x to CalVer in AppStream:**
- Amberol lists `2024.1` directly above `0.10.3`: https://gitlab.gnome.org/World/amberol/-/blob/3830fbb311088543d33e1f4145cea75d4def23d9/data/io.bassi.Amberol.metainfo.xml.in.in#L97-109
- Podcasts lists `25.2` directly above `0.7.2`: https://gitlab.gnome.org/World/podcasts/-/blob/a6f1ea2f491c5da2e603103760eee02128ccbe01/podcasts-gtk/resources/org.gnome.Podcasts.metainfo.xml.in.in#L50-62

**Caveat:** vercmp treats `25.09.1` and `25.9.1` as the same version. Choose one spelling for metainfo, preferably `25.09.1`, and never list both.

## 4. Flatpak: is there an app version field, and where does `flatpak info` get "Version"?

**flatpak-builder manifest (flatpak-manifest(5))**
- There is no application version property.
- Top-level `branch` and `default-branch` set the ref branch ("Defaults to master"). `runtime-version` is the runtime's branch. `version`/`versions` appear only as extension-point properties.
  - https://github.com/flatpak/flatpak-builder/blob/c2d9ea9c2f6ab193f54d54afcf69c2deb7291d34/doc/flatpak-manifest.xml#L56-81
  - https://github.com/flatpak/flatpak-builder/blob/c2d9ea9c2f6ab193f54d54afcf69c2deb7291d34/doc/flatpak-manifest.xml#L117-118
  - https://github.com/flatpak/flatpak-builder/blob/c2d9ea9c2f6ab193f54d54afcf69c2deb7291d34/doc/flatpak-manifest.xml#L495-507
  - rendered: https://docs.flatpak.org/en/latest/flatpak-builder-command-reference.html#flatpak-manifest
- `appstream-compose` (boolean, default true): "Run `appstreamcli compose` during cleanup phase": [flatpak-manifest.xml#L171-172](https://github.com/flatpak/flatpak-builder/blob/c2d9ea9c2f6ab193f54d54afcf69c2deb7291d34/doc/flatpak-manifest.xml#L171-172)
- `builder-manifest.c` runs `appstreamcli compose` with `--result-root=<app root>` and a data dir under `share/app-info/xmls`: https://github.com/flatpak/flatpak-builder/blob/c2d9ea9c2f6ab193f54d54afcf69c2deb7291d34/src/builder-manifest.c#L3101-3150

**flatpak (runtime side)**
- On deploy, `read_appdata_xml_from_deploy_dir()` reads `files/share/app-info/xmls/<id>.xml.gz`. `flatpak_parse_appdata()` then stores `appdata-version` in the deploy data.
  - https://github.com/flatpak/flatpak/blob/ddcd5c4ebb545a7a1e7225a96bd44256c61ac5cb/common/flatpak-dir.c#L4068-4090
  - https://github.com/flatpak/flatpak/blob/ddcd5c4ebb545a7a1e7225a96bd44256c61ac5cb/common/flatpak-dir.c#L4150-4170
- `flatpak info` prints `flatpak_deploy_data_get_appdata_version()` as "Version:".
  - https://github.com/flatpak/flatpak/blob/ddcd5c4ebb545a7a1e7225a96bd44256c61ac5cb/app/flatpak-builtins-info.c#L163
  - https://github.com/flatpak/flatpak/blob/ddcd5c4ebb545a7a1e7225a96bd44256c61ac5cb/app/flatpak-builtins-info.c#L264
- The chosen version belongs to the `<release>` with the **latest date/timestamp** (`if (ts > data->timestamp)`), not the highest vercmp: https://github.com/flatpak/flatpak/blob/ddcd5c4ebb545a7a1e7225a96bd44256c61ac5cb/common/flatpak-appdata.c#L146-195
  - Because the comparison is strict, when dates tie the first release listed wins. Banshee's `0.2.2` and a new `25.09.1` could both be dated 2026-09-27; `25.09.1` still wins as long as it is listed first, as the spec requires anyway.
- Verified locally: `flatpak info org.freedesktop.Platform//25.08` → `Version: freedesktop-sdk-25.08.17`, which is the runtime's AppStream release string (§1).

**Conclusion for Banshee:** `io.github.castrojo.Banshee.yaml` needs no version change. `flatpak list`/`flatpak info` will show whatever the newest-dated `<release version>` in `data/io.github.castrojo.Banshee.metainfo.xml` is. Put `25.09.1` there, and the Cargo form never reaches Flatpak.
