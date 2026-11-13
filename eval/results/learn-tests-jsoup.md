# learn-tests на jsoup (java): сводка

Все функции (pick + случайные). Полнота / точность / из 10 падают / поймано первыми 10. По методу — как истина eval (имя метода теста); по классу — класс, где есть падающий метод.

| способ | единица | функций | граф (`tests-for`) | покрытие | объединение | карта, с | × обычного |
|---|---|---:|---|---|---|---:|---:|
| per-test | метод | 33 | 29% / 64% / 66% / 88% | 94% / 94% / 94% / 97% | 96% / 83% / 92% / 97% | 49.6 | 3.5 |
| per-test | класс | 33 | 40% / 81% / 82% / 88% | 98% / 95% / 96% / 97% | 99% / 91% / 92% / 97% | 49.6 | 3.5 |
| per-test-fork | метод | 33 | 29% / 64% / 66% / 88% | 94% / 94% / 94% / 97% | 96% / 83% / 92% / 97% | 139.0 | 8.4 |
| per-test-fork | класс | 33 | 40% / 81% / 82% / 88% | 100% / 95% / 96% / 97% | 100% / 91% / 92% / 97% | 139.0 | 8.4 |
| per-class | метод | 33 | 29% / 64% / 66% / 88% | 100% / 36% / 33% / 79% | 100% / 36% / 67% / 94% | 416.0 | 27.2 |
| per-class | класс | 33 | 40% / 81% / 82% / 88% | 100% / 74% / 80% / 97% | 100% / 73% / 80% / 97% | 416.0 | 27.2 |

# learn-tests (per-test) на jsoup (java): покрытие по тестам против `tests-for`
jsoup d90d57521d93, gitnext 14bcb17, seed 7. Истина — eval (тесты, падающие при throw в начале функции). pick — функции с ответом в кэше eval, random — случайные.

| выборка | функций | источник | полнота | точность | из 10 падают | поймано |
|---|---:|---|---:|---:|---:|---:|
| pick (кэш eval) | 21 | gitnext | 26% | 70% | 69% | 90% |
| pick (кэш eval) | 21 | coverage | 91% | 100% | 100% | 100% |
| pick (кэш eval) | 21 | union | 94% | 95% | 100% | 100% |
| случайные | 12 | gitnext | 34% | 53% | 62% | 83% |
| случайные | 12 | coverage | 100% | 85% | 86% | 92% |
| случайные | 12 | union | 100% | 61% | 78% | 92% |
| все | 33 | gitnext | 29% | 64% | 66% | 88% |
| все | 33 | coverage | 94% | 94% | 94% | 97% |
| все | 33 | union | 96% | 83% | 92% | 97% |

## По функциям

| выборка | функция | падают | граф: полн / из 10 | покрытие: полн / точн / из 10 | мимо покрытия |
|---|---|---:|---|---|---|
| pick | `org::jsoup::internal::LegacyMatching::cssAttributeMatches` | 7 | 0% / 0% | 100% / 100% / 100% |  |
| pick | `org::jsoup::internal::Normalizer::lowerCase` | 1689 | 70% / 80% | 18% / 100% / 100% | DocumentsWithSameContentAreEqual, DocumentsWithSameContentAreVerifiable, NoErrorOnEofIfBodyOpen, absAttr |
| pick | `org::jsoup::internal::SoftPool::release` | 1755 | 46% / 60% | 98% / 100% / 90% | adoptionDoesNotAddSecondDocumentElement, canCreateEmptyConnection, caseInsensitiveHeaders, downgradesSocksProxyToUrlConnectionExecutor |
| pick | `org::jsoup::internal::StringUtil::appendNormalisedWhitespace` | 446 | 29% / 100% | 100% / 100% / 100% |  |
| pick | `org::jsoup::internal::StringUtil::isActuallyWhitespace` | 446 | 28% / 100% | 100% / 100% / 100% |  |
| pick | `org::jsoup::internal::StringUtil::isAsciiLetter` | 1699 | 2% / 90% | 100% / 100% / 100% | init, provideEvaluators, rcDataRecoverParser |
| pick | `org::jsoup::internal::StringUtil::trimAsciiWhitespace` | 1811 | 8% / 100% | 76% / 100% / 100% | DocumentsWithSameContentAreEqual, DocumentsWithSameContentAreVerifiable, NoErrorOnEofIfBodyOpen, acceptsLenientXmlProcessingInstructionTarget |
| pick | `org::jsoup::nodes::Attributes::deduplicate` | 780 | 1% / 100% | 100% / 100% / 100% | init, provideEvaluators |
| pick | `org::jsoup::nodes::Attributes::getIgnoreCase` | 610 | 35% / 100% | 100% / 100% / 100% |  |
| pick | `org::jsoup::nodes::Attributes::removeIgnoreCase` | 28 | 7% / 10% | 100% / 100% / 100% |  |
| pick | `org::jsoup::nodes::DocumentType::publicId` | 115 | 3% / 43% | 100% / 100% / 100% |  |
| pick | `org::jsoup::nodes::Entities::isNamedEntity` | 51 | 16% / 30% | 100% / 100% / 100% |  |
| pick | `org::jsoup::parser::Parser::getMaxDepth` | 1467 | 0% / 40% | 100% / 100% / 100% | init, provideEvaluators |
| pick | `org::jsoup::parser::Tag::isFormSubmittable` | 16 | 100% / 100% | 100% / 100% / 100% |  |
| pick | `org::jsoup::parser::Token::Doctype::getSystemIdentifier` | 130 | 0% / 0% | 100% / 100% / 100% |  |
| pick | `org::jsoup::parser::TokenQueue::consumeCssIdentifier` | 383 | 2% / 90% | 100% / 99% / 100% |  |
| pick | `org::jsoup::parser::TokenQueue::matchesWhitespace` | 1721 | 7% / 90% | 59% / 100% / 100% | DocumentsWithSameContentAreEqual, DocumentsWithSameContentAreVerifiable, NoErrorOnEofIfBodyOpen, acceptsLenientXmlProcessingInstructionTarget |
| pick | `org::jsoup::parser::TokenQueue::matchesWord` | 1717 | 0% / 30% | 59% / 100% / 100% | DocumentsWithSameContentAreEqual, DocumentsWithSameContentAreVerifiable, NoErrorOnEofIfBodyOpen, acceptsLenientXmlProcessingInstructionTarget |
| pick | `org::jsoup::safety::Safelist::addEnforcedAttribute` | 38 | 100% / 100% | 100% / 100% / 100% |  |
| pick | `org::jsoup::safety::Safelist::addTags` | 72 | 99% / 100% | 100% / 100% / 100% |  |
| pick | `org::jsoup::select::Evaluator::asPredicate` | 957 | 2% / 90% | 100% / 100% / 100% |  |
| random | `org::jsoup::nodes::Node::ownerDocument` | 891 | 85% / 90% | 100% / 100% / 100% |  |
| random | `org::jsoup::nodes::NodeInternals::attributeRange` | 39 | 0% / 0% | 100% / 56% / 30% |  |
| random | `org::jsoup::parser::TokeniserState::read` | 1 | 0% / 0% | 100% / 0% / 0% |  |
| random | `org::jsoup::helper::HttpConnection::Response::sameOrigin` | 19 | 11% / 20% | 100% / 100% / 100% |  |
| random | `org::jsoup::nodes::Node::outerHtml` | 116 | 89% / 100% | 100% / 65% / 100% |  |
| random | `org::jsoup::nodes::Element::TextAccumulator::hasBlockChild` | 123 | 21% / 60% | 100% / 100% / 100% |  |
| random | `org::jsoup::nodes::Node::setParentNode` | 1554 | 21% / 70% | 100% / 100% / 100% | init, provideEvaluators |
| random | `org::jsoup::nodes::Document::text` | 1 | 100% / 10% | 100% / 100% / 100% |  |
| random | `org::jsoup::nodes::DataNode::outerHtmlHead` | 37 | 19% / 100% | 100% / 100% / 100% |  |
| random | `org::jsoup::parser::Token::Character::normalizeNulls` | 1090 | 0% / 100% | 100% / 100% / 100% | init, provideEvaluators |
| random | `org::jsoup::parser::TreeBuilder::parseAttributes` | 29 | 59% / 90% | 100% / 100% / 100% |  |
| random | `org::jsoup::nodes::Entities::encoderFor` | 573 | 2% / 100% | 100% / 98% / 100% |  |

## Цена

- `how`: per-test
- `class_inits_s`: 3.6
- `class_inits_booked`: True
- `files_with_init_lines`: 64
- `mvn_s`: 20.2
- `analyze_s`: 25.7
- `map_s`: 49.6
- `exec_bytes`: 8545291
- `dumps`: 5372
- `test_methods`: 2130
- `classes`: 76
- `classes_with_setup_lines`: 6
- `run_failed`: False
- `names_in_several_classes`: 115
- `junit`: 5
- `package`: org.jsoup
- `owner`: body
- `repeat`: 3
- `plain_s`: 14.2
- `tests`: 2130
- `map_pairs`: 556456
- `map_functions`: 1771
- `map_sqlite_bytes`: 5718016
- `functions_with_body`: 2299
- `functions_never_run`: 99
- `fail_alone_not_together`: []
- `fail_together_not_alone`: []
- `slowdown_runs`: 3.5

# learn-tests (per-test-fork) на jsoup (java): покрытие по тестам против `tests-for`
jsoup d90d57521d93, gitnext 14bcb17, seed 7. Истина — eval (тесты, падающие при throw в начале функции). pick — функции с ответом в кэше eval, random — случайные.

| выборка | функций | источник | полнота | точность | из 10 падают | поймано |
|---|---:|---|---:|---:|---:|---:|
| pick (кэш eval) | 21 | gitnext | 26% | 70% | 69% | 90% |
| pick (кэш eval) | 21 | coverage | 91% | 100% | 100% | 100% |
| pick (кэш eval) | 21 | union | 94% | 95% | 100% | 100% |
| случайные | 12 | gitnext | 34% | 53% | 62% | 83% |
| случайные | 12 | coverage | 100% | 85% | 86% | 92% |
| случайные | 12 | union | 100% | 61% | 78% | 92% |
| все | 33 | gitnext | 29% | 64% | 66% | 88% |
| все | 33 | coverage | 94% | 94% | 94% | 97% |
| все | 33 | union | 96% | 83% | 92% | 97% |

## По функциям

| выборка | функция | падают | граф: полн / из 10 | покрытие: полн / точн / из 10 | мимо покрытия |
|---|---|---:|---|---|---|
| pick | `org::jsoup::internal::LegacyMatching::cssAttributeMatches` | 7 | 0% / 0% | 100% / 100% / 100% |  |
| pick | `org::jsoup::internal::Normalizer::lowerCase` | 1689 | 70% / 80% | 21% / 100% / 100% | DocumentsWithSameContentAreEqual, DocumentsWithSameContentAreVerifiable, NoErrorOnEofIfBodyOpen, absAttr |
| pick | `org::jsoup::internal::SoftPool::release` | 1755 | 46% / 60% | 98% / 100% / 90% | adoptionDoesNotAddSecondDocumentElement, canCreateEmptyConnection, caseInsensitiveHeaders, downgradesSocksProxyToUrlConnectionExecutor |
| pick | `org::jsoup::internal::StringUtil::appendNormalisedWhitespace` | 446 | 29% / 100% | 100% / 100% / 100% |  |
| pick | `org::jsoup::internal::StringUtil::isActuallyWhitespace` | 446 | 28% / 100% | 100% / 100% / 100% |  |
| pick | `org::jsoup::internal::StringUtil::isAsciiLetter` | 1699 | 2% / 90% | 100% / 100% / 100% | init, provideEvaluators, rcDataRecoverParser |
| pick | `org::jsoup::internal::StringUtil::trimAsciiWhitespace` | 1811 | 8% / 100% | 78% / 100% / 100% | DocumentsWithSameContentAreEqual, DocumentsWithSameContentAreVerifiable, NoErrorOnEofIfBodyOpen, acceptsLenientXmlProcessingInstructionTarget |
| pick | `org::jsoup::nodes::Attributes::deduplicate` | 780 | 1% / 100% | 100% / 100% / 100% | init, provideEvaluators |
| pick | `org::jsoup::nodes::Attributes::getIgnoreCase` | 610 | 35% / 100% | 100% / 100% / 100% |  |
| pick | `org::jsoup::nodes::Attributes::removeIgnoreCase` | 28 | 7% / 10% | 100% / 100% / 100% |  |
| pick | `org::jsoup::nodes::DocumentType::publicId` | 115 | 3% / 43% | 100% / 100% / 100% |  |
| pick | `org::jsoup::nodes::Entities::isNamedEntity` | 51 | 16% / 30% | 100% / 100% / 100% |  |
| pick | `org::jsoup::parser::Parser::getMaxDepth` | 1467 | 0% / 40% | 100% / 100% / 100% | init, provideEvaluators |
| pick | `org::jsoup::parser::Tag::isFormSubmittable` | 16 | 100% / 100% | 100% / 100% / 100% |  |
| pick | `org::jsoup::parser::Token::Doctype::getSystemIdentifier` | 130 | 0% / 0% | 100% / 100% / 100% |  |
| pick | `org::jsoup::parser::TokenQueue::consumeCssIdentifier` | 383 | 2% / 90% | 100% / 99% / 100% |  |
| pick | `org::jsoup::parser::TokenQueue::matchesWhitespace` | 1721 | 7% / 90% | 61% / 100% / 100% | DocumentsWithSameContentAreEqual, DocumentsWithSameContentAreVerifiable, NoErrorOnEofIfBodyOpen, acceptsLenientXmlProcessingInstructionTarget |
| pick | `org::jsoup::parser::TokenQueue::matchesWord` | 1717 | 0% / 30% | 61% / 100% / 100% | DocumentsWithSameContentAreEqual, DocumentsWithSameContentAreVerifiable, NoErrorOnEofIfBodyOpen, acceptsLenientXmlProcessingInstructionTarget |
| pick | `org::jsoup::safety::Safelist::addEnforcedAttribute` | 38 | 100% / 100% | 100% / 100% / 100% |  |
| pick | `org::jsoup::safety::Safelist::addTags` | 72 | 99% / 100% | 100% / 100% / 100% |  |
| pick | `org::jsoup::select::Evaluator::asPredicate` | 957 | 2% / 90% | 100% / 100% / 100% |  |
| random | `org::jsoup::nodes::Node::ownerDocument` | 891 | 85% / 90% | 100% / 100% / 100% |  |
| random | `org::jsoup::nodes::NodeInternals::attributeRange` | 39 | 0% / 0% | 100% / 56% / 30% |  |
| random | `org::jsoup::parser::TokeniserState::read` | 1 | 0% / 0% | 100% / 0% / 0% |  |
| random | `org::jsoup::helper::HttpConnection::Response::sameOrigin` | 19 | 11% / 20% | 100% / 100% / 100% |  |
| random | `org::jsoup::nodes::Node::outerHtml` | 116 | 89% / 100% | 100% / 65% / 100% |  |
| random | `org::jsoup::nodes::Element::TextAccumulator::hasBlockChild` | 123 | 21% / 60% | 100% / 100% / 100% |  |
| random | `org::jsoup::nodes::Node::setParentNode` | 1554 | 21% / 70% | 100% / 100% / 100% | init, provideEvaluators |
| random | `org::jsoup::nodes::Document::text` | 1 | 100% / 10% | 100% / 100% / 100% |  |
| random | `org::jsoup::nodes::DataNode::outerHtmlHead` | 37 | 19% / 100% | 100% / 100% / 100% |  |
| random | `org::jsoup::parser::Token::Character::normalizeNulls` | 1090 | 0% / 100% | 100% / 100% / 100% | init, provideEvaluators |
| random | `org::jsoup::parser::TreeBuilder::parseAttributes` | 29 | 59% / 90% | 100% / 100% / 100% |  |
| random | `org::jsoup::nodes::Entities::encoderFor` | 573 | 2% / 100% | 100% / 98% / 100% |  |

## Цена

- `how`: per-test-fork
- `class_inits_s`: 3.3
- `class_inits_booked`: True
- `files_with_init_lines`: 64
- `mvn_s`: 110.0
- `analyze_s`: 25.8
- `map_s`: 139.0
- `exec_bytes`: 8878815
- `dumps`: 5372
- `test_methods`: 2130
- `classes`: 76
- `classes_with_setup_lines`: 6
- `run_failed`: False
- `names_in_several_classes`: 115
- `junit`: 5
- `package`: org.jsoup
- `owner`: body
- `repeat`: 3
- `plain_s`: 16.5
- `tests`: 2130
- `map_pairs`: 559173
- `map_functions`: 1771
- `map_sqlite_bytes`: 5746688
- `functions_with_body`: 2299
- `functions_never_run`: 99
- `fail_alone_not_together`: []
- `fail_together_not_alone`: []
- `slowdown_runs`: 8.4

# learn-tests (per-class) на jsoup (java): покрытие по тестам против `tests-for`
jsoup d90d57521d93, gitnext 14bcb17, seed 7. Истина — eval (тесты, падающие при throw в начале функции). pick — функции с ответом в кэше eval, random — случайные.

| выборка | функций | источник | полнота | точность | из 10 падают | поймано |
|---|---:|---|---:|---:|---:|---:|
| pick (кэш eval) | 21 | gitnext | 26% | 70% | 69% | 90% |
| pick (кэш eval) | 21 | coverage | 100% | 42% | 37% | 86% |
| pick (кэш eval) | 21 | union | 100% | 42% | 71% | 95% |
| случайные | 12 | gitnext | 34% | 53% | 62% | 83% |
| случайные | 12 | coverage | 100% | 24% | 25% | 67% |
| случайные | 12 | union | 100% | 23% | 58% | 92% |
| все | 33 | gitnext | 29% | 64% | 66% | 88% |
| все | 33 | coverage | 100% | 36% | 33% | 79% |
| все | 33 | union | 100% | 36% | 67% | 94% |

## По функциям

| выборка | функция | падают | граф: полн / из 10 | покрытие: полн / точн / из 10 | мимо покрытия |
|---|---|---:|---|---|---|
| pick | `org::jsoup::internal::LegacyMatching::cssAttributeMatches` | 7 | 0% / 0% | 100% / 4% / 0% |  |
| pick | `org::jsoup::internal::Normalizer::lowerCase` | 1689 | 70% / 80% | 100% / 87% / 30% | emptyCookieJar, init, provideEvaluators, testOutputSettings |
| pick | `org::jsoup::internal::SoftPool::release` | 1755 | 46% / 60% | 100% / 89% / 70% | emptyCookieJar, init, provideEvaluators, testOutputSettings |
| pick | `org::jsoup::internal::StringUtil::appendNormalisedWhitespace` | 446 | 29% / 100% | 100% / 26% / 0% |  |
| pick | `org::jsoup::internal::StringUtil::isActuallyWhitespace` | 446 | 28% / 100% | 100% / 26% / 0% |  |
| pick | `org::jsoup::internal::StringUtil::isAsciiLetter` | 1699 | 2% / 90% | 100% / 88% / 20% | init, provideEvaluators, rcDataRecoverParser |
| pick | `org::jsoup::internal::StringUtil::trimAsciiWhitespace` | 1811 | 8% / 100% | 100% / 93% / 70% | emptyCookieJar, init, provideEvaluators, rcDataRecoverParser |
| pick | `org::jsoup::nodes::Attributes::deduplicate` | 780 | 1% / 100% | 100% / 45% / 100% | init, provideEvaluators |
| pick | `org::jsoup::nodes::Attributes::getIgnoreCase` | 610 | 35% / 100% | 100% / 36% / 20% |  |
| pick | `org::jsoup::nodes::Attributes::removeIgnoreCase` | 28 | 7% / 10% | 100% / 5% / 10% |  |
| pick | `org::jsoup::nodes::DocumentType::publicId` | 115 | 3% / 43% | 100% / 9% / 30% |  |
| pick | `org::jsoup::nodes::Entities::isNamedEntity` | 51 | 16% / 30% | 100% / 5% / 30% |  |
| pick | `org::jsoup::parser::Parser::getMaxDepth` | 1467 | 0% / 40% | 100% / 79% / 20% | init, provideEvaluators |
| pick | `org::jsoup::parser::Tag::isFormSubmittable` | 16 | 100% / 100% | 100% / 4% / 10% |  |
| pick | `org::jsoup::parser::Token::Doctype::getSystemIdentifier` | 130 | 0% / 0% | 100% / 10% / 30% |  |
| pick | `org::jsoup::parser::TokenQueue::consumeCssIdentifier` | 383 | 2% / 90% | 100% / 27% / 30% |  |
| pick | `org::jsoup::parser::TokenQueue::matchesWhitespace` | 1721 | 7% / 90% | 100% / 89% / 90% | emptyCookieJar, init, provideEvaluators, testOutputSettings |
| pick | `org::jsoup::parser::TokenQueue::matchesWord` | 1717 | 0% / 30% | 100% / 89% / 90% | emptyCookieJar, init, provideEvaluators, testOutputSettings |
| pick | `org::jsoup::safety::Safelist::addEnforcedAttribute` | 38 | 100% / 100% | 100% / 10% / 30% |  |
| pick | `org::jsoup::safety::Safelist::addTags` | 72 | 99% / 100% | 100% / 19% / 90% |  |
| pick | `org::jsoup::select::Evaluator::asPredicate` | 957 | 2% / 90% | 100% / 53% / 10% |  |
| random | `org::jsoup::nodes::Node::ownerDocument` | 891 | 85% / 90% | 100% / 51% / 60% |  |
| random | `org::jsoup::nodes::NodeInternals::attributeRange` | 39 | 0% / 0% | 100% / 7% / 10% |  |
| random | `org::jsoup::parser::TokeniserState::read` | 1 | 0% / 0% | 100% / 0% / 0% |  |
| random | `org::jsoup::helper::HttpConnection::Response::sameOrigin` | 19 | 11% / 20% | 100% / 14% / 10% |  |
| random | `org::jsoup::nodes::Node::outerHtml` | 116 | 89% / 100% | 100% / 8% / 40% |  |
| random | `org::jsoup::nodes::Element::TextAccumulator::hasBlockChild` | 123 | 21% / 60% | 100% / 9% / 0% |  |
| random | `org::jsoup::nodes::Node::setParentNode` | 1554 | 21% / 70% | 100% / 83% / 20% | init, provideEvaluators |
| random | `org::jsoup::nodes::Document::text` | 1 | 100% / 10% | 100% / 3% / 10% |  |
| random | `org::jsoup::nodes::DataNode::outerHtmlHead` | 37 | 19% / 100% | 100% / 4% / 70% |  |
| random | `org::jsoup::parser::Token::Character::normalizeNulls` | 1090 | 0% / 100% | 100% / 60% / 0% | init, provideEvaluators |
| random | `org::jsoup::parser::TreeBuilder::parseAttributes` | 29 | 59% / 90% | 100% / 9% / 80% |  |
| random | `org::jsoup::nodes::Entities::encoderFor` | 573 | 2% / 100% | 100% / 36% / 0% |  |

## Цена

- `how`: per-class
- `mvn_s`: 413.9
- `analyze_s`: 2.1
- `map_s`: 416.0
- `exec_bytes`: 657374
- `dumps`: 76
- `test_methods`: 2130
- `classes`: 76
- `names_in_several_classes`: 115
- `junit`: 5
- `package`: org.jsoup
- `owner`: body
- `repeat`: 3
- `plain_s`: 15.3
- `tests`: 2130
- `map_pairs`: 1521462
- `map_functions`: 1771
- `map_sqlite_bytes`: 15364096
- `functions_with_body`: 2299
- `functions_never_run`: 99
- `fail_alone_not_together`: []
- `fail_together_not_alone`: []
- `slowdown_runs`: 27.2

# По классу теста

Класс засчитан, если в нём есть метод с падающим именем (имя в нескольких классах — все они).

| способ | выборка | функций | источник | полнота | точность | из 10 падают | поймано |
|---|---|---:|---|---:|---:|---:|---:|
| per-test | pick | 21 | gitnext | 39% | 89% | 89% | 90% |
| per-test | pick | 21 | coverage | 96% | 100% | 100% | 100% |
| per-test | pick | 21 | union | 98% | 100% | 100% | 100% |
| per-test | random | 12 | gitnext | 42% | 68% | 71% | 83% |
| per-test | random | 12 | coverage | 100% | 86% | 88% | 92% |
| per-test | random | 12 | union | 100% | 75% | 78% | 92% |
| per-test | all | 33 | gitnext | 40% | 81% | 82% | 88% |
| per-test | all | 33 | coverage | 98% | 95% | 96% | 97% |
| per-test | all | 33 | union | 99% | 91% | 92% | 97% |
| per-test-fork | pick | 21 | gitnext | 39% | 89% | 89% | 90% |
| per-test-fork | pick | 21 | coverage | 100% | 100% | 100% | 100% |
| per-test-fork | pick | 21 | union | 100% | 100% | 100% | 100% |
| per-test-fork | random | 12 | gitnext | 42% | 68% | 71% | 83% |
| per-test-fork | random | 12 | coverage | 100% | 86% | 88% | 92% |
| per-test-fork | random | 12 | union | 100% | 75% | 78% | 92% |
| per-test-fork | all | 33 | gitnext | 40% | 81% | 82% | 88% |
| per-test-fork | all | 33 | coverage | 100% | 95% | 96% | 97% |
| per-test-fork | all | 33 | union | 100% | 91% | 92% | 97% |
| per-class | pick | 21 | gitnext | 39% | 89% | 89% | 90% |
| per-class | pick | 21 | coverage | 100% | 80% | 85% | 100% |
| per-class | pick | 21 | union | 100% | 80% | 86% | 100% |
| per-class | random | 12 | gitnext | 42% | 68% | 71% | 83% |
| per-class | random | 12 | coverage | 100% | 65% | 71% | 92% |
| per-class | random | 12 | union | 100% | 62% | 72% | 92% |
| per-class | all | 33 | gitnext | 40% | 81% | 82% | 88% |
| per-class | all | 33 | coverage | 100% | 74% | 80% | 97% |
| per-class | all | 33 | union | 100% | 73% | 80% | 97% |

# Цена

| | per-test | per-test-fork | per-class |
|---|---:|---:|---:|
| plain_s | 14.2 | 16.5 | 15.3 |
| mvn_s | 20.2 | 110.0 | 413.9 |
| analyze_s | 25.7 | 25.8 | 2.1 |
| map_s | 49.6 | 139.0 | 416.0 |
| slowdown_runs | 3.5 | 8.4 | 27.2 |
| exec_bytes | 8545291 | 8878815 | 657374 |
| map_sqlite_bytes | 5718016 | 5746688 | 15364096 |
| dumps | 5372 | 5372 | 76 |
| test_methods | 2130 | 2130 | 2130 |
| classes | 76 | 76 | 76 |
| names_in_several_classes | 115 | 115 | 115 |
| map_pairs | 556456 | 559173 | 1521462 |
| map_functions | 1771 | 1771 | 1771 |
| functions_with_body | 2299 | 2299 | 2299 |
| functions_never_run | 99 | 99 | 99 |

jsoup d90d57521d93
