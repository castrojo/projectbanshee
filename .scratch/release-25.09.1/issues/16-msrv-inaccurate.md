# 16: Declared `rust-version = "1.85"` is below what the lockfile needs

**What to build:** `cargo update --dry-run --verbose` shows the locked gtk4 0.11.5, glib 0.22, gstreamer 0.25 and pango each "requires Rust 1.92" (confirmed via `cargo metadata` rust_version). Setting `rust-version = "1.92"` makes clippy's MSRV-gated `collapsible_if` let-chain lint fire in 7 places in untouched code, so the bump was reverted for this release.

**Blocked by:** None

**Status:** ready-for-human

**GitHub:** https://github.com/castrojo/projectbanshee/issues/16

- [ ] Set `rust-version = "1.92"` and apply the let-chain clippy fixes in one change

## Comments

