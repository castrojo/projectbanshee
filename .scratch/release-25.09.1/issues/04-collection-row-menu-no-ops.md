# 04: Collection rows offer Play Now / Play Next / Open Artist that do nothing

**What to build:** The row menu for Result/QuickAdd rows always lists Play Now, Play Next and Open Artist (`src/ui/rows.rs`, menu model). For `RowItem::Result(SearchItem::Collection(_))` the actions are `=> {}` no-ops, and `item_track` returns None, so Open Artist does nothing either.

**Blocked by:** None

**Status:** ready-for-agent

**GitHub:** https://github.com/castrojo/projectbanshee/issues/4

- [x] `bind` disables `play-now`, `play-next` and `open-artist` while a Collection is bound, and re-enables them for tracks (rows are recycled)

## Comments

- 2026-09-27: Fixed. Broadway run with search "ok computer": the album row menu shows the three items disabled and Copy Link enabled; the "Airbag" track row shows all four enabled.
