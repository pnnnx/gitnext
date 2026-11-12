# learn-tests на yaml (typescript): покрытие по тестам против `tests-for`
yaml 528ef30d6ded, gitnext 92dc87d, seed 7. Истина — eval (тесты, падающие при throw в начале функции). pick — функции с ответом в кэше eval, random — случайные.

| выборка | функций | источник | полнота | точность | из 10 падают | поймано |
|---|---:|---|---:|---:|---:|---:|
| pick (кэш eval) | 21 | gitnext | 2% | 16% | 16% | 24% |
| pick (кэш eval) | 21 | coverage | 100% | 100% | 100% | 100% |
| pick (кэш eval) | 21 | union | 100% | 99% | 100% | 100% |
| случайные | 11 | gitnext | 4% | 33% | 33% | 36% |
| случайные | 11 | coverage | 100% | 100% | 100% | 100% |
| случайные | 11 | union | 100% | 99% | 100% | 100% |
| все | 32 | gitnext | 3% | 22% | 22% | 28% |
| все | 32 | coverage | 100% | 100% | 100% | 100% |
| все | 32 | union | 100% | 99% | 100% | 100% |

## По функциям

| выборка | функция | падают | граф: полн / из 10 | покрытие: полн / точн / из 10 | мимо покрытия |
|---|---|---:|---|---|---|
| pick | `src::compose::compose_collection::composeCollection` | 17 | 0% / 0% | 100% / 100% / 100% |  |
| pick | `src::compose::compose_doc::composeDoc` | 18 | 6% / 50% | 100% / 100% / 100% |  |
| pick | `src::compose::compose_scalar::composeScalar` | 18 | 0% / 0% | 100% / 100% / 100% |  |
| pick | `src::compose::composer::Composer::compose` | 17 | 0% / 0% | 100% / 100% / 100% |  |
| pick | `src::compose::resolve_block_seq::resolveBlockSeq` | 13 | 0% / 0% | 100% / 100% / 100% |  |
| pick | `src::compose::resolve_end::resolveEnd` | 19 | 5% / 50% | 100% / 100% / 100% |  |
| pick | `src::compose::resolve_flow_scalar::resolveFlowScalar` | 0 | mutant does not compile | названо 19 | |
| pick | `src::compose::util_contains_newline::containsNewline` | 17 | 0% / 0% | 100% / 100% / 100% |  |
| pick | `src::compose::util_flow_indent_check::flowIndentCheck` | 1 | 0% / 0% | 100% / 100% / 100% |  |
| pick | `src::doc::Document::Document::setSchema` | 20 | 0% / 0% | 100% / 100% / 100% |  |
| pick | `src::doc::NodeCreator::NodeCreator::setAnchors` | 20 | 5% / 100% | 100% / 100% / 100% |  |
| pick | `src::doc::directives::Directives::atDocument` | 18 | 0% / 0% | 100% / 100% / 100% |  |
| pick | `src::nodes::YAMLMap::YAMLMap::#stringifyFlowMap` | 8 | 0% / 0% | 100% / 100% / 100% |  |
| pick | `src::nodes::YAMLSeq::YAMLSeq::#stringifyBlockSeq` | 12 | 0% / 0% | 100% / 100% / 100% |  |
| pick | `src::nodes::YAMLSet::YAMLSet::#stringifyBlockSet` | 2 | 0% / 0% | 100% / 100% / 100% |  |
| pick | `src::nodes::YAMLSet::YAMLSet::#stringifyFlowSet` | 1 | 0% / 0% | 100% / 100% / 100% |  |
| pick | `src::nodes::toJS::ToJSContext::resolveAlias` | 6 | 0% / 0% | 100% / 100% / 100% |  |
| pick | `src::nodes::toJS::ToJSContext::setAnchor` | 6 | 0% / 0% | 100% / 100% / 100% |  |
| pick | `src::parse::cst::tokenType` | 19 | 11% / 67% | 100% / 100% / 100% |  |
| pick | `src::parse::lexer::Lexer::flow` | 15 | 13% / 67% | 100% / 100% / 100% |  |
| pick | `src::public_api::parseAllDocuments` | 4 | 0% / 0% | 100% / 100% / 100% |  |
| pick | `src::schema::yaml_1_1::pairs::resolvePairs` | 4 | 0% / 0% | 100% / 100% / 100% |  |
| random | `src::nodes::YAMLSeq::YAMLSeq::toJS` | 8 | 12% / 100% | 100% / 100% / 100% |  |
| random | `src::nodes::Alias::Alias::toJS` | 6 | 17% / 100% | 100% / 100% / 100% |  |
| random | `src::doc::anchors::anchorIsValid` | 8 | 0% / 0% | 100% / 100% / 100% |  |
| random | `src::schema::core::float::couldBeFloat` | 18 | 0% / 0% | 100% / 100% / 100% |  |
| random | `src::schema::yaml_1_1::pairs::createPairs` | 3 | 0% / 0% | 100% / 100% / 100% |  |
| random | `src::doc::Document::Document::set` | 5 | 0% / 0% | 100% / 100% / 100% |  |
| random | `src::nodes::util_clone_map_or_set::cloneMapOrSet` | 1 | 0% / 0% | 100% / 100% / 100% |  |
| random | `src::parse::lexer::Lexer::indicators` | 20 | 10% / 100% | 100% / 100% / 100% |  |
| random | `src::stringify::stringify::getResolvedOptions` | 17 | 0% / 0% | 100% / 100% / 100% |  |
| random | `src::stringify::stringifyNumber::stringifyNumber` | 11 | 0% / 0% | 100% / 100% / 100% |  |
| random | `src::parse::cst_scalar::createScalarToken` | 0 | mutant does not compile | названо 1 | |
| random | `src::parse::lexer::lex` | 20 | 10% / 67% | 100% / 100% / 100% |  |

## Цена

- `run_one_by_one_s`: 48.6
- `map_s`: 48.6
- `whole_suite_coverage_s`: 9.2
- `coverage_json_bytes`: 24735036
- `test_files`: 24
- `repeat`: 3
- `plain_s`: 7.8
- `tests`: 24
- `map_pairs`: 2919
- `map_functions`: 284
- `map_sqlite_bytes`: 40960
- `functions_with_body`: 306
- `functions_never_run`: 21
- `fail_alone_not_together`: []
- `fail_together_not_alone`: []
- `slowdown_runs`: 6.2
