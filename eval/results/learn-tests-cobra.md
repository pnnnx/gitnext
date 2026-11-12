# learn-tests на cobra (go): покрытие по тестам против `tests-for`
cobra adbc8813901b, gitnext 08e599c, seed 7. Истина — eval (тесты, падающие при panic в начале функции). pick — функции с ответом в кэше eval, random — случайные.

| выборка | функций | источник | полнота | точность | из 10 падают | поймано |
|---|---:|---|---:|---:|---:|---:|
| pick (кэш eval) | 21 | gitnext | 89% | 36% | 44% | 81% |
| pick (кэш eval) | 21 | coverage | 100% | 100% | 98% | 100% |
| pick (кэш eval) | 21 | union | 100% | 39% | 80% | 100% |
| случайные | 8 | gitnext | 86% | 30% | 35% | 100% |
| случайные | 8 | coverage | 100% | 100% | 100% | 100% |
| случайные | 8 | union | 100% | 39% | 70% | 100% |
| все | 29 | gitnext | 88% | 35% | 42% | 86% |
| все | 29 | coverage | 100% | 100% | 99% | 100% |
| все | 29 | union | 100% | 39% | 77% | 100% |

## По функциям

| выборка | функция | падают | граф: полн / из 10 | покрытие: полн / точн / из 10 | мимо покрытия |
|---|---|---:|---|---|---|
| pick | `main::CheckErr` | 109 | 93% / 80% | 100% / 100% / 100% |  |
| pick | `main::Command::CalledAs` | 47 | 100% / 20% | 100% / 100% / 100% |  |
| pick | `main::Command::CommandPathPadding` | 1 | 100% / 0% | 100% / 100% / 100% |  |
| pick | `main::Command::Commands` | 96 | 100% / 100% | 100% / 100% / 100% |  |
| pick | `main::Command::FlagErrorFunc` | 17 | 100% / 20% | 100% / 100% / 100% |  |
| pick | `main::Command::Flags` | 0 | mutant does not compile | названо 252 | |
| pick | `main::Command::GenPowerShellCompletionWithDesc` | 0 | no test fails when it throws | названо 0 | |
| pick | `main::Command::GenZshCompletion` | 3 | 100% / 30% | 100% / 100% / 100% |  |
| pick | `main::Command::HasParent` | 0 | mutant does not compile | названо 265 | |
| pick | `main::Command::HasPersistentFlags` | 4 | 100% / 30% | 100% / 100% / 100% |  |
| pick | `main::Command::Help` | 7 | 100% / 0% | 100% / 100% / 100% |  |
| pick | `main::Command::InitDefaultVersionFlag` | 197 | 98% / 80% | 100% / 100% / 100% |  |
| pick | `main::Command::LocalFlags` | 149 | 5% / 89% | 100% / 100% / 100% |  |
| pick | `main::Command::NamePadding` | 21 | 90% / 0% | 100% / 100% / 100% |  |
| pick | `main::Command::OutOrStderr` | 87 | 99% / 40% | 100% / 100% / 100% |  |
| pick | `main::Command::Print` | 59 | 100% / 30% | 100% / 100% / 100% |  |
| pick | `main::Command::PrintErrf` | 7 | 86% / 10% | 100% / 100% / 100% |  |
| pick | `main::Command::PrintErrln` | 61 | 98% / 20% | 100% / 100% / 100% |  |
| pick | `main::Command::Root` | 248 | 94% / 100% | 100% / 99% / 100% |  |
| pick | `main::Command::SetGlobalNormalizationFunc` | 4 | 100% / 40% | 100% / 100% / 100% |  |
| pick | `main::Command::UseLine` | 86 | 12% / 40% | 100% / 100% / 100% |  |
| pick | `main::Command::ValidateFlagGroups` | 116 | 97% / 100% | 100% / 100% / 100% |  |
| pick | `main::CompErrorln` | 1 | 100% / 0% | 100% / 100% / 100% |  |
| pick | `main::MinimumNArgs` | 204 | 98% / 100% | 100% / 98% / 60% |  |
| pick | `main::flagCompError::Error` | 0 | no test fails when it throws | названо 0 | |
| random | `main::Command::UsageTemplate` | 0 | no test fails when it throws | названо 0 | |
| random | `main::rpad` | 22 | 5% / 20% | 100% / 100% / 100% |  |
| random | `main::ShellCompDirective::string` | 46 | 100% / 20% | 100% / 100% / 100% |  |
| random | `main::writeCmdAliases` | 4 | 100% / 50% | 100% / 100% / 100% |  |
| random | `main::Command::UsagePadding` | 0 | no test fails when it throws | названо 0 | |
| random | `main::Command::getVersionTemplateFunc` | 8 | 100% / 20% | 100% / 100% / 100% |  |
| random | `doc::GenManTreeFromOpts` | 1 | 100% / 17% | 100% / 100% / 100% |  |
| random | `main::validateOneRequiredFlagGroups` | 68 | 85% / 40% | 100% / 100% / 100% |  |
| random | `main::Command::ResetCommands` | 0 | no test fails when it throws | названо 0 | |
| random | `main::Command::SetHelpTemplate` | 1 | 100% / 10% | 100% / 100% / 100% |  |
| random | `main::prepareCustomAnnotationsForFlags` | 8 | 100% / 100% | 100% / 100% / 100% |  |
| random | `main::Command::CommandPath` | 0 | mutant does not compile | названо 253 | |

## Цена

- `build_s`: 0.9
- `run_one_by_one_s`: 2.7
- `map_s`: 3.5
- `profiles_bytes`: 16868180
- `packages`: 2
- `testmain_packages`: []
- `repeat`: 3
- `plain_s`: 0.7
- `tests`: 279
- `map_pairs`: 13106
- `map_functions`: 236
- `map_sqlite_bytes`: 143360
- `functions_with_body`: 270
- `functions_never_run`: 34
- `fail_alone_not_together`: []
- `fail_together_not_alone`: []
- `slowdown_runs`: 5.0
