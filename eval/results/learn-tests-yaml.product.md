# gitnext learn-tests на yaml: `tests-for` до карты (граф) и после (карта)
yaml 528ef30d6ded, gitnext eeae698, seed 7. Истина — eval (файлы тестов, падающие при throw в начале функции). pick — функции с ответом в кэше eval, random — случайные.
Ответ — `tests-for <функция> --all` по файлам; «из карты» — сколько функций `tests-for` ответил по карте.

| выборка | функций | из карты | источник | полнота | точность | из 10 падают | поймано |
|---|---:|---:|---|---:|---:|---:|---:|
| pick (кэш eval) | 21 | 21 | граф | 18% | 31% | 32% | 38% |
| pick (кэш eval) | 21 | 21 | карта | 100% | 100% | 100% | 100% |
| случайные | 11 | 11 | граф | 14% | 38% | 38% | 46% |
| случайные | 11 | 11 | карта | 100% | 100% | 100% | 100% |
| все | 32 | 32 | граф | 16% | 33% | 34% | 41% |
| все | 32 | 32 | карта | 100% | 100% | 100% | 100% |

## По функциям

| выборка | функция | падают | граф: полн / из 10 | карта: полн / точн / из 10 | мимо карты |
|---|---|---:|---|---|---|
| pick | `src::compose::compose_collection::composeCollection` | 17 | 0% / 0% | 100% / 100% / 100% |  |
| pick | `src::compose::compose_doc::composeDoc` | 18 | 61% / 90% | 100% / 100% / 100% |  |
| pick | `src::compose::compose_scalar::composeScalar` | 18 | 0% / 0% | 100% / 100% / 100% |  |
| pick | `src::compose::composer::Composer::compose` | 17 | 71% / 90% | 100% / 100% / 100% |  |
| pick | `src::compose::resolve_block_seq::resolveBlockSeq` | 13 | 0% / 0% | 100% / 100% / 100% |  |
| pick | `src::compose::resolve_end::resolveEnd` | 19 | 58% / 90% | 100% / 100% / 100% |  |
| pick | `src::compose::resolve_flow_scalar::resolveFlowScalar` | 0 | mutant does not compile | названо 19 | |
| pick | `src::compose::util_contains_newline::containsNewline` | 17 | 0% / 0% | 100% / 100% / 100% |  |
| pick | `src::compose::util_flow_indent_check::flowIndentCheck` | 1 | 0% / 0% | 100% / 100% / 100% |  |
| pick | `src::doc::Document::Document::setSchema` | 20 | 5% / 100% | 100% / 100% / 100% |  |
| pick | `src::doc::NodeCreator::NodeCreator::setAnchors` | 20 | 35% / 100% | 100% / 100% / 100% |  |
| pick | `src::doc::directives::Directives::atDocument` | 18 | 0% / 0% | 100% / 100% / 100% |  |
| pick | `src::nodes::YAMLMap::YAMLMap::#stringifyFlowMap` | 8 | 0% / 0% | 100% / 100% / 100% |  |
| pick | `src::nodes::YAMLSeq::YAMLSeq::#stringifyBlockSeq` | 12 | 0% / 0% | 100% / 100% / 100% |  |
| pick | `src::nodes::YAMLSet::YAMLSet::#stringifyBlockSet` | 2 | 0% / 0% | 100% / 100% / 100% |  |
| pick | `src::nodes::YAMLSet::YAMLSet::#stringifyFlowSet` | 1 | 0% / 0% | 100% / 100% / 100% |  |
| pick | `src::nodes::toJS::ToJSContext::resolveAlias` | 6 | 0% / 0% | 100% / 100% / 100% |  |
| pick | `src::nodes::toJS::ToJSContext::setAnchor` | 6 | 0% / 0% | 100% / 100% / 100% |  |
| pick | `src::parse::cst::tokenType` | 19 | 63% / 90% | 100% / 100% / 100% |  |
| pick | `src::parse::lexer::Lexer::flow` | 15 | 60% / 60% | 100% / 100% / 100% |  |
| pick | `src::public_api::parseAllDocuments` | 4 | 25% / 50% | 100% / 100% / 100% |  |
| pick | `src::schema::yaml_1_1::pairs::resolvePairs` | 4 | 0% / 0% | 100% / 100% / 100% |  |
| random | `src::nodes::YAMLSeq::YAMLSeq::toJS` | 8 | 12% / 50% | 100% / 100% / 100% |  |
| random | `src::nodes::Alias::Alias::toJS` | 6 | 33% / 100% | 100% / 100% / 100% |  |
| random | `src::doc::anchors::anchorIsValid` | 8 | 0% / 0% | 100% / 100% / 100% |  |
| random | `src::schema::core::float::couldBeFloat` | 18 | 0% / 0% | 100% / 100% / 100% |  |
| random | `src::schema::yaml_1_1::pairs::createPairs` | 3 | 0% / 0% | 100% / 100% / 100% |  |
| random | `src::doc::Document::Document::set` | 5 | 0% / 0% | 100% / 100% / 100% |  |
| random | `src::nodes::util_clone_map_or_set::cloneMapOrSet` | 1 | 0% / 0% | 100% / 100% / 100% |  |
| random | `src::parse::lexer::Lexer::indicators` | 20 | 20% / 80% | 100% / 100% / 100% |  |
| random | `src::stringify::stringify::getResolvedOptions` | 17 | 0% / 0% | 100% / 100% / 100% |  |
| random | `src::stringify::stringifyNumber::stringifyNumber` | 11 | 9% / 100% | 100% / 100% / 100% |  |
| random | `src::parse::cst_scalar::createScalarToken` | 0 | mutant does not compile | названо 1 | |
| random | `src::parse::lexer::lex` | 20 | 75% / 90% | 100% / 100% / 100% |  |

## Цена

- `plain_s`: 5.7
- `learn_cold_s`: 37.3
- `learn_output`:

```
learned which tests run which functions: 285 functions, 22 tests, 2942 pairs (37s: npx vitest run <file> --coverage.enabled --coverage.provider=v8 --coverage.reporter=json --coverage.include=src/**/*.{ts,tsx,mts,cts,js,jsx,mjs,cjs}, each test file alone (24 files, vitest 4.1.11))
54 functions were run by no test — for those `tests-for` still uses the call graph
note: JS/TS: 2 test files wrote no coverage report and are not in the map: tests/json-test-suite.ts (ENOENT: no such file or directory, scandir '/home/claude/gitnext/eval/.work/yaml/tests/json-test-suite/test_parsing'), tests/yaml-test-suite.ts (ENOENT: no such file or directory, scandir '/home/claude/gitnext/eval/.work/yaml/tests/yaml-test-suite/src')
note: the test run itself reported failures; the map holds what ran
`tests-for`, `look` and `check` now use the map; run `gitnext learn-tests` again after larger changes
```
- `learn_warm_s`: 37.8
- `learn_x_plain`: 6.6
- `map_sqlite_bytes`: 225280
