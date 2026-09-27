# 04: Artwork Store with byte-bounded LRU + disk cap + periodic GC

**What to build:** Artwork Store with byte-bounded LRU + disk cap + periodic GC, as described in `../spec.md`.

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] Memory and disk budgets enforced, malloc_trim
- [ ] Tests: eviction and memory stability

## Comments

- Implemented in the Rust rewrite (2026-09-26); see final report.
