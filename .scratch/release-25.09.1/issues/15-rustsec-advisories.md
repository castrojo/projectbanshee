# 15: RustSec advisories in the dependency tree

**What to build:** `cargo audit` (v0.22.2, 1271 advisories) reports 4:
- quick-xml 0.38.4: RUSTSEC-2026-0194 and RUSTSEC-2026-0195 (7.5, DoS; fixed in >=0.41.0), pulled in by librespot-core 0.8.0 (latest on crates.io)
- rsa 0.9.10: RUSTSEC-2023-0071 (Marvin; no fix available), from librespot-core
- time 0.3.45: RUSTSEC-2026-0009 (stack-exhaustion DoS; fixed in >=0.3.47), from librespot-core and vergen. `cargo update -p time` locks nothing under the declared MSRV 1.85; with `--ignore-rust-version` it moves time to 0.3.55 plus num-conv, time-core and time-macros.

Bumping needs a lockfile change and `build-aux/cargo-sources.json` regenerated with flatpak-cargo-generator (not installed here), and quick-xml needs a librespot release. All exposure goes through librespot parsing Spotify responses over TLS.

**Blocked by:** 16 (for time)

**Status:** ready-for-human

**GitHub:** https://github.com/castrojo/projectbanshee/issues/15

- [ ] Bump time to >=0.3.47 and regenerate cargo-sources.json
- [ ] Track librespot for quick-xml >=0.41 and an rsa replacement

## Comments

