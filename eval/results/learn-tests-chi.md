# learn-tests на chi (go): покрытие по тестам против `tests-for`
chi c64ed7d6635f, gitnext 08e599c, seed 7. Истина — eval (тесты, падающие при panic в начале функции). pick — функции с ответом в кэше eval, random — случайные.

| выборка | функций | источник | полнота | точность | из 10 падают | поймано |
|---|---:|---|---:|---:|---:|---:|
| pick (кэш eval) | 16 | gitnext | 66% | 61% | 67% | 88% |
| pick (кэш eval) | 16 | coverage | 100% | 99% | 94% | 100% |
| pick (кэш eval) | 16 | union | 100% | 72% | 81% | 100% |
| случайные | 9 | gitnext | 68% | 87% | 87% | 100% |
| случайные | 9 | coverage | 100% | 97% | 84% | 100% |
| случайные | 9 | union | 100% | 86% | 78% | 100% |
| все | 25 | gitnext | 67% | 71% | 74% | 92% |
| все | 25 | coverage | 100% | 98% | 91% | 100% |
| все | 25 | union | 100% | 77% | 80% | 100% |

## По функциям

| выборка | функция | падают | граф: полн / из 10 | покрытие: полн / точн / из 10 | мимо покрытия |
|---|---|---:|---|---|---|
| pick | `_examples::fileserver::FileServer` | 0 | no test fails when it throws | названо 0 | |
| pick | `_examples::rest::ErrInvalidRequest` | 0 | no test fails when it throws | названо 0 | |
| pick | `_examples::rest::NewArticleListResponse` | 0 | no test fails when it throws | названо 0 | |
| pick | `_examples::rest::NewUserPayloadResponse` | 0 | no test fails when it throws | названо 0 | |
| pick | `main::Chain` | 11 | 18% / 100% | 100% / 92% / 90% |  |
| pick | `main::Middlewares::HandlerFunc` | 1 | 0% / 0% | 100% / 100% / 100% |  |
| pick | `main::Mux::MethodNotAllowedHandler` | 4 | 100% / 40% | 100% / 100% / 100% |  |
| pick | `main::Mux::NotFoundHandler` | 14 | 64% / 30% | 100% / 100% / 100% |  |
| pick | `main::NewMux` | 101 | 98% / 100% | 100% / 95% / 50% |  |
| pick | `main::RouteContext` | 31 | 74% / 100% | 100% / 100% / 100% |  |
| pick | `main::URLParamFromCtx` | 20 | 100% / 100% | 100% / 100% / 100% |  |
| pick | `main::Walk` | 5 | 100% / 100% | 100% / 100% / 100% |  |
| pick | `main::endpoints::Value` | 105 | 38% / 90% | 100% / 97% / 70% |  |
| pick | `middleware::GetClientIPAddr` | 24 | 79% / 100% | 100% / 100% / 100% |  |
| pick | `middleware::GetLogEntry` | 2 | 0% / 0% | 100% / 100% / 100% |  |
| pick | `middleware::GetReqID` | 4 | 25% / 100% | 100% / 100% / 100% |  |
| pick | `middleware::HeaderRoute::IsMatch` | 1 | 100% / 50% | 100% / 100% / 100% |  |
| pick | `middleware::NewCompressor` | 3 | 100% / 100% | 100% / 100% / 100% |  |
| pick | `middleware::NewWrapResponseWriter` | 5 | 60% / 38% | 100% / 100% / 100% |  |
| pick | `middleware::RequestLogger` | 0 | mutant does not compile | названо 83 | |
| pick | `middleware::Throttle` | 0 | no test fails when it throws | названо 0 | |
| pick | `middleware::compressResponseWriter::Close` | 2 | 100% / 20% | 100% / 100% / 100% |  |
| random | `middleware::ContentCharset` | 2 | 100% / 50% | 100% / 100% / 100% |  |
| random | `middleware::matchAcceptEncoding` | 2 | 100% / 67% | 100% / 100% / 100% |  |
| random | `middleware::init` | 0 | mutant does not compile | названо 83 | |
| random | `middleware::init` | 0 | mutant does not compile | названо 83 | |
| random | `main::Context::URLParam` | 20 | 100% / 100% | 100% / 100% / 100% |  |
| random | `middleware::ThrottleWithOpts` | 6 | 100% / 100% | 100% / 100% / 100% |  |
| random | `main::node::setEndpoint` | 105 | 8% / 100% | 100% / 97% / 70% |  |
| random | `main::node::getEdge` | 105 | 8% / 100% | 100% / 95% / 60% |  |
| random | `middleware::mustParsePrefixes` | 16 | 94% / 70% | 100% / 89% / 80% |  |
| random | `main::walk` | 5 | 100% / 100% | 100% / 100% / 100% |  |
| random | `main::chain` | 100 | 3% / 100% | 100% / 95% / 50% |  |
| random | `middleware::PageRoute` | 0 | no test fails when it throws | названо 0 | |

## Цена

- `build_s`: 1.2
- `run_one_by_one_s`: 29.7
- `map_s`: 30.9
- `profiles_bytes`: 5994063
- `packages`: 2
- `testmain_packages`: []
- `repeat`: 3
- `plain_s`: 26.8
- `tests`: 140
- `map_pairs`: 3826
- `map_functions`: 164
- `map_sqlite_bytes`: 53248
- `functions_with_body`: 189
- `functions_never_run`: 23
- `fail_alone_not_together`: []
- `fail_together_not_alone`: []
- `slowdown_runs`: 1.2
