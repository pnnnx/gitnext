# learn-tests на zustand (typescript): покрытие по тестам против `tests-for`
zustand 9fed2eb2f3af, gitnext 92dc87d, seed 7. Истина — eval (тесты, падающие при throw в начале функции). pick — функции с ответом в кэше eval, random — случайные.

| выборка | функций | источник | полнота | точность | из 10 падают | поймано |
|---|---:|---|---:|---:|---:|---:|
| pick (кэш eval) | 1 | gitnext | 33% | 100% | 100% | 100% |
| pick (кэш eval) | 1 | coverage | 100% | 100% | 100% | 100% |
| pick (кэш eval) | 1 | union | 100% | 100% | 100% | 100% |
| случайные | 18 | gitnext | 53% | 56% | 56% | 61% |
| случайные | 18 | coverage | 100% | 93% | 93% | 100% |
| случайные | 18 | union | 100% | 90% | 90% | 100% |
| все | 19 | gitnext | 52% | 58% | 58% | 63% |
| все | 19 | coverage | 100% | 94% | 94% | 100% |
| все | 19 | union | 100% | 91% | 91% | 100% |

## По функциям

| выборка | функция | падают | граф: полн / из 10 | покрытие: полн / точн / из 10 | мимо покрытия |
|---|---|---:|---|---|---|
| pick | `src::middleware::persist::createJSONStorage` | 6 | 33% / 100% | 100% / 100% / 100% |  |
| random | `src::middleware::devtools::setStateFromDevtools` | 1 | 100% / 100% | 100% / 100% / 100% |  |
| random | `src::middleware::persist::persistImpl` | 6 | 0% / 0% | 100% / 100% / 100% |  |
| random | `src::middleware::persist::parse` | 3 | 0% / 0% | 100% / 60% / 60% |  |
| random | `src::middleware::devtools::maskToRegex` | 1 | 100% / 100% | 100% / 100% / 100% |  |
| random | `src::vanilla::subscribe` | 7 | 0% / 0% | 100% / 100% / 100% |  |
| random | `src::vanilla::shallow::shallow` | 2 | 100% / 100% | 100% / 100% / 100% |  |
| random | `src::react::shallow::useShallow` | 1 | 100% / 50% | 100% / 100% / 100% |  |
| random | `src::middleware::devtools::parseDropdownArgs` | 1 | 100% / 100% | 100% / 100% / 100% |  |
| random | `src::react::createImpl` | 9 | 22% / 100% | 100% / 100% / 100% |  |
| random | `src::vanilla::createStoreImpl` | 13 | 31% / 100% | 100% / 100% / 100% |  |
| random | `src::middleware::devtools::getTrackedConnectionState` | 1 | 100% / 100% | 100% / 100% / 100% |  |
| random | `src::vanilla::shallow::compareIterables` | 1 | 100% / 50% | 100% / 50% / 50% |  |
| random | `src::traditional::createWithEqualityFnImpl` | 2 | 0% / 0% | 100% / 100% / 100% |  |
| random | `src::middleware::devtools::getParamNames` | 1 | 100% / 100% | 100% / 100% / 100% |  |
| random | `src::middleware::persist::toSyncThenable` | 0 | mutant does not compile | названо 0 | |
| random | `src::vanilla::shallow::hasIterableEntries` | 0 | mutant does not compile | названо 2 | |
| random | `src::vanilla::shallow::compareEntries` | 2 | 100% / 100% | 100% / 100% / 100% |  |
| random | `src::middleware::redux::reduxImpl` | 2 | 0% / 0% | 100% / 67% / 67% |  |
| random | `src::middleware::persist::toThenable` | 6 | 0% / 0% | 100% / 100% / 100% |  |
| random | `src::middleware::persist::hydrate` | 6 | 0% / 0% | 100% / 100% / 100% |  |

## Цена

- `run_one_by_one_s`: 31.9
- `map_s`: 31.9
- `whole_suite_coverage_s`: 16.7
- `coverage_json_bytes`: 1770510
- `test_files`: 14
- `repeat`: 3
- `plain_s`: 14.1
- `tests`: 14
- `map_pairs`: 163
- `map_functions`: 47
- `map_sqlite_bytes`: 12288
- `functions_with_body`: 55
- `functions_never_run`: 7
- `fail_alone_not_together`: []
- `fail_together_not_alone`: []
- `slowdown_runs`: 2.3
