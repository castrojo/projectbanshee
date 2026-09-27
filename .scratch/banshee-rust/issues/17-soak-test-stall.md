# 17: Memory soak test stalled once under the full suite

**What to build:** `tests/memory_soak.rs::extended_session_keeps_memory_and_pipelines_bounded` hit its 240 s stall guard once (`soak stalled after N items`) during `cargo test --release` while a second Banshee instance was running on the same machine. It then passed 18 consecutive runs (3 + 15, full suite, ~2 s each). Find whether a Player Core transition can miss `Finished` under load (a real stall a user would hear as silence) or whether the test's own timers race.

**Blocked by:** None (can start immediately)

**Status:** needs-triage

- [ ] Reproduce under CPU load (e.g. `stress-ng --cpu 0` alongside a 50× loop of the soak test)
- [ ] If Player Core drops a transition, fix it and keep the soak test as the regression test

## Comments

- 2026-09-26: 40 further runs (20 pairs running concurrently) after the review fixes: 0 stalls. Still unexplained; keep open at needs-triage.
