# learn-tests на indexmap (rust): покрытие по тестам против `tests-for`
indexmap 41a870887c4c, gitnext 08e599c, seed 7. Истина — eval (тесты, падающие при panic в начале функции). pick — функции с ответом в кэше eval, random — случайные.

| выборка | функций | источник | полнота | точность | из 10 падают | поймано |
|---|---:|---|---:|---:|---:|---:|
| pick (кэш eval) | 19 | gitnext | 52% | 40% | 40% | 74% |
| pick (кэш eval) | 19 | coverage | 100% | 97% | 97% | 100% |
| pick (кэш eval) | 19 | union | 100% | 68% | 68% | 100% |
| случайные | 7 | gitnext | 53% | 35% | 35% | 86% |
| случайные | 7 | coverage | 100% | 100% | 100% | 100% |
| случайные | 7 | union | 100% | 61% | 61% | 100% |
| все | 26 | gitnext | 52% | 39% | 39% | 77% |
| все | 26 | coverage | 100% | 98% | 98% | 100% |
| все | 26 | union | 100% | 66% | 66% | 100% |

## По функциям

| выборка | функция | падают | граф: полн / из 10 | покрытие: полн / точн / из 10 | мимо покрытия |
|---|---|---:|---|---|---|
| pick | `indexmap::benches::bench::simple_sort` | 0 | no test fails when it throws | названо 0 | |
| pick | `indexmap::inner::Core::get_index_of_raw` | 0 | no test fails when it throws | названо 0 | |
| pick | `indexmap::inner::Core::replace_index_unique` | 2 | 50% / 50% | 100% / 100% / 100% |  |
| pick | `indexmap::inner::Core::retain_in_order` | 2 | 50% / 50% | 100% / 100% / 100% |  |
| pick | `indexmap::inner::Core::shift_insert_unique` | 6 | 0% / 0% | 100% / 75% / 75% |  |
| pick | `indexmap::inner::Core::split_splice` | 0 | no test fails when it throws | названо 0 | |
| pick | `indexmap::inner::extract::ExtractCore::remaining` | 2 | 0% / 0% | 100% / 100% / 100% |  |
| pick | `indexmap::map::IndexMap::contains_key` | 15 | 27% / 100% | 100% / 100% / 100% |  |
| pick | `indexmap::map::IndexMap::sort_keys` | 0 | no test fails when it throws | названо 0 | |
| pick | `indexmap::map::IndexMap::sort_unstable_keys` | 1 | 100% / 14% | 100% / 100% / 100% |  |
| pick | `indexmap::map::disjoint::get_disjoint_opt_mut` | 8 | 100% / 89% | 100% / 89% / 89% |  |
| pick | `indexmap::map::raw_entry_v1::RawEntryBuilder::index_from_hash` | 0 | no test fails when it throws | названо 0 | |
| pick | `indexmap::map::raw_entry_v1::RawOccupiedEntryMut::into_key_value_mut` | 0 | no test fails when it throws | названо 0 | |
| pick | `indexmap::map::raw_entry_v1::RawVacantEntryMut::insert_hashed_nocheck` | 0 | no test fails when it throws | названо 0 | |
| pick | `indexmap::map::raw_entry_v1::RawVacantEntryMut::shift_insert_hashed_nocheck` | 1 | 0% / 0% | 100% / 100% / 100% |  |
| pick | `indexmap::map::slice::Slice::from_mut_slice` | 13 | 92% / 100% | 100% / 100% / 100% |  |
| pick | `indexmap::rayon::set::IndexSet::par_difference` | 0 | no test fails when it throws | названо 0 | |
| pick | `indexmap::rayon::set::IndexSet::par_is_subset` | 0 | no test fails when it throws | названо 0 | |
| pick | `indexmap::serde::cautious_capacity` | 0 | no test fails when it throws | названо 0 | |
| pick | `indexmap::set::IndexSet::contains` | 5 | 60% / 60% | 100% / 100% / 100% |  |
| pick | `indexmap::set::IndexSet::difference` | 2 | 50% / 20% | 100% / 100% / 100% |  |
| pick | `indexmap::set::IndexSet::intersection` | 2 | 50% / 20% | 100% / 100% / 100% |  |
| pick | `indexmap::set::IndexSet::is_subset` | 4 | 50% / 40% | 100% / 100% / 100% |  |
| pick | `indexmap::set::IndexSet::swap_take` | 2 | 100% / 25% | 100% / 100% / 100% |  |
| pick | `indexmap::set::IndexSet::union` | 2 | 50% / 20% | 100% / 100% / 100% |  |
| pick | `indexmap::util::assert_index_le` | 6 | 100% / 75% | 100% / 75% / 75% |  |
| pick | `indexmap::util::simplify_range` | 6 | 0% / 0% | 100% / 100% / 100% |  |
| pick | `indexmap::util::third` | 11 | 0% / 0% | 100% / 100% / 100% |  |
| pick | `indexmap::util::try_simplify_range` | 3 | 100% / 100% | 100% / 100% / 100% |  |
| random | `indexmap::map::iter::IterMut::default` | 1 | 100% / 25% | 100% / 100% / 100% |  |
| random | `indexmap::rayon::set::IndexSet::par_sort_unstable_by_key` | 0 | no test fails when it throws | названо 0 | |
| random | `indexmap::map::slice::Slice::split_first` | 1 | 100% / 10% | 100% / 100% / 100% |  |
| random | `indexmap::inner::Core::move_index_inner` | 7 | 14% / 50% | 100% / 100% / 100% |  |
| random | `indexmap::inner::update_index` | 32 | 6% / 67% | 100% / 100% / 100% |  |
| random | `indexmap::rayon::map::Slice::par_values` | 0 | no test fails when it throws | названо 0 | |
| random | `indexmap::map::iter::ExtractIf::next` | 2 | 0% / 0% | 100% / 100% / 100% |  |
| random | `indexmap::map::IndexMap::shift_remove_entry` | 2 | 100% / 25% | 100% / 100% / 100% |  |
| random | `indexmap::rayon::map::Slice::par_values_mut` | 0 | no test fails when it throws | названо 0 | |
| random | `indexmap::set::iter::Difference::clone` | 0 | no test fails when it throws | названо 0 | |
| random | `indexmap::rayon::set::ParIntersection::fmt` | 0 | no test fails when it throws | названо 0 | |
| random | `indexmap::inner::Core::shrink_to` | 4 | 50% / 67% | 100% / 100% / 100% |  |

## Цена

- `build_s`: 12.0
- `run_one_by_one_s`: 4.1
- `export_s`: 9.8
- `map_s`: 25.9
- `map_without_build_s`: 13.9
- `profiles_bytes`: 40501624
- `test_binaries`: 5
- `files_with_doctests`: 7
- `repeat`: 3
- `plain_s`: 2.1
- `tests`: 183
- `map_pairs`: 5003
- `map_functions`: 381
- `map_sqlite_bytes`: 69632
- `functions_with_body`: 739
- `functions_never_run`: 345
- `fail_alone_not_together`: []
- `fail_together_not_alone`: []
- `slowdown_runs`: 12.3
