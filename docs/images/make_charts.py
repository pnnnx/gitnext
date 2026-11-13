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


AGENT = [(46, 52, 72), (44, 51, 72), (54, 60, 72), (144, 163, 216)]  # 4B, 9B, 27B, all: plain tools, the five gitnext tools, tasks
WHY = [(1, 18, 20), (6, 14, 17), (3, 17, 20), (10, 49, 57)]  # python, go, rust, all: graph, map, bugs

chart(HERE / "agent.svg", "Three local models on real projects: tasks solved",
      "Qwen 3.5 on llama.cpp · six projects in Python, Rust and Go · the same 72 tasks each way · judged by the projects' compilers and tests",
      ("plain tools: read, grep, edit, run", "the five gitnext tools: look, replace, add_after, rename, check"),
      [(n, s, pct(a, d), pct(b, d)) for (n, s), (a, b, d) in zip([("Qwen 3.5 4B", "median steps 10 → 3"), ("Qwen 3.5 9B", "median steps 9 → 4"), ("Qwen 3.5 27B", "median steps 10 → 3 · tokens ÷ 2.4"), ("All three models", "+9 points, 95% interval +1 to +17")], AGENT)],
      ["For one model alone the difference in solved tasks is inside the noise; the steps and tokens differ in every kind of task.",
       "Fixing a seeded bug with the 27B model: 7 of 18 with plain tools, 14 of 18 with gitnext. The small models fix 3–6 of 18 either way."],
      "Tasks solved by three local models: 144 of 216 with plain tools, 163 of 216 with the five gitnext tools")
chart(HERE / "agent.ru.svg", "Три локальные модели в настоящих проектах: решено заданий",
      "Qwen 3.5 на llama.cpp · шесть проектов на Python, Rust и Go · одни и те же 72 задания · судят компилятор и тесты проектов",
      ("обычные инструменты: читать, искать, править, запускать", "пять инструментов gitnext: look, replace, add_after, rename, check"),
      [(n, s, pct(a, d, "из"), pct(b, d, "из")) for (n, s), (a, b, d) in zip([("Qwen 3.5 4B", "шагов (медиана) 10 → 3"), ("Qwen 3.5 9B", "шагов (медиана) 9 → 4"), ("Qwen 3.5 27B", "шагов 10 → 3 · токенов ÷ 2,4"), ("Все три модели", "+9 пунктов, 95%-й интервал от +1 до +17")], AGENT)],
      ["Для одной модели разница в числе решённых заданий не выходит за погрешность; шаги и токены различаются в каждом виде заданий.",
       "Починка подсаженной ошибки моделью 27B: 7 из 18 с обычными инструментами, 14 из 18 с gitnext. Малые модели чинят 3–6 из 18 в любом режиме."],
      "Задания, решённые тремя локальными моделями: 144 из 216 с обычными инструментами, 163 из 216 с пятью инструментами gitnext")
chart(HERE / "why.svg", "A test fails: is the function at fault first in the list?",
      "57 seeded bugs in 6 projects: one operator changed inside a function, then gitnext why <failing test> is asked",
      ("from the call graph alone", "after gitnext learn-tests (one test run with coverage)"),
      [(n, s, pct(a, d), pct(b, d)) for (n, s), (a, b, d) in zip([("Python", "click, sqlparse"), ("Go", "cobra, chi"), ("Rust", "semver, indexmap"), ("All six projects", "")], WHY)],
      ["With the map, why lists what the test really ran, puts the code changed since the tests last passed on top, and shows the changed lines.",
       "Reproduce: python3 eval/why_rank.py --learn --variants static,default — tables in eval/results/why-rank*.md"],
      "Share of seeded bugs where gitnext why names the function at fault first: 18% from the call graph, 86% with the learned coverage map")
chart(HERE / "why.ru.svg", "Тест упал: стоит ли виновная функция первой в списке?",
      "57 подсаженных ошибок в 6 проектах: в функции меняется один оператор, затем спрашиваем gitnext why <упавший тест>",
      ("только по графу вызовов", "после gitnext learn-tests (один прогон тестов с покрытием)"),
      [(n, s, pct(a, d, "из"), pct(b, d, "из")) for (n, s), (a, b, d) in zip([("Python", "click, sqlparse"), ("Go", "cobra, chi"), ("Rust", "semver, indexmap"), ("Все шесть проектов", "")], WHY)],
      ["С картой why показывает то, что тест на деле запускал, ставит первым код, изменённый с тех пор, как тесты проходили, и сами изменённые строки.",
       "Воспроизвести: python3 eval/why_rank.py --learn --variants static,default — таблицы в eval/results/why-rank*.md"],
      "Доля подсаженных ошибок, где gitnext why называет виновную функцию первой: 18% по графу вызовов, 86% с картой покрытия")
print("written:", ", ".join(p.name for p in sorted(HERE.glob("*.svg")) if p.name.startswith(("agent", "why"))))
