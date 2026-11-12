#!/usr/bin/env python3
"""The two result charts of the README (English and Russian): docs/images/agent*.svg, why*.svg.

    python3 docs/images/make_charts.py

Numbers: eval/results/HISTORY.md (the agent runs, `why` with the learned map). Two colours, the
same meaning in both charts: orange — without the thing shown, blue — with it. Every bar has its
value next to it, so nothing is told by colour alone.
"""
from pathlib import Path
from xml.sax.saxutils import escape

HERE = Path(__file__).resolve().parent
FONT = "-apple-system, BlinkMacSystemFont, Segoe UI, Helvetica, Arial, sans-serif"
SURFACE, BORDER, INK, MUTED = "#fbfcfa", "#d5dad3", "#1c2433", "#5e6b78"
WITHOUT, WITH = "#eb6834", "#2a78d6"
X0, SPAN, THICK, GAP, PITCH = 330, 470, 18, 2, 64


def bar(x: float, y: float, w: float, color: str) -> str:
    """Square at the baseline, 4px round at the data end."""
    if w <= 0:
        return ""
    r = min(4, w)
    return f'<path d="M{x} {y}h{w - r:.1f}a{r} {r} 0 0 1 {r} {r}v{THICK - 2 * r}a{r} {r} 0 0 1 -{r} {r}h-{w - r:.1f}z" fill="{color}"/>'


def chart(path: Path, title: str, subtitle: str, legend: tuple[str, str], rows: list, notes: list[str], alt: str) -> None:
    top = 122
    height = top + PITCH * len(rows) + 14 + 20 * len(notes) + 14
    o = [f'<svg xmlns="http://www.w3.org/2000/svg" width="1000" height="{height}" viewBox="0 0 1000 {height}" font-family="{FONT}" role="img" aria-label="{escape(alt)}">',
         f'<title>{escape(alt)}</title>',
         f'<rect width="1000" height="{height}" rx="14" fill="{SURFACE}" stroke="{BORDER}"/>',
         f'<text x="28" y="40" font-size="19" font-weight="700" fill="{INK}">{escape(title)}</text>',
         f'<text x="28" y="64" font-size="13" fill="{MUTED}">{escape(subtitle)}</text>']
    # legend: a swatch and its name, twice
    x = 28
    for color, name in zip((WITHOUT, WITH), legend):
        o.append(f'<rect x="{x}" y="82" width="12" height="12" rx="3" fill="{color}"/><text x="{x + 18}" y="93" font-size="13" fill="{INK}">{escape(name)}</text>')
        x += 18 + 7.2 * len(name) + 26
    # one hairline: the baseline every bar grows from
    y_end = top + PITCH * len(rows) - 14
    o.append(f'<path d="M{X0} {top - 8}V{y_end}" stroke="{BORDER}" stroke-width="1"/>')
    for k, (name, sub, a, b) in enumerate(rows):
        y = top + PITCH * k
        o.append(f'<text x="28" y="{y + 15}" font-size="14" font-weight="600" fill="{INK}">{escape(name)}</text>')
        if sub:
            o.append(f'<text x="28" y="{y + 33}" font-size="12" fill="{MUTED}">{escape(sub)}</text>')
        for j, ((share, label), color) in enumerate(((a, WITHOUT), (b, WITH))):
            yy = y + j * (THICK + GAP)
            w = SPAN * share
            o.append(bar(X0, yy, w, color))
            o.append(f'<text x="{X0 + w + 10:.1f}" y="{yy + 13.5}" font-size="13" font-weight="{700 if j else 400}" fill="{INK}">{escape(label)}</text>')
    y = top + PITCH * len(rows) + 14
    for n in notes:
        o.append(f'<text x="28" y="{y}" font-size="12.5" fill="{MUTED}">{escape(n)}</text>')
        y += 20
    o.append("</svg>")
    path.write_text("\n".join(o) + "\n")


def of6(n: int, word: str = "of") -> tuple[float, str]:
    return n / 6, f"{n} {word} 6"


def pct(n: int, d: int, word: str = "of") -> tuple[float, str]:
    return n / d, f"{round(100 * n / d)}%  ·  {n} {word} {d}"


AGENT = [(4, 6), (0, 6), (0, 5), (0, 0)]  # find, who calls, rename, fix: plain tools, with gitnext
WHY = [(1, 18, 20), (6, 14, 17), (3, 14, 20), (10, 46, 57)]  # python, go, rust, all: graph, map, bugs

chart(HERE / "agent.svg", "A 4B local model on real projects: tasks solved",
      "Qwen 3.5 4B on llama.cpp · click (Python) and semver (Rust) · 6 tasks of each kind · judged by the projects' own compilers and tests",
      ("plain tools: read, grep, edit, run", "the same model with gitnext tools"),
      [(n, s, of6(a), of6(b)) for (n, s), (a, b) in zip([("Find where a function is", ""), ("Who calls this function", ""), ("Rename it everywhere", "the project must still build"), ("Fix a failing test", "a seeded bug")], AGENT)],
      ["24 tasks: 4 solved with plain tools, 17 with gitnext (15 when the model is not asked to look before it answers).",
       "Fixing a bug is the open one: 0 of 6 here; 3 of 12 on click in a later run, once gitnext why knew what the failing test really runs."],
      "Tasks solved by a 4B local model: with plain tools 4 of 24, with gitnext tools 17 of 24")
chart(HERE / "agent.ru.svg", "Локальная модель на 4B в настоящих проектах: решено заданий",
      "Qwen 3.5 4B на llama.cpp · click (Python) и semver (Rust) · по 6 заданий каждого вида · судят компилятор и тесты самих проектов",
      ("обычные инструменты: читать, искать, править, запускать", "та же модель с инструментами gitnext"),
      [(n, s, of6(a, "из"), of6(b, "из")) for (n, s), (a, b) in zip([("Найти, где функция", ""), ("Кто вызывает функцию", ""), ("Переименовать везде", "проект должен собираться"), ("Починить упавший тест", "подсаженная ошибка")], AGENT)],
      ["24 задания: с обычными инструментами решено 4, с gitnext — 17 (15, если от модели не требовать сначала посмотреть код).",
       "Починка ошибки пока не решена: здесь 0 из 6; в более позднем замере на click 3 из 12 — когда gitnext why узнал, что на деле запускает упавший тест."],
      "Задания, решённые локальной моделью на 4B: с обычными инструментами 4 из 24, с gitnext 17 из 24")
chart(HERE / "why.svg", "A test fails: is the function at fault first in the list?",
      "57 seeded bugs in 6 projects: one operator changed inside a function, then gitnext why <failing test> is asked",
      ("from the call graph alone", "after gitnext learn-tests (one test run with coverage)"),
      [(n, s, pct(a, d), pct(b, d)) for (n, s), (a, b, d) in zip([("Python", "click, sqlparse"), ("Go", "cobra, chi"), ("Rust", "semver, indexmap"), ("All six projects", "")], WHY)],
      ["With the map, why lists what the test really ran, puts the code changed since the tests last passed on top, and shows the changed lines.",
       "Reproduce: python3 eval/why_rank.py --learn --variants static,default — tables in eval/results/why-rank*.md"],
      "Share of seeded bugs where gitnext why names the function at fault first: 18% from the call graph, 81% with the learned coverage map")
chart(HERE / "why.ru.svg", "Тест упал: стоит ли виновная функция первой в списке?",
      "57 подсаженных ошибок в 6 проектах: в функции меняется один оператор, затем спрашиваем gitnext why <упавший тест>",
      ("только по графу вызовов", "после gitnext learn-tests (один прогон тестов с покрытием)"),
      [(n, s, pct(a, d, "из"), pct(b, d, "из")) for (n, s), (a, b, d) in zip([("Python", "click, sqlparse"), ("Go", "cobra, chi"), ("Rust", "semver, indexmap"), ("Все шесть проектов", "")], WHY)],
      ["С картой why показывает то, что тест на деле запускал, ставит первым код, изменённый с тех пор, как тесты проходили, и сами изменённые строки.",
       "Воспроизвести: python3 eval/why_rank.py --learn --variants static,default — таблицы в eval/results/why-rank*.md"],
      "Доля подсаженных ошибок, где gitnext why называет виновную функцию первой: 18% по графу вызовов, 81% с картой покрытия")
print("written:", ", ".join(p.name for p in sorted(HERE.glob("*.svg")) if p.name.startswith(("agent", "why"))))
