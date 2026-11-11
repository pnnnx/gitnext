# learn-tests на click: покрытие по тестам против `tests-for`
click 8abb77a8432e, gitnext adb3aac, seed 7. Метрики как в eval: полнота, точность, доля падающих в первых 10, поймано ли первыми 10.

## Итог: полная истина (основная)

| выборка | функций | источник | полнота | точность | из 10 падают | поймано |
|---|---:|---|---:|---:|---:|---:|
| pick (есть вызывающие) | 18 | gitnext | 17% | 41% | 43% | 67% |
| pick (есть вызывающие) | 18 | coverage | 99% | 98% | 99% | 100% |
| pick (есть вызывающие) | 18 | union | 99% | 86% | 96% | 100% |
| случайные | 22 | gitnext | 21% | 28% | 29% | 41% |
| случайные | 22 | coverage | 100% | 98% | 98% | 100% |
| случайные | 22 | union | 100% | 77% | 82% | 100% |
| все | 40 | gitnext | 19% | 34% | 35% | 52% |
| все | 40 | coverage | 100% | 98% | 99% | 100% |
| все | 40 | union | 100% | 81% | 88% | 100% |

## Итог: истина eval (`tail -400`)

| выборка | функций | источник | полнота | точность | из 10 падают | поймано |
|---|---:|---|---:|---:|---:|---:|
| pick (есть вызывающие) | 17 | gitnext | 17% | 27% | 29% | 53% |
| pick (есть вызывающие) | 17 | coverage | 99% | 87% | 84% | 100% |
| pick (есть вызывающие) | 17 | union | 99% | 74% | 81% | 100% |
| случайные | 21 | gitnext | 22% | 19% | 20% | 29% |
| случайные | 21 | coverage | 100% | 82% | 79% | 95% |
| случайные | 21 | union | 100% | 62% | 62% | 95% |
| все | 38 | gitnext | 20% | 22% | 24% | 40% |
| все | 38 | coverage | 99% | 85% | 81% | 97% |
| все | 38 | union | 99% | 67% | 70% | 97% |

## По функциям (полная истина)

| выборка | функция | падают (eval) | gitnext: полн / из 10 | покрытие: полн / точн / из 10 | объединение: полн / из 10 |
|---|---|---:|---|---|---|
| pick | `click::types::Path::coerce_path_result` | 9 (9) | 0% / 0% | 89% / 100% / 100% | 89% / 100% |
| pick | `click::core::Option::get_help_extra` | 89 (89) | 12% / 100% | 100% / 99% / 100% | 100% / 100% |
| pick | `click::formatting::HelpFormatter::write_heading` | 76 (76) | 13% / 50% | 100% / 99% / 100% | 100% / 90% |
| pick | `click::formatting::HelpFormatter::indentation` | 10 (10) | 50% / 40% | 100% / 100% / 100% | 100% / 100% |
| pick | `click::globals::push_context` | 372 (122) | 0% / 0% | 100% / 98% / 100% | 100% / 100% |
| pick | `click::_textwrap::TextWrapper::extra_indent` | 77 (77) | 1% / 10% | 100% / 99% / 100% | 100% / 100% |
| pick | `click::termui::confirm` | 6 (6) | 50% / 20% | 100% / 100% / 100% | 100% / 60% |
| pick | `click::core::Command::format_usage` | 109 (109) | 0% / 0% | 100% / 98% / 100% | 100% / 100% |
| pick | `click::decorators::option` | 355 (0, не импортир. файлов 1) | 14% / 90% | 100% / 94% / 100% | 100% / 100% |
| pick | `click::_compat::should_strip_ansi` | 26 (26) | 15% / 20% | 96% / 100% / 100% | 96% / 100% |
| pick | `click::_winconsole::get_buffer` | 0 | no test fails when it throws: названо 0 | названо 0 | |
| pick | `click::core::Parameter::handle_parse_result` | 365 (122) | 2% / 100% | 100% / 97% / 100% | 100% / 100% |
| pick | `click::core::Context::get_parameter_source` | 365 (122) | 1% / 100% | 100% / 97% / 100% | 100% / 100% |
| pick | `click::core::Context::call_on_close` | 16 (16) | 0% / 0% | 94% / 100% / 100% | 94% / 100% |
| pick | `click::formatting::HelpFormatter::write_dl` | 76 (76) | 13% / 50% | 100% / 99% / 100% | 100% / 90% |
| pick | `click::core::Group::result_callback` | 2 (2) | 100% / 100% | 100% / 100% / 100% | 100% / 100% |
| pick | `click::core::Context::ensure_object` | 2 (2) | 0% / 0% | 100% / 100% / 100% | 100% / 100% |
| pick | `click::_compat::get_best_encoding` | 5 (5) | 0% / 0% | 100% / 100% / 100% | 100% / 100% |
| pick | `click::_termui_impl::ProgressBar::format_bar` | 9 (9) | 33% / 100% | 100% / 90% / 90% | 100% / 90% |
| pick | `examples::aliases::aliases::Config::write_config` | 0 | no test fails when it throws: названо 0 | названо 0 | |
| random | `click::_winconsole::_WindowsConsoleWriter::write` | 0 | no test fails when it throws: названо 7 | названо 0 | |
| random | `click::core::Option::_pick_type` | 365 (0, не импортир. файлов 3) | 0% / 0% | 100% / 87% / 70% | 100% / 70% |
| random | `click::core::Argument::get_error_hint` | 14 (14) | 0% / 0% | 100% / 93% / 90% | 100% / 90% |
| random | `click::core::Parameter::consume_value` | 365 (122) | 1% / 100% | 100% / 97% / 100% | 100% / 100% |
| random | `click::core::Option::flag_activation_value` | 369 (120) | 0% / 0% | 100% / 97% / 100% | 100% / 100% |
| random | `click::_textwrap::TextWrapper::_wrap_chunks` | 113 (113) | 0% / 0% | 100% / 98% / 100% | 100% / 100% |
| random | `click::core::Group::invoke` | 41 (41) | 2% / 10% | 100% / 100% / 100% | 100% / 100% |
| random | `click::shell_completion::FishComplete::get_completion_args` | 1 (1) | 100% / 0% | 100% / 100% / 100% | 100% / 10% |
| random | `click::_winconsole::_get_windows_console_stream` | 0 | no test fails when it throws: названо 0 | названо 0 | |
| random | `click::decorators::version_option` | 3 (3) | 100% / 60% | 100% / 100% / 100% | 100% / 60% |
| random | `click::_compat::_FixupStream::readable` | 1 (1) | 0% / 0% | 100% / 100% / 100% | 100% / 33% |
| random | `click::_termui_impl::get_pager_file` | 19 (19) | 42% / 100% | 100% / 100% / 100% | 100% / 100% |
| random | `click::exceptions::ClickException::__init__` | 95 (95) | 0% / 0% | 100% / 93% / 100% | 100% / 100% |
| random | `click::types::Tuple::to_info_dict` | 1 (1) | 0% / 0% | 100% / 100% / 100% | 100% / 20% |
| random | `click::core::Option::process_value` | 335 (95) | 0% / 0% | 100% / 90% / 100% | 100% / 100% |
| random | `click::core::Group::parse_args` | 58 (58) | 16% / 60% | 100% / 98% / 100% | 100% / 100% |
| random | `click::core::Parameter::value_from_envvar` | 31 (31) | 0% / 0% | 100% / 97% / 100% | 100% / 100% |
| random | `click::core::Argument::get_help_record` | 26 (26) | 4% / 100% | 100% / 100% / 100% | 100% / 100% |
| random | `click::types::File::shell_complete` | 1 (1) | 0% / 0% | 100% / 100% / 100% | 100% / 100% |
| random | `click::core::Command::get_help_option` | 373 (122) | 1% / 80% | 100% / 98% / 100% | 100% / 100% |
| random | `click::shell_completion::ShellComplete::__init__` | 34 (34) | 100% / 80% | 100% / 100% / 100% | 100% / 100% |
| random | `click::parser::_OptionParser::parse_args` | 367 (121) | 1% / 40% | 100% / 97% / 100% | 100% / 100% |
| random | `click::shell_completion::BashComplete::get_completion_args` | 2 (2) | 100% / 0% | 100% / 100% / 100% | 100% / 20% |
| random | `click::_compat::_NonClosingTextIOWrapper::__del__` | 1 (1) | 0% / 0% | 100% / 100% / 100% | 100% / 100% |

## Цена

- прогон тестов: без покрытия 6.2 с, pytest-cov по тестам 14.2 с (×2.3), coverage `test_function` 15.2 с; медиана из 3
- `.coverage` 3047424 байт; карта «функция → тест»: 143423 пар, 430 функций, 2571 контекстов; SQLite (2 int на пару) 1626112 байт
- тестов собрано 2648, разных имён тестов в карте 585
- функций с телом: 585; исполнено хоть раз вне тестов (импорт, сбор): 33, только вне тестов: 0; не исполнено ни разу: 155
- `test_function` против pytest-cov: пар 43755 против 143423; тестов, падающих по полной истине, но не найденных: 4 против 4
