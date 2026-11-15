# heldout (A) → heldout-lsp-refined (B)

Paired: each function is measured by both versions, the difference B − A is taken per function, and the 90% interval comes from drawing the functions again (within each repo). `better` / `worse`: the whole interval is on one side of zero. `can't tell`: it covers zero. `same`: no function changed. `< 0.5 pt`: the whole interval is within half a point. `changed`: functions whose value differs — a difference rests on them alone, read them below. `± alone`: the noise of one version on its own (as in latest.md): two separate runs would need a difference that large.

Not paired:
- boltons: only in A (or failed in the other run) — left out
- itsdangerous: only in A (or failed in the other run) — left out
- marshmallow: only in A (or failed in the other run) — left out
- more-itertools: only in A (or failed in the other run) — left out
- pathe: only in A (or failed in the other run) — left out
- ufo: only in A (or failed in the other run) — left out

## All repos

| metric | functions | changed | A | B | B − A, points | 90% interval | ± alone | verdict |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| callers recall | 85 | 0 | 98.9% | 98.9% | +0.0 | +0.0 … +0.0 | ±1.1 | **same** |
| callers precision | 85 | 0 | 94.1% | 94.1% | +0.0 | +0.0 … +0.0 | ±3.6 | **same** |
| callers recall, picked blind | 66 | 0 | 85.3% | 85.3% | +0.0 | +0.0 … +0.0 | ±7.0 | **same** |
| shared names: recall | 108 | 23 | 72.7% | 92.7% | +19.9 | +14.8 … +25.0 | ±6.4 | **better** |
| shared names: recall with maybe | 108 | 0 | 93.6% | 93.6% | +0.0 | +0.0 … +0.0 | ±3.1 | **same** |
| shared names: precision | 86 | 1 | 98.8% | 98.6% | -0.2 | -0.7 … +0.0 | ±1.7 | **can't tell** |
| rename ok (tests pass) | 103 | 0 | 81.6% | 81.6% | +0.0 | +0.0 … +0.0 | ±6.3 | **same** |
| tests recall | 63 | 11 | 68.2% | 71.7% | +3.4 | +1.3 … +5.9 | ±8.0 | **better** |
| tests precision | 63 | 15 | 64.1% | 67.6% | +3.5 | +0.1 … +7.2 | ±7.9 | **better** |
| top-10 that fail | 63 | 7 | 69.6% | 73.3% | +3.7 | +0.4 … +7.5 | ±7.9 | **better** |
| … best possible order | 63 | 6 | 73.6% | 77.6% | +4.0 | +0.7 … +7.8 | ±7.7 | **better** |
| caught by the top 10 | 63 | 4 | 92.1% | 98.4% | +6.3 | +1.6 … +11.1 | ±5.6 | **better** |
| look / file | 103 | 29 | 14.3% | 14.9% | +0.7 | +0.2 … +1.3 | ±3.7 | **better** |

## go

| metric | functions | changed | A | B | B − A, points | 90% interval | ± alone | verdict |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| callers recall | 39 | 0 | 97.6% | 97.6% | +0.0 | +0.0 … +0.0 | ±2.2 | **same** |
| callers precision | 39 | 0 | 93.8% | 93.8% | +0.0 | +0.0 … +0.0 | ±4.9 | **same** |
| callers recall, picked blind | 30 | 0 | 84.3% | 84.3% | +0.0 | +0.0 … +0.0 | ±10.5 | **same** |
| shared names: recall | 90 | 22 | 70.4% | 93.2% | +22.8 | +17.2 … +28.3 | ±7.2 | **better** |
| shared names: recall with maybe | 90 | 0 | 94.3% | 94.3% | +0.0 | +0.0 … +0.0 | ±3.3 | **same** |
| shared names: precision | 69 | 1 | 98.6% | 98.3% | -0.3 | -0.9 … +0.0 | ±2.2 | **can't tell** |
| rename ok (tests pass) | 41 | 0 | 78.0% | 78.0% | +0.0 | +0.0 … +0.0 | ±11.0 | **same** |
| tests recall | 29 | 7 | 62.1% | 66.6% | +4.5 | +0.4 … +9.4 | ±11.9 | **better** |
| tests precision | 29 | 8 | 59.2% | 58.0% | -1.1 | -3.1 … -0.1 | ±11.7 | **worse** |
| top-10 that fail | 29 | 1 | 65.4% | 64.7% | -0.7 | -2.1 … +0.0 | ±11.4 | **can't tell** |
| … best possible order | 29 | 0 | 71.6% | 71.6% | +0.0 | +0.0 … +0.0 | ±11.0 | **same** |
| caught by the top 10 | 29 | 0 | 100.0% | 100.0% | +0.0 | +0.0 … +0.0 | ±0.0 | **same** |
| look / file | 41 | 19 | 11.8% | 11.9% | +0.1 | +0.0 … +0.2 | ±3.5 | **< 0.5 pt** |

## rust

| metric | functions | changed | A | B | B − A, points | 90% interval | ± alone | verdict |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| callers recall | 46 | 0 | 100.0% | 100.0% | +0.0 | +0.0 … +0.0 | ±0.0 | **same** |
| callers precision | 46 | 0 | 94.3% | 94.3% | +0.0 | +0.0 … +0.0 | ±5.0 | **same** |
| callers recall, picked blind | 36 | 0 | 86.1% | 86.1% | +0.0 | +0.0 … +0.0 | ±9.7 | **same** |
| shared names: recall | 18 | 1 | 84.3% | 89.8% | +5.6 | +0.0 … +16.7 | ±11.1 | **can't tell** |
| shared names: recall with maybe | 18 | 0 | 89.8% | 89.8% | +0.0 | +0.0 … +0.0 | ±7.9 | **same** |
| shared names: precision | 17 | 0 | 100.0% | 100.0% | +0.0 | +0.0 … +0.0 | ±0.0 | **same** |
| rename ok (tests pass) | 62 | 0 | 83.9% | 83.9% | +0.0 | +0.0 … +0.0 | ±8.1 | **same** |
| tests recall | 34 | 4 | 73.5% | 76.0% | +2.5 | +0.6 … +4.9 | ±11.1 | **better** |
| tests precision | 34 | 7 | 68.4% | 75.8% | +7.4 | +1.1 … +15.1 | ±10.8 | **better** |
| top-10 that fail | 34 | 6 | 73.2% | 80.7% | +7.5 | +1.2 … +15.1 | ±10.9 | **better** |
| … best possible order | 34 | 6 | 75.3% | 82.7% | +7.5 | +1.2 … +15.1 | ±10.7 | **better** |
| caught by the top 10 | 34 | 4 | 85.3% | 97.1% | +11.8 | +2.9 … +20.6 | ±10.3 | **better** |
| look / file | 62 | 10 | 15.9% | 16.9% | +1.1 | +0.2 … +2.1 | ±5.6 | **better** |

## Per repo (metrics where a function changed)

| repo | metric | functions | changed | A | B | B − A | 90% interval | verdict |
|---|---|---:|---:|---:|---:|---:|---:|---|
| itertools | shared names: recall | 13 | 1 | 88.5% | 96.2% | +7.7 | +0.0 … +23.1 | can't tell |
| itertools | tests recall | 23 | 4 | 60.8% | 64.5% | +3.7 | +0.9 … +7.2 | better |
| itertools | tests precision | 23 | 7 | 65.2% | 76.1% | +11.0 | +1.6 … +21.8 | better |
| itertools | top-10 that fail | 23 | 6 | 65.4% | 76.4% | +11.0 | +1.7 … +21.9 | better |
| itertools | … best possible order | 23 | 6 | 68.4% | 79.5% | +11.0 | +1.7 … +21.9 | better |
| itertools | caught by the top 10 | 23 | 4 | 78.3% | 95.7% | +17.4 | +4.3 … +30.4 | better |
| itertools | look / file | 47 | 10 | 19.5% | 20.9% | +1.4 | +0.3 … +2.7 | better |
| logrus | shared names: recall | 41 | 20 | 48.8% | 93.9% | +45.1 | +34.1 … +57.3 | better |
| logrus | shared names: precision | 22 | 1 | 100.0% | 99.1% | -0.9 | -2.7 … +0.0 | can't tell |
| logrus | tests recall | 5 | 3 | 59.0% | 66.7% | +7.8 | +0.4 … +21.1 | better |
| logrus | tests precision | 5 | 3 | 64.7% | 63.9% | -0.8 | -1.3 … -0.2 | worse |
| logrus | look / file | 6 | 4 | 8.6% | 8.6% | -0.1 | -0.1 … -0.0 | < 0.5 pt |
| mux | tests recall | 4 | 1 | 68.5% | 89.8% | +21.2 | +0.0 … +42.5 | can't tell |
| mux | tests precision | 4 | 1 | 100.0% | 92.6% | -7.4 | -14.7 … +0.0 | can't tell |
| mux | top-10 that fail | 4 | 1 | 100.0% | 95.0% | -5.0 | -10.0 … +0.0 | can't tell |
| mux | look / file | 5 | 1 | 4.4% | 4.7% | +0.3 | +0.0 … +1.0 | can't tell |
| pflag | look / file | 11 | 7 | 10.0% | 10.0% | +0.0 | -0.1 … +0.1 | < 0.5 pt |
| urfave-cli | shared names: recall | 5 | 2 | 60.0% | 100.0% | +40.0 | +0.0 … +80.0 | can't tell |
| urfave-cli | tests recall | 13 | 3 | 40.9% | 41.5% | +0.6 | +0.1 … +1.1 | better |
| urfave-cli | tests precision | 13 | 4 | 41.9% | 41.9% | -0.0 | -0.0 … +0.0 | < 0.5 pt |
| urfave-cli | look / file | 19 | 7 | 15.9% | 15.9% | +0.1 | +0.0 … +0.1 | < 0.5 pt |

## Functions that changed (up to 10 per metric, largest first)

- **shared names: recall** (23): itertools `itertools::Itertools::minmax_by` 0% → 100%; logrus `main::Logger::Panicln` 0% → 100%; logrus `main::Logger::Infoln` 0% → 100%; logrus `main::Logger::Errorln` 0% → 100%; logrus `main::Logger::Info` 0% → 100%; logrus `main::Logger::WithField` 0% → 100%; logrus `main::Logger::Println` 0% → 100%; logrus `main::Logger::WithContext` 0% → 100%; logrus `main::Logger::Debugf` 0% → 100%; logrus `main::Logger::Error` 0% → 100%
- **shared names: precision** (1): logrus `main::Level::String` 100% → 80%
- **tests recall** (11): mux `main::Route::GetHandlerWithMiddlewares` 5% → 90%; logrus `main::NewEntry` 50% → 85%; itertools `itertools::size_hint::mul_scalar` 0% → 33%; itertools `itertools::adaptors::checked_binomial` 0% → 27%; itertools `itertools::minmax::minmax_impl` 0% → 20%; itertools `itertools::lazy_buffer::LazyBuffer::get_at` 0% → 6%; urfave-cli `main::Command::Walk` 93% → 96%; urfave-cli `main::Command::Lineage` 28% → 30%; urfave-cli `main::HandleExitCoder` 98% → 100%; logrus `main::mutexWrap::Lock` 60% → 62%
- **tests precision** (15): itertools `itertools::minmax::minmax_impl` 0% → 100%; itertools `itertools::adaptors::checked_binomial` 0% → 80%; itertools `itertools::lazy_buffer::LazyBuffer::get_at` 0% → 50%; itertools `itertools::size_hint::mul_scalar` 0% → 50%; mux `main::Route::GetHandlerWithMiddlewares` 100% → 71%; itertools `itertools::heap::sift_down` 100% → 83%; itertools `itertools::next_array::ArrayBuilder::push` 60% → 50%; logrus `main::mutexWrap::Lock` 84% → 82%; logrus `main::mutexWrap::Unlock` 84% → 82%; itertools `itertools::next_array::ArrayBuilder::take` 7% → 6%
- **top-10 that fail** (7): itertools `itertools::minmax::minmax_impl` 0% → 100%; itertools `itertools::adaptors::checked_binomial` 0% → 80%; itertools `itertools::lazy_buffer::LazyBuffer::get_at` 0% → 50%; itertools `itertools::size_hint::mul_scalar` 0% → 50%; mux `main::Route::GetHandlerWithMiddlewares` 100% → 80%; itertools `itertools::heap::sift_down` 100% → 83%; itertools `itertools::next_array::ArrayBuilder::push` 60% → 50%
- **… best possible order** (6): itertools `itertools::minmax::minmax_impl` 0% → 100%; itertools `itertools::adaptors::checked_binomial` 0% → 80%; itertools `itertools::lazy_buffer::LazyBuffer::get_at` 0% → 50%; itertools `itertools::size_hint::mul_scalar` 0% → 50%; itertools `itertools::heap::sift_down` 100% → 83%; itertools `itertools::next_array::ArrayBuilder::push` 60% → 50%
- **caught by the top 10** (4): itertools `itertools::adaptors::checked_binomial` 0% → 100%; itertools `itertools::lazy_buffer::LazyBuffer::get_at` 0% → 100%; itertools `itertools::size_hint::mul_scalar` 0% → 100%; itertools `itertools::minmax::minmax_impl` 0% → 100%
- **look / file** (29): itertools `itertools::lazy_buffer::LazyBuffer::get_next` 50% → 78%; itertools `itertools::size_hint::add_scalar` 41% → 58%; itertools `itertools::lazy_buffer::LazyBuffer::get_at` 29% → 40%; itertools `itertools::size_hint::mul_scalar` 26% → 31%; itertools `itertools::minmax::minmax_impl` 72% → 74%; mux `main::Route::GetHandlerWithMiddlewares` 4% → 5%; itertools `itertools::adaptors::checked_binomial` 4% → 5%; itertools `itertools::either_or_both::EitherOrBoth::or_else` 9% → 10%; itertools `itertools::heap::sift_down` 114% → 115%; urfave-cli `main::ValueSourceChain::EnvKeys` 16% → 17%
