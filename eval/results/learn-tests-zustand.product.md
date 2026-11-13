# gitnext learn-tests на zustand: `tests-for` до карты (граф) и после (карта)
zustand 9fed2eb2f3af, gitnext eeae698, seed 7. Истина — eval (файлы тестов, падающие при throw в начале функции). pick — функции с ответом в кэше eval, random — случайные.
Ответ — `tests-for <функция> --all` по файлам; «из карты» — сколько функций `tests-for` ответил по карте.

| выборка | функций | из карты | источник | полнота | точность | из 10 падают | поймано |
|---|---:|---:|---|---:|---:|---:|---:|
| pick (кэш eval) | 1 | 1 | граф | 50% | 100% | 100% | 100% |
| pick (кэш eval) | 1 | 1 | карта | 100% | 100% | 100% | 100% |
| случайные | 18 | 18 | граф | 69% | 67% | 67% | 72% |
| случайные | 18 | 18 | карта | 100% | 93% | 93% | 100% |
| все | 19 | 19 | граф | 68% | 68% | 68% | 74% |
| все | 19 | 19 | карта | 100% | 94% | 94% | 100% |

## По функциям

| выборка | функция | падают | граф: полн / из 10 | карта: полн / точн / из 10 | мимо карты |
|---|---|---:|---|---|---|
| pick | `src::middleware::persist::createJSONStorage` | 6 | 50% / 100% | 100% / 100% / 100% |  |
| random | `src::middleware::devtools::setStateFromDevtools` | 1 | 100% / 100% | 100% / 100% / 100% |  |
| random | `src::middleware::persist::persistImpl` | 6 | 0% / 0% | 100% / 100% / 100% |  |
| random | `src::middleware::persist::parse` | 3 | 0% / 0% | 100% / 60% / 60% |  |
| random | `src::middleware::devtools::maskToRegex` | 1 | 100% / 100% | 100% / 100% / 100% |  |
| random | `src::vanilla::subscribe` | 7 | 43% / 100% | 100% / 100% / 100% |  |
| random | `src::vanilla::shallow::shallow` | 2 | 100% / 100% | 100% / 100% / 100% |  |
| random | `src::react::shallow::useShallow` | 1 | 100% / 50% | 100% / 100% / 100% |  |
| random | `src::middleware::devtools::parseDropdownArgs` | 1 | 100% / 100% | 100% / 100% / 100% |  |
| random | `src::react::createImpl` | 9 | 100% / 100% | 100% / 100% / 100% |  |
| random | `src::vanilla::createStoreImpl` | 13 | 100% / 100% | 100% / 100% / 100% |  |
| random | `src::middleware::devtools::getTrackedConnectionState` | 1 | 100% / 100% | 100% / 100% / 100% |  |
| random | `src::vanilla::shallow::compareIterables` | 1 | 100% / 50% | 100% / 50% / 50% |  |
| random | `src::traditional::createWithEqualityFnImpl` | 2 | 100% / 100% | 100% / 100% / 100% |  |
| random | `src::middleware::devtools::getParamNames` | 1 | 100% / 100% | 100% / 100% / 100% |  |
| random | `src::middleware::persist::toSyncThenable` | 0 | mutant does not compile | названо 0 | |
| random | `src::vanilla::shallow::hasIterableEntries` | 0 | mutant does not compile | названо 2 | |
| random | `src::vanilla::shallow::compareEntries` | 2 | 100% / 100% | 100% / 100% / 100% |  |
| random | `src::middleware::redux::reduxImpl` | 2 | 0% / 0% | 100% / 67% / 67% |  |
| random | `src::middleware::persist::toThenable` | 6 | 0% / 0% | 100% / 100% / 100% |  |
| random | `src::middleware::persist::hydrate` | 6 | 0% / 0% | 100% / 100% / 100% |  |

## Цена

- `plain_s`: 8.5
- `learn_cold_s`: 22.4
- `learn_output`:

```
learned which tests run which functions: 48 functions, 14 tests, 164 pairs (22s: npx vitest run <file> --coverage.enabled --coverage.provider=v8 --coverage.reporter=json --coverage.include=examples/**/*.{ts,tsx,mts,cts,js,jsx,mjs,cjs},rollup.config.mjs,src/**/*.{ts,tsx,mts,cts,js,jsx,mjs,cjs}, each test file alone (14 files, vitest 5.1.0))
38 functions were run by no test — for those `tests-for` still uses the call graph
`tests-for`, `look` and `check` now use the map; run `gitnext learn-tests` again after larger changes
```
- `learn_warm_s`: 22.3
- `learn_x_plain`: 2.6
- `map_sqlite_bytes`: 32768
