# learn-tests на json-java (java): сводка

Все функции (pick + случайные). Полнота / точность / из 10 падают / поймано первыми 10. По методу — как истина eval (имя метода теста); по классу — класс, где есть падающий метод.

| способ | единица | функций | граф (`tests-for`) | покрытие | объединение | карта, с | × обычного |
|---|---|---:|---|---|---|---:|---:|
| per-test | метод | 34 | 71% / 48% / 61% / 97% | 99% / 94% / 95% / 100% | 100% / 52% / 81% / 100% | 10.5 | 2.1 |
| per-test | класс | 34 | 78% / 77% / 77% / 100% | 100% / 97% / 97% / 100% | 100% / 77% / 77% / 100% | 10.5 | 2.1 |
| per-test-fork | метод | 34 | 71% / 48% / 61% / 97% | 99% / 94% / 95% / 100% | 100% / 52% / 81% / 100% | 20.2 | 4.1 |
| per-test-fork | класс | 34 | 78% / 77% / 77% / 100% | 100% / 97% / 97% / 100% | 100% / 77% / 77% / 100% | 20.2 | 4.1 |
| per-class | метод | 34 | 71% / 48% / 61% / 97% | 100% / 21% / 32% / 76% | 100% / 20% / 60% / 97% | 72.6 | 16.1 |
| per-class | класс | 34 | 78% / 77% / 77% / 100% | 100% / 77% / 77% / 100% | 100% / 73% / 73% / 100% | 72.6 | 16.1 |

# learn-tests (per-test) на json-java (java): покрытие по тестам против `tests-for`
json-java 2cd0a8c2112a, gitnext 14bcb17, seed 7. Истина — eval (тесты, падающие при throw в начале функции). pick — функции с ответом в кэше eval, random — случайные.

| выборка | функций | источник | полнота | точность | из 10 падают | поймано |
|---|---:|---|---:|---:|---:|---:|
| pick (кэш eval) | 22 | gitnext | 70% | 52% | 66% | 100% |
| pick (кэш eval) | 22 | coverage | 100% | 96% | 98% | 100% |
| pick (кэш eval) | 22 | union | 100% | 54% | 83% | 100% |
| случайные | 12 | gitnext | 72% | 42% | 52% | 92% |
| случайные | 12 | coverage | 98% | 90% | 89% | 100% |
| случайные | 12 | union | 100% | 47% | 76% | 100% |
| все | 34 | gitnext | 71% | 48% | 61% | 97% |
| все | 34 | coverage | 99% | 94% | 95% | 100% |
| все | 34 | union | 100% | 52% | 81% | 100% |

## По функциям

| выборка | функция | падают | граф: полн / из 10 | покрытие: полн / точн / из 10 | мимо покрытия |
|---|---|---:|---|---|---|
| pick | `org::json::JSONObject::numberToString` | 46 | 20% / 50% | 100% / 98% / 100% |  |
| pick | `org::json::JSONObject::putOnce` | 5 | 100% / 40% | 100% / 100% / 100% |  |
| pick | `org::json::JSONObject::putOpt` | 3 | 67% / 10% | 100% / 100% / 100% |  |
| pick | `org::json::JSONObject::toMap` | 2 | 100% / 20% | 100% / 100% / 100% |  |
| pick | `org::json::JSONParserConfiguration::getMaxSharedKeys` | 224 | 17% / 100% | 100% / 92% / 100% | setJsonObject |
| pick | `org::json::JSONParserConfiguration::isStrictMode` | 268 | 5% / 100% | 100% / 93% / 100% | setJsonObject |
| pick | `org::json::JSONPointer::queryFrom` | 70 | 100% / 100% | 100% / 100% / 100% |  |
| pick | `org::json::JSONTokener::getJsonParserConfiguration` | 271 | 2% / 100% | 100% / 93% / 100% | setJsonObject |
| pick | `org::json::JSONTokener::more` | 136 | 100% / 80% | 100% / 100% / 100% |  |
| pick | `org::json::JSONTokener::nextClean` | 271 | 2% / 100% | 100% / 93% / 100% | setJsonObject |
| pick | `org::json::JSONTokener::nextString` | 235 | 2% / 100% | 100% / 92% / 100% | setJsonObject |
| pick | `org::json::ParserConfiguration::getMaxNumberLength` | 82 | 33% / 70% | 100% / 46% / 60% |  |
| pick | `org::json::ParserConfiguration::isKeepStrings` | 34 | 100% / 80% | 100% / 100% / 100% |  |
| pick | `org::json::XML::noSpace` | 12 | 100% / 80% | 100% / 100% / 100% |  |
| pick | `org::json::XMLParserConfiguration::getForceList` | 64 | 100% / 100% | 100% / 100% / 100% |  |
| pick | `org::json::XMLParserConfiguration::getXsiTypeMap` | 22 | 100% / 60% | 100% / 100% / 100% |  |
| pick | `org::json::XMLParserConfiguration::isConvertNilAttributeToNull` | 22 | 100% / 50% | 100% / 100% / 100% |  |
| pick | `org::json::XMLParserConfiguration::shouldTrimWhiteSpace` | 93 | 100% / 60% | 100% / 100% / 100% |  |
| pick | `org::json::XMLParserConfiguration::withKeepBooleanAsString` | 2 | 100% / 20% | 100% / 100% / 100% |  |
| pick | `org::json::XMLTokener::nextCDATA` | 5 | 100% / 20% | 100% / 100% / 100% |  |
| pick | `org::json::XMLTokener::nextMeta` | 6 | 100% / 20% | 100% / 100% / 100% |  |
| pick | `org::json::XMLTokener::skipPast` | 88 | 100% / 80% | 100% / 100% / 100% |  |
| random | `org::json::JSONObject::optJSONObject` | 3 | 100% / 20% | 100% / 100% / 100% |  |
| random | `org::json::JSONParserConfiguration::withMaxSharedKeys` | 5 | 100% / 100% | 100% / 100% / 100% |  |
| random | `org::json::XMLTokener::parseHexEntity` | 16 | 19% / 43% | 100% / 100% / 100% |  |
| random | `org::json::JSONObject::checkKeyForStrictMode` | 223 | 30% / 60% | 100% / 92% / 100% | setJsonObject |
| random | `org::json::JSONML::toString` | 4 | 100% / 20% | 100% / 29% / 10% |  |
| random | `org::json::JSONWriter::array` | 4 | 100% / 40% | 100% / 100% / 100% |  |
| random | `org::json::CDL::rowToJSONObject` | 13 | 100% / 80% | 77% / 100% / 100% | badEscapedQuote, unbalancedEscapedQuote, unbalancedQuoteInValue |
| random | `org::json::JSONArray::getJSONObject` | 22 | 100% / 100% | 100% / 100% / 100% |  |
| random | `org::json::XML::indent` | 27 | 15% / 67% | 100% / 100% / 100% |  |
| random | `org::json::JSONObject::optEnum` | 3 | 100% / 30% | 100% / 100% / 100% |  |
| random | `org::json::JSONObject::getKeyNameFromMethod` | 32 | 75% / 0% | 100% / 100% / 100% |  |
| random | `org::json::JSONObject::isDecimalNotation` | 87 | 29% / 70% | 100% / 56% / 60% |  |

## Цена

- `how`: per-test
- `class_inits_s`: 0.5
- `class_inits_booked`: True
- `files_with_init_lines`: 5
- `mvn_s`: 6.1
- `analyze_s`: 3.8
- `map_s`: 10.5
- `exec_bytes`: 503756
- `dumps`: 1686
- `test_methods`: 634
- `classes`: 22
- `classes_with_setup_lines`: 1
- `run_failed`: False
- `names_in_several_classes`: 46
- `junit`: 4
- `package`: org.json
- `owner`: body
- `repeat`: 3
- `plain_s`: 5.1
- `tests`: 634
- `map_pairs`: 18211
- `map_functions`: 311
- `map_sqlite_bytes`: 233472
- `functions_with_body`: 510
- `functions_never_run`: 16
- `fail_alone_not_together`: []
- `fail_together_not_alone`: []
- `slowdown_runs`: 2.1

# learn-tests (per-test-fork) на json-java (java): покрытие по тестам против `tests-for`
json-java 2cd0a8c2112a, gitnext 14bcb17, seed 7. Истина — eval (тесты, падающие при throw в начале функции). pick — функции с ответом в кэше eval, random — случайные.

| выборка | функций | источник | полнота | точность | из 10 падают | поймано |
|---|---:|---|---:|---:|---:|---:|
| pick (кэш eval) | 22 | gitnext | 70% | 52% | 66% | 100% |
| pick (кэш eval) | 22 | coverage | 100% | 96% | 98% | 100% |
| pick (кэш eval) | 22 | union | 100% | 54% | 83% | 100% |
| случайные | 12 | gitnext | 72% | 42% | 52% | 92% |
| случайные | 12 | coverage | 98% | 90% | 89% | 100% |
| случайные | 12 | union | 100% | 47% | 76% | 100% |
| все | 34 | gitnext | 71% | 48% | 61% | 97% |
| все | 34 | coverage | 99% | 94% | 95% | 100% |
| все | 34 | union | 100% | 52% | 81% | 100% |

## По функциям

| выборка | функция | падают | граф: полн / из 10 | покрытие: полн / точн / из 10 | мимо покрытия |
|---|---|---:|---|---|---|
| pick | `org::json::JSONObject::numberToString` | 46 | 20% / 50% | 100% / 98% / 100% |  |
| pick | `org::json::JSONObject::putOnce` | 5 | 100% / 40% | 100% / 100% / 100% |  |
| pick | `org::json::JSONObject::putOpt` | 3 | 67% / 10% | 100% / 100% / 100% |  |
| pick | `org::json::JSONObject::toMap` | 2 | 100% / 20% | 100% / 100% / 100% |  |
| pick | `org::json::JSONParserConfiguration::getMaxSharedKeys` | 224 | 17% / 100% | 100% / 92% / 100% | setJsonObject |
| pick | `org::json::JSONParserConfiguration::isStrictMode` | 268 | 5% / 100% | 100% / 93% / 100% | setJsonObject |
| pick | `org::json::JSONPointer::queryFrom` | 70 | 100% / 100% | 100% / 100% / 100% |  |
| pick | `org::json::JSONTokener::getJsonParserConfiguration` | 271 | 2% / 100% | 100% / 93% / 100% | setJsonObject |
| pick | `org::json::JSONTokener::more` | 136 | 100% / 80% | 100% / 100% / 100% |  |
| pick | `org::json::JSONTokener::nextClean` | 271 | 2% / 100% | 100% / 93% / 100% | setJsonObject |
| pick | `org::json::JSONTokener::nextString` | 235 | 2% / 100% | 100% / 92% / 100% | setJsonObject |
| pick | `org::json::ParserConfiguration::getMaxNumberLength` | 82 | 33% / 70% | 100% / 46% / 60% |  |
| pick | `org::json::ParserConfiguration::isKeepStrings` | 34 | 100% / 80% | 100% / 100% / 100% |  |
| pick | `org::json::XML::noSpace` | 12 | 100% / 80% | 100% / 100% / 100% |  |
| pick | `org::json::XMLParserConfiguration::getForceList` | 64 | 100% / 100% | 100% / 100% / 100% |  |
| pick | `org::json::XMLParserConfiguration::getXsiTypeMap` | 22 | 100% / 60% | 100% / 100% / 100% |  |
| pick | `org::json::XMLParserConfiguration::isConvertNilAttributeToNull` | 22 | 100% / 50% | 100% / 100% / 100% |  |
| pick | `org::json::XMLParserConfiguration::shouldTrimWhiteSpace` | 93 | 100% / 60% | 100% / 100% / 100% |  |
| pick | `org::json::XMLParserConfiguration::withKeepBooleanAsString` | 2 | 100% / 20% | 100% / 100% / 100% |  |
| pick | `org::json::XMLTokener::nextCDATA` | 5 | 100% / 20% | 100% / 100% / 100% |  |
| pick | `org::json::XMLTokener::nextMeta` | 6 | 100% / 20% | 100% / 100% / 100% |  |
| pick | `org::json::XMLTokener::skipPast` | 88 | 100% / 80% | 100% / 100% / 100% |  |
| random | `org::json::JSONObject::optJSONObject` | 3 | 100% / 20% | 100% / 100% / 100% |  |
| random | `org::json::JSONParserConfiguration::withMaxSharedKeys` | 5 | 100% / 100% | 100% / 100% / 100% |  |
| random | `org::json::XMLTokener::parseHexEntity` | 16 | 19% / 43% | 100% / 100% / 100% |  |
| random | `org::json::JSONObject::checkKeyForStrictMode` | 223 | 30% / 60% | 100% / 92% / 100% | setJsonObject |
| random | `org::json::JSONML::toString` | 4 | 100% / 20% | 100% / 29% / 10% |  |
| random | `org::json::JSONWriter::array` | 4 | 100% / 40% | 100% / 100% / 100% |  |
| random | `org::json::CDL::rowToJSONObject` | 13 | 100% / 80% | 77% / 100% / 100% | badEscapedQuote, unbalancedEscapedQuote, unbalancedQuoteInValue |
| random | `org::json::JSONArray::getJSONObject` | 22 | 100% / 100% | 100% / 100% / 100% |  |
| random | `org::json::XML::indent` | 27 | 15% / 67% | 100% / 100% / 100% |  |
| random | `org::json::JSONObject::optEnum` | 3 | 100% / 30% | 100% / 100% / 100% |  |
| random | `org::json::JSONObject::getKeyNameFromMethod` | 32 | 75% / 0% | 100% / 100% / 100% |  |
| random | `org::json::JSONObject::isDecimalNotation` | 87 | 29% / 70% | 100% / 56% / 60% |  |

## Цена

- `how`: per-test-fork
- `class_inits_s`: 0.5
- `class_inits_booked`: True
- `files_with_init_lines`: 5
- `mvn_s`: 16.6
- `analyze_s`: 3.1
- `map_s`: 20.2
- `exec_bytes`: 504392
- `dumps`: 1686
- `test_methods`: 634
- `classes`: 22
- `classes_with_setup_lines`: 1
- `run_failed`: False
- `names_in_several_classes`: 46
- `junit`: 4
- `package`: org.json
- `owner`: body
- `repeat`: 3
- `plain_s`: 4.9
- `tests`: 634
- `map_pairs`: 18214
- `map_functions`: 311
- `map_sqlite_bytes`: 233472
- `functions_with_body`: 510
- `functions_never_run`: 16
- `fail_alone_not_together`: []
- `fail_together_not_alone`: []
- `slowdown_runs`: 4.1

# learn-tests (per-class) на json-java (java): покрытие по тестам против `tests-for`
json-java 2cd0a8c2112a, gitnext 14bcb17, seed 7. Истина — eval (тесты, падающие при throw в начале функции). pick — функции с ответом в кэше eval, random — случайные.

| выборка | функций | источник | полнота | точность | из 10 падают | поймано |
|---|---:|---|---:|---:|---:|---:|
| pick (кэш eval) | 22 | gitnext | 70% | 52% | 66% | 100% |
| pick (кэш eval) | 22 | coverage | 100% | 23% | 33% | 82% |
| pick (кэш eval) | 22 | union | 100% | 23% | 67% | 100% |
| случайные | 12 | gitnext | 72% | 42% | 52% | 92% |
| случайные | 12 | coverage | 100% | 18% | 31% | 67% |
| случайные | 12 | union | 100% | 15% | 48% | 92% |
| все | 34 | gitnext | 71% | 48% | 61% | 97% |
| все | 34 | coverage | 100% | 21% | 32% | 76% |
| все | 34 | union | 100% | 20% | 60% | 97% |

## По функциям

| выборка | функция | падают | граф: полн / из 10 | покрытие: полн / точн / из 10 | мимо покрытия |
|---|---|---:|---|---|---|
| pick | `org::json::JSONObject::numberToString` | 46 | 20% / 50% | 100% / 12% / 20% |  |
| pick | `org::json::JSONObject::putOnce` | 5 | 100% / 40% | 100% / 3% / 40% |  |
| pick | `org::json::JSONObject::putOpt` | 3 | 67% / 10% | 100% / 2% / 20% |  |
| pick | `org::json::JSONObject::toMap` | 2 | 100% / 20% | 100% / 1% / 0% |  |
| pick | `org::json::JSONParserConfiguration::getMaxSharedKeys` | 224 | 17% / 100% | 100% / 42% / 70% | setJsonObject |
| pick | `org::json::JSONParserConfiguration::isStrictMode` | 268 | 5% / 100% | 100% / 51% / 80% | setJsonObject |
| pick | `org::json::JSONPointer::queryFrom` | 70 | 100% / 100% | 100% / 25% / 50% |  |
| pick | `org::json::JSONTokener::getJsonParserConfiguration` | 271 | 2% / 100% | 100% / 51% / 80% | setJsonObject |
| pick | `org::json::JSONTokener::more` | 136 | 100% / 80% | 100% / 39% / 50% |  |
| pick | `org::json::JSONTokener::nextClean` | 271 | 2% / 100% | 100% / 51% / 80% | setJsonObject |
| pick | `org::json::JSONTokener::nextString` | 235 | 2% / 100% | 100% / 44% / 70% | setJsonObject |
| pick | `org::json::ParserConfiguration::getMaxNumberLength` | 82 | 33% / 70% | 100% / 17% / 40% |  |
| pick | `org::json::ParserConfiguration::isKeepStrings` | 34 | 100% / 80% | 100% / 17% / 0% |  |
| pick | `org::json::XML::noSpace` | 12 | 100% / 80% | 100% / 10% / 20% |  |
| pick | `org::json::XMLParserConfiguration::getForceList` | 64 | 100% / 100% | 100% / 24% / 20% |  |
| pick | `org::json::XMLParserConfiguration::getXsiTypeMap` | 22 | 100% / 60% | 100% / 22% / 0% |  |
| pick | `org::json::XMLParserConfiguration::isConvertNilAttributeToNull` | 22 | 100% / 50% | 100% / 22% / 0% |  |
| pick | `org::json::XMLParserConfiguration::shouldTrimWhiteSpace` | 93 | 100% / 60% | 100% / 30% / 20% |  |
| pick | `org::json::XMLParserConfiguration::withKeepBooleanAsString` | 2 | 100% / 20% | 100% / 4% / 10% |  |
| pick | `org::json::XMLTokener::nextCDATA` | 5 | 100% / 20% | 100% / 3% / 10% |  |
| pick | `org::json::XMLTokener::nextMeta` | 6 | 100% / 20% | 100% / 4% / 20% |  |
| pick | `org::json::XMLTokener::skipPast` | 88 | 100% / 80% | 100% / 28% / 30% |  |
| random | `org::json::JSONObject::optJSONObject` | 3 | 100% / 20% | 100% / 2% / 0% |  |
| random | `org::json::JSONParserConfiguration::withMaxSharedKeys` | 5 | 100% / 100% | 100% / 8% / 0% |  |
| random | `org::json::XMLTokener::parseHexEntity` | 16 | 19% / 43% | 100% / 11% / 0% |  |
| random | `org::json::JSONObject::checkKeyForStrictMode` | 223 | 30% / 60% | 100% / 42% / 70% | setJsonObject |
| random | `org::json::JSONML::toString` | 4 | 100% / 20% | 100% / 3% / 0% |  |
| random | `org::json::JSONWriter::array` | 4 | 100% / 40% | 100% / 40% / 40% |  |
| random | `org::json::CDL::rowToJSONObject` | 13 | 100% / 80% | 100% / 62% / 40% |  |
| random | `org::json::JSONArray::getJSONObject` | 22 | 100% / 100% | 100% / 7% / 30% |  |
| random | `org::json::XML::indent` | 27 | 15% / 67% | 100% / 10% / 70% |  |
| random | `org::json::JSONObject::optEnum` | 3 | 100% / 30% | 100% / 2% / 10% |  |
| random | `org::json::JSONObject::getKeyNameFromMethod` | 32 | 75% / 0% | 100% / 17% / 50% |  |
| random | `org::json::JSONObject::isDecimalNotation` | 87 | 29% / 70% | 100% / 18% / 60% |  |

## Цена

- `how`: per-class
- `mvn_s`: 72.2
- `analyze_s`: 0.4
- `map_s`: 72.6
- `exec_bytes`: 16999
- `dumps`: 22
- `test_methods`: 640
- `classes`: 22
- `names_in_several_classes`: 47
- `junit`: 4
- `package`: org.json
- `owner`: body
- `repeat`: 3
- `plain_s`: 4.5
- `tests`: 640
- `map_pairs`: 75187
- `map_functions`: 311
- `map_sqlite_bytes`: 774144
- `functions_with_body`: 510
- `functions_never_run`: 16
- `fail_alone_not_together`: []
- `fail_together_not_alone`: []
- `slowdown_runs`: 16.1

# По классу теста

Класс засчитан, если в нём есть метод с падающим именем (имя в нескольких классах — все они).

| способ | выборка | функций | источник | полнота | точность | из 10 падают | поймано |
|---|---|---:|---|---:|---:|---:|---:|
| per-test | pick | 22 | gitnext | 79% | 84% | 84% | 100% |
| per-test | pick | 22 | coverage | 100% | 98% | 98% | 100% |
| per-test | pick | 22 | union | 100% | 83% | 82% | 100% |
| per-test | random | 12 | gitnext | 76% | 62% | 63% | 100% |
| per-test | random | 12 | coverage | 100% | 95% | 95% | 100% |
| per-test | random | 12 | union | 100% | 66% | 66% | 100% |
| per-test | all | 34 | gitnext | 78% | 77% | 77% | 100% |
| per-test | all | 34 | coverage | 100% | 97% | 97% | 100% |
| per-test | all | 34 | union | 100% | 77% | 77% | 100% |
| per-test-fork | pick | 22 | gitnext | 79% | 84% | 84% | 100% |
| per-test-fork | pick | 22 | coverage | 100% | 98% | 98% | 100% |
| per-test-fork | pick | 22 | union | 100% | 83% | 82% | 100% |
| per-test-fork | random | 12 | gitnext | 76% | 62% | 63% | 100% |
| per-test-fork | random | 12 | coverage | 100% | 95% | 95% | 100% |
| per-test-fork | random | 12 | union | 100% | 66% | 66% | 100% |
| per-test-fork | all | 34 | gitnext | 78% | 77% | 77% | 100% |
| per-test-fork | all | 34 | coverage | 100% | 97% | 97% | 100% |
| per-test-fork | all | 34 | union | 100% | 77% | 77% | 100% |
| per-class | pick | 22 | gitnext | 79% | 84% | 84% | 100% |
| per-class | pick | 22 | coverage | 100% | 79% | 79% | 100% |
| per-class | pick | 22 | union | 100% | 79% | 78% | 100% |
| per-class | random | 12 | gitnext | 76% | 62% | 63% | 100% |
| per-class | random | 12 | coverage | 100% | 74% | 74% | 100% |
| per-class | random | 12 | union | 100% | 63% | 63% | 100% |
| per-class | all | 34 | gitnext | 78% | 77% | 77% | 100% |
| per-class | all | 34 | coverage | 100% | 77% | 77% | 100% |
| per-class | all | 34 | union | 100% | 73% | 73% | 100% |

# Цена

| | per-test | per-test-fork | per-class |
|---|---:|---:|---:|
| plain_s | 5.1 | 4.9 | 4.5 |
| mvn_s | 6.1 | 16.6 | 72.2 |
| analyze_s | 3.8 | 3.1 | 0.4 |
| map_s | 10.5 | 20.2 | 72.6 |
| slowdown_runs | 2.1 | 4.1 | 16.1 |
| exec_bytes | 503756 | 504392 | 16999 |
| map_sqlite_bytes | 233472 | 233472 | 774144 |
| dumps | 1686 | 1686 | 22 |
| test_methods | 634 | 634 | 640 |
| classes | 22 | 22 | 22 |
| names_in_several_classes | 46 | 46 | 47 |
| map_pairs | 18211 | 18214 | 75187 |
| map_functions | 311 | 311 | 311 |
| functions_with_body | 510 | 510 | 510 |
| functions_never_run | 16 | 16 | 16 |

json-java 2cd0a8c2112a
