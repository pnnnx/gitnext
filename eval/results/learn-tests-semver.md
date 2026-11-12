# learn-tests на semver (rust): покрытие по тестам против `tests-for`
semver 280ebcb6edac, gitnext 08e599c, seed 7. Истина — eval (тесты, падающие при panic в начале функции). pick — функции с ответом в кэше eval, random — случайные.

| выборка | функций | источник | полнота | точность | из 10 падают | поймано |
|---|---:|---|---:|---:|---:|---:|
| pick (кэш eval) | 3 | gitnext | 61% | 64% | 67% | 67% |
| pick (кэш eval) | 3 | coverage | 100% | 100% | 100% | 100% |
| pick (кэш eval) | 3 | union | 100% | 97% | 100% | 100% |
| случайные | 12 | gitnext | 72% | 56% | 60% | 75% |
| случайные | 12 | coverage | 100% | 100% | 100% | 100% |
| случайные | 12 | union | 100% | 82% | 88% | 100% |
| все | 15 | gitnext | 70% | 57% | 61% | 73% |
| все | 15 | coverage | 100% | 100% | 100% | 100% |
| все | 15 | union | 100% | 85% | 91% | 100% |

## По функциям

| выборка | функция | падают | граф: полн / из 10 | покрытие: полн / точн / из 10 | мимо покрытия |
|---|---|---:|---|---|---|
| pick | `semver::eval::matches_comparator` | 0 | no test fails when it throws | названо 0 | |
| pick | `semver::eval::matches_req` | 11 | 100% / 100% | 100% / 100% / 100% |  |
| pick | `semver::identifier::Identifier::empty` | 0 | mutant does not compile | названо 29 | |
| pick | `semver::identifier::Identifier::new_unchecked` | 24 | 83% / 100% | 100% / 100% / 100% |  |
| pick | `semver::identifier::Identifier::ptr_eq` | 20 | 0% / 0% | 100% / 100% / 100% |  |
| random | `semver::identifier::Identifier::clone` | 1 | 0% / 0% | 100% / 100% / 100% |  |
| random | `semver::eval::matches_tilde` | 1 | 100% / 10% | 100% / 100% / 100% |  |
| random | `semver::identifier::decode_len` | 0 | no test fails when it throws | названо 4 | |
| random | `semver::eval::matches_exact` | 7 | 100% / 60% | 100% / 100% / 100% |  |
| random | `semver::impls::VersionReq::from_iter` | 0 | no test fails when it throws | названо 0 | |
| random | `semver::eval::matches_impl` | 10 | 100% / 90% | 100% / 100% / 100% |  |
| random | `semver::display::Version::fmt` | 2 | 0% / 0% | 100% / 100% / 100% |  |
| random | `semver::parse::BuildMetadata::from_str` | 2 | 100% / 100% | 100% / 100% / 100% |  |
| random | `semver::serde::Version::deserialize` | 0 | no test fails when it throws | названо 0 | |
| random | `semver::eval::pre_is_compatible` | 8 | 100% / 80% | 100% / 100% / 100% |  |
| random | `semver::parse::identifier` | 24 | 0% / 0% | 100% / 100% / 100% |  |
| random | `semver::identifier::Identifier::is_empty` | 0 | no test fails when it throws | названо 29 | |
| random | `semver::error::Error::fmt` | 0 | no test fails when it throws | названо 9 | |
| random | `semver::parse::Prerelease::from_str` | 4 | 100% / 100% | 100% / 100% / 100% |  |
| random | `semver::parse::build_identifier` | 11 | 82% / 80% | 100% / 100% / 100% |  |
| random | `semver::serde::VersionReq::deserialize` | 0 | no test fails when it throws | названо 0 | |
| random | `semver::Version::cmp_precedence` | 0 | no test fails when it throws | названо 0 | |
| random | `semver::identifier::Identifier::is_empty_or_inline` | 0 | no test fails when it throws | названо 29 | |
| random | `semver::parse::version_req` | 17 | 100% / 100% | 100% / 100% / 100% |  |
| random | `semver::parse::prerelease_identifier` | 24 | 83% / 100% | 100% / 100% / 100% |  |

## Цена

- `build_s`: 20.6
- `run_one_by_one_s`: 0.3
- `export_s`: 0.6
- `map_s`: 21.5
- `map_without_build_s`: 0.9
- `profiles_bytes`: 341456
- `test_binaries`: 5
- `files_with_doctests`: 2
- `repeat`: 3
- `plain_s`: 0.4
- `tests`: 34
- `map_pairs`: 782
- `map_functions`: 70
- `map_sqlite_bytes`: 20480
- `functions_with_body`: 85
- `functions_never_run`: 11
- `fail_alone_not_together`: []
- `fail_together_not_alone`: []
- `slowdown_runs`: 53.8
