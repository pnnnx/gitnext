# latest (A) → lsp-refined (B)

Paired: each function is measured by both versions, the difference B − A is taken per function, and the 90% interval comes from drawing the functions again (within each repo). `better` / `worse`: the whole interval is on one side of zero. `can't tell`: it covers zero. `same`: no function changed. `< 0.5 pt`: the whole interval is within half a point. `changed`: functions whose value differs — a difference rests on them alone, read them below. `± alone`: the noise of one version on its own (as in latest.md): two separate runs would need a difference that large.

Not paired:
- click: only in A (or failed in the other run) — left out
- json-java: only in A (or failed in the other run) — left out
- jsoup: only in A (or failed in the other run) — left out
- sqlparse: only in A (or failed in the other run) — left out
- yaml: only in A (or failed in the other run) — left out
- zustand: only in A (or failed in the other run) — left out

## All repos

| metric | functions | changed | A | B | B − A, points | 90% interval | ± alone | verdict |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| callers recall | 108 | 0 | 99.7% | 99.7% | +0.0 | +0.0 … +0.0 | ±0.3 | **same** |
| callers precision | 108 | 0 | 98.8% | 98.8% | +0.0 | +0.0 … +0.0 | ±1.2 | **same** |
| callers recall, picked blind | 66 | 0 | 99.9% | 99.9% | +0.0 | +0.0 … +0.0 | ±0.2 | **same** |
| shared names: recall | 42 | 9 | 78.0% | 96.4% | +18.5 | +9.5 … +27.4 | ±10.1 | **better** |
| shared names: recall with maybe | 42 | 0 | 100.0% | 100.0% | +0.0 | +0.0 … +0.0 | ±0.0 | **same** |
| shared names: precision | 34 | 1 | 99.4% | 100.0% | +0.6 | +0.0 … +1.8 | ±0.9 | **can't tell** |
| rename ok (tests pass) | 119 | 1 | 90.8% | 91.6% | +0.8 | +0.0 … +2.5 | ±4.2 | **can't tell** |
| tests recall | 59 | 11 | 69.4% | 77.9% | +8.5 | +3.4 … +13.9 | ±7.9 | **better** |
| tests precision | 59 | 9 | 45.7% | 51.1% | +5.4 | +1.5 … +9.8 | ±8.2 | **better** |
| top-10 that fail | 59 | 8 | 50.2% | 55.9% | +5.7 | +1.9 … +10.1 | ±8.4 | **better** |
| … best possible order | 59 | 8 | 63.6% | 68.3% | +4.7 | +0.4 … +9.5 | ±8.3 | **better** |
| caught by the top 10 | 59 | 4 | 79.7% | 86.4% | +6.8 | +1.7 … +11.9 | ±8.5 | **better** |
| look / file | 119 | 33 | 13.5% | 14.0% | +0.5 | +0.2 … +0.9 | ±3.1 | **better** |

## go

| metric | functions | changed | A | B | B − A, points | 90% interval | ± alone | verdict |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| callers recall | 71 | 0 | 100.0% | 100.0% | +0.0 | +0.0 … +0.0 | ±0.0 | **same** |
| callers precision | 71 | 0 | 98.2% | 98.2% | +0.0 | +0.0 … +0.0 | ±1.8 | **same** |
| callers recall, picked blind | 40 | 0 | 100.0% | 100.0% | +0.0 | +0.0 … +0.0 | ±0.0 | **same** |
| shared names: recall | 11 | 3 | 72.7% | 100.0% | +27.3 | +9.1 … +45.5 | ±22.7 | **better** |
| shared names: recall with maybe | 11 | 0 | 100.0% | 100.0% | +0.0 | +0.0 … +0.0 | ±0.0 | **same** |
| shared names: precision | 8 | 0 | 100.0% | 100.0% | +0.0 | +0.0 … +0.0 | ±0.0 | **same** |
| rename ok (tests pass) | 81 | 0 | 92.6% | 92.6% | +0.0 | +0.0 … +0.0 | ±4.9 | **same** |
| tests recall | 37 | 2 | 79.3% | 78.2% | -1.1 | -3.8 … +0.5 | ±9.2 | **can't tell** |
| tests precision | 37 | 1 | 47.1% | 46.7% | -0.4 | -1.3 … +0.0 | ±10.1 | **can't tell** |
| top-10 that fail | 37 | 1 | 54.0% | 53.7% | -0.3 | -0.8 … +0.0 | ±10.4 | **can't tell** |
| … best possible order | 37 | 1 | 75.3% | 73.4% | -1.9 | -5.7 … +0.0 | ±9.3 | **can't tell** |
| caught by the top 10 | 37 | 0 | 83.8% | 83.8% | +0.0 | +0.0 … +0.0 | ±10.8 | **same** |
| look / file | 81 | 9 | 13.5% | 13.7% | +0.2 | +0.0 … +0.4 | ±3.7 | **< 0.5 pt** |

## rust

| metric | functions | changed | A | B | B − A, points | 90% interval | ± alone | verdict |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| callers recall | 37 | 0 | 99.3% | 99.3% | +0.0 | +0.0 … +0.0 | ±0.9 | **same** |
| callers precision | 37 | 0 | 100.0% | 100.0% | +0.0 | +0.0 … +0.0 | ±0.0 | **same** |
| callers recall, picked blind | 26 | 0 | 99.7% | 99.7% | +0.0 | +0.0 … +0.0 | ±0.4 | **same** |
| shared names: recall | 31 | 6 | 79.8% | 95.2% | +15.3 | +5.6 … +25.8 | ±10.5 | **better** |
| shared names: recall with maybe | 31 | 0 | 100.0% | 100.0% | +0.0 | +0.0 … +0.0 | ±0.0 | **same** |
| shared names: precision | 26 | 1 | 99.2% | 100.0% | +0.8 | +0.0 … +2.3 | ±1.2 | **can't tell** |
| rename ok (tests pass) | 38 | 1 | 86.8% | 89.5% | +2.6 | +0.0 … +7.9 | ±9.2 | **can't tell** |
| tests recall | 22 | 9 | 52.8% | 77.4% | +24.6 | +11.8 … +38.1 | ±14.0 | **better** |
| tests precision | 22 | 8 | 43.4% | 58.6% | +15.2 | +4.9 … +26.2 | ±13.2 | **better** |
| top-10 that fail | 22 | 7 | 43.8% | 59.6% | +15.8 | +5.7 … +26.8 | ±13.4 | **better** |
| … best possible order | 22 | 7 | 43.8% | 59.6% | +15.8 | +5.7 … +26.8 | ±13.4 | **better** |
| caught by the top 10 | 22 | 4 | 72.7% | 90.9% | +18.2 | +4.5 … +31.8 | ±15.9 | **better** |
| look / file | 38 | 24 | 13.6% | 14.8% | +1.2 | +0.4 … +2.2 | ±5.2 | **better** |

## Per repo (metrics where a function changed)

| repo | metric | functions | changed | A | B | B − A | 90% interval | verdict |
|---|---|---:|---:|---:|---:|---:|---:|---|
| chi | shared names: recall | 10 | 3 | 70.0% | 100.0% | +30.0 | +10.0 … +50.0 | better |
| chi | tests recall | 16 | 2 | 66.5% | 64.0% | -2.6 | -8.8 … +1.1 | can't tell |
| chi | tests precision | 16 | 1 | 61.4% | 60.4% | -1.0 | -3.0 … +0.0 | can't tell |
| chi | top-10 that fail | 16 | 1 | 66.7% | 66.1% | -0.6 | -1.9 … +0.0 | can't tell |
| chi | … best possible order | 16 | 1 | 71.1% | 66.7% | -4.4 | -13.1 … +0.0 | can't tell |
| chi | look / file | 35 | 6 | 21.9% | 22.3% | +0.4 | -0.0 … +1.0 | can't tell |
| cobra | look / file | 46 | 3 | 7.1% | 7.2% | +0.0 | +0.0 … +0.1 | < 0.5 pt |
| indexmap | shared names: recall | 29 | 6 | 78.4% | 94.8% | +16.4 | +6.0 … +27.6 | better |
| indexmap | shared names: precision | 24 | 1 | 99.2% | 100.0% | +0.8 | +0.0 … +2.5 | can't tell |
| indexmap | rename ok (tests pass) | 33 | 1 | 84.8% | 87.9% | +3.0 | +0.0 … +9.1 | can't tell |
| indexmap | tests recall | 19 | 9 | 51.5% | 80.0% | +28.5 | +13.7 … +44.1 | better |
| indexmap | tests precision | 19 | 8 | 40.2% | 57.7% | +17.6 | +5.7 … +30.3 | better |
| indexmap | top-10 that fail | 19 | 7 | 40.2% | 58.5% | +18.3 | +6.9 … +31.0 | better |
| indexmap | … best possible order | 19 | 7 | 40.2% | 58.5% | +18.3 | +6.9 … +31.0 | better |
| indexmap | caught by the top 10 | 19 | 4 | 73.7% | 94.7% | +21.1 | +5.3 … +36.8 | better |
| indexmap | look / file | 33 | 21 | 13.4% | 14.7% | +1.3 | +0.4 … +2.5 | better |
| semver | look / file | 5 | 3 | 15.0% | 15.2% | +0.2 | -0.0 … +0.4 | < 0.5 pt |

## Functions that changed (up to 10 per metric, largest first)

- **shared names: recall** (9): chi `main::Context::Reset` 0% → 100%; chi `main::Mux::MethodNotAllowed` 0% → 100%; chi `main::Mux::NotFound` 0% → 100%; indexmap `indexmap::map::slice::Slice::partition_point` 0% → 100%; indexmap `indexmap::set::slice::Slice::is_sorted` 0% → 100%; indexmap `indexmap::map::raw_entry_v1::RawOccupiedEntryMut::get_key_value_mut` 0% → 100%; indexmap `indexmap::set::slice::Slice::is_sorted_by_key` 0% → 100%; indexmap `indexmap::inner::Core::drain` 50% → 100%; indexmap `indexmap::map::slice::Slice::binary_search_by` 75% → 100%
- **shared names: precision** (1): indexmap `indexmap::map::slice::Slice::from_boxed` 80% → 100%
- **rename ok (tests pass)** (1): indexmap `indexmap::map::IndexMap::get_index_entry` 0% → 100%
- **tests recall** (11): indexmap `indexmap::util::simplify_range` 0% → 100%; indexmap `indexmap::inner::Core::shift_insert_unique` 0% → 100%; indexmap `indexmap::util::third` 0% → 100%; indexmap `indexmap::map::raw_entry_v1::RawVacantEntryMut::shift_insert_hashed_nocheck` 0% → 100%; indexmap `indexmap::map::IndexMap::contains_key` 27% → 80%; chi `main::Mux::NotFoundHandler` 64% → 14%; indexmap `indexmap::inner::Core::replace_index_unique` 50% → 100%; indexmap `indexmap::inner::Core::retain_in_order` 50% → 100%; indexmap `indexmap::set::IndexSet::contains` 60% → 40%; chi `main::Chain` 18% → 27%
- **tests precision** (9): indexmap `indexmap::util::simplify_range` 0% → 100%; indexmap `indexmap::util::third` 0% → 100%; indexmap `indexmap::inner::Core::shift_insert_unique` 0% → 67%; indexmap `indexmap::map::raw_entry_v1::RawVacantEntryMut::shift_insert_hashed_nocheck` 0% → 50%; indexmap `indexmap::set::IndexSet::swap_take` 25% → 50%; indexmap `indexmap::inner::Core::retain_in_order` 50% → 67%; chi `main::Mux::NotFoundHandler` 21% → 5%; indexmap `indexmap::map::IndexMap::contains_key` 100% → 86%; indexmap `indexmap::set::IndexSet::contains` 60% → 50%
- **top-10 that fail** (8): indexmap `indexmap::util::simplify_range` 0% → 100%; indexmap `indexmap::util::third` 0% → 100%; indexmap `indexmap::inner::Core::shift_insert_unique` 0% → 67%; indexmap `indexmap::map::raw_entry_v1::RawVacantEntryMut::shift_insert_hashed_nocheck` 0% → 50%; indexmap `indexmap::set::IndexSet::swap_take` 25% → 50%; indexmap `indexmap::inner::Core::retain_in_order` 50% → 67%; chi `main::Mux::NotFoundHandler` 30% → 20%; indexmap `indexmap::set::IndexSet::contains` 60% → 50%
- **… best possible order** (8): indexmap `indexmap::util::simplify_range` 0% → 100%; indexmap `indexmap::util::third` 0% → 100%; chi `main::Mux::NotFoundHandler` 90% → 20%; indexmap `indexmap::inner::Core::shift_insert_unique` 0% → 67%; indexmap `indexmap::map::raw_entry_v1::RawVacantEntryMut::shift_insert_hashed_nocheck` 0% → 50%; indexmap `indexmap::set::IndexSet::swap_take` 25% → 50%; indexmap `indexmap::inner::Core::retain_in_order` 50% → 67%; indexmap `indexmap::set::IndexSet::contains` 60% → 50%
- **caught by the top 10** (4): indexmap `indexmap::util::simplify_range` 0% → 100%; indexmap `indexmap::inner::Core::shift_insert_unique` 0% → 100%; indexmap `indexmap::util::third` 0% → 100%; indexmap `indexmap::map::raw_entry_v1::RawVacantEntryMut::shift_insert_hashed_nocheck` 0% → 100%
- **look / file** (33): indexmap `indexmap::util::third` 14% → 30%; indexmap `indexmap::util::simplify_range` 50% → 60%; indexmap `indexmap::util::assert_index_lt` 41% → 51%; chi `main::Middlewares::HandlerFunc` 38% → 48%; chi `main::Chain` 38% → 43%; indexmap `indexmap::util::assert_index_le` 40% → 43%; indexmap `indexmap::inner::Core::shift_insert_unique` 7% → 9%; indexmap `indexmap::util::try_simplify_range` 48% → 49%; cobra `doc::GenYamlCustom` 43% → 44%; indexmap `indexmap::inner::Core::replace_index_unique` 5% → 6%
