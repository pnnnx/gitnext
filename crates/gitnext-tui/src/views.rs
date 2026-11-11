//! Detail views written for people: aligned columns instead of markdown tables,
//! words and arrows instead of glyphs, inputs and outputs highlighted.
//!
//! Colors carry one meaning each: inputs are yellow, outputs green, names bold,
//! relations (uses / is a / called by) use arrows → and ←.

use super::*;
use gitnext_ops::gitnext_index::SymbolRow;
use gitnext_ops::gitnext_parse::Symbol;

pub(crate) const INPUT: Color = Color::Yellow;
pub(crate) const OUTPUT: Color = Color::Green;
pub(crate) const RELATION: Color = Color::Magenta;

/// Kind as a short word, padded so names line up.
pub(crate) fn kind_word(k: Kind) -> &'static str {
    match k {
        Kind::Module => "module",
        Kind::Function => "fn",
        Kind::Method => "fn",
        Kind::Struct => "struct",
        Kind::Union => "union",
        Kind::Class => "class",
        Kind::Enum => "enum",
        Kind::Trait => "trait",
        Kind::TypeAlias => "type",
        Kind::Const => "const",
        Kind::Static => "static",
        Kind::Macro => "macro",
    }
}

fn is_self(p: &gitnext_ops::gitnext_parse::Param) -> bool {
    matches!(p.name.as_str(), "self" | "cls" | "this") || p.ty.trim_start_matches(['&', ' ']).starts_with("mut self") || p.ty == "&self" || p.ty == "&mut self" || p.ty == "self"
}

/// `name(a: A, b: B) → R` with inputs and output colored.
pub(crate) fn call_spans(s: &Symbol, name_style: Style) -> Vec<Span<'static>> {
    let mut v = vec![Span::styled(s.name.clone(), name_style)];
    if s.kind.is_callable() {
        v.push(Span::raw("("));
        let params: Vec<&gitnext_ops::gitnext_parse::Param> = s.params.iter().filter(|p| !is_self(p)).collect();
        for (k, p) in params.iter().enumerate() {
            if k > 0 {
                v.push(Span::raw(", "));
            }
            v.push(Span::styled(p.name.clone(), Style::new().fg(INPUT)));
            if !p.ty.is_empty() {
                let sep = if p.ty.starts_with('=') { " " } else { ": " };
                v.push(Span::styled(format!("{sep}{}", p.ty), Style::new().fg(DIM)));
            }
        }
        v.push(Span::raw(")"));
        if let Some(r) = s.returns.as_ref().filter(|r| !r.is_empty() && r.as_str() != "None" && r.as_str() != "void" && r.as_str() != "()") {
            v.push(Span::styled("  →  ", Style::new().fg(DIM)));
            v.push(Span::styled(r.clone(), Style::new().fg(OUTPUT)));
        }
    }
    v
}

fn pad(s: &str, w: usize) -> String {
    let n = s.chars().count();
    if n >= w {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(w - n))
    }
}

/// Cut to `max` characters with an ellipsis; drop markdown backticks.
pub(crate) fn fit(s: &str, max: usize) -> String {
    let s = s.replace('`', "");
    if s.chars().count() <= max || max < 4 {
        s
    } else {
        s.chars().take(max - 1).collect::<String>() + "…"
    }
}

fn plural(n: usize, w: &str) -> String {
    gitnext_ops::changes::plural(n, w)
}

impl App<'_> {
    /// One line per relation of a type: `is a → Animal`, `implements → Shape, Display`.
    fn type_relations(&self, i: usize) -> Vec<Line<'static>> {
        let g = &self.g;
        let h = g.hierarchy(i);
        let mut l = Vec::new();
        let rel = |label: &str, arrow: &str, names: Vec<String>| -> Line<'static> {
            Line::from(vec![
                Span::styled(format!("  {} ", pad(label, 14)), Style::new().fg(DIM)),
                Span::styled(format!("{arrow} "), Style::new().fg(RELATION)),
                Span::raw(names.join(", ")),
            ])
        };
        if !h.extends.is_empty() {
            l.push(rel("is a", "→", h.extends.clone()));
        }
        if !h.implements.is_empty() {
            l.push(rel("implements", "→", h.implements.clone()));
        }
        if !h.subtypes.is_empty() {
            l.push(rel("extended by", "←", h.subtypes.iter().map(|x| x.short.clone()).collect()));
        }
        if !h.implementors.is_empty() {
            l.push(rel("implemented by", "←", h.implementors.iter().map(|x| x.short.clone()).collect()));
        }
        l
    }

    pub(crate) fn dir_view(&self, dir: &str) -> Text<'static> {
        let g = &self.g;
        let ctx = gitnext_ops::gitnext_docs::Ctx::new(&self.ws.root, g);
        let prefix = if dir.is_empty() { String::new() } else { format!("{dir}/") };
        let files: Vec<&gitnext_ops::gitnext_index::FileRow> = g.files.iter().filter(|f| gitnext_ops::gitnext_index::dir_of(&f.path) == dir).collect();
        let all_below: Vec<&gitnext_ops::gitnext_index::FileRow> = g.files.iter().filter(|f| f.path.starts_with(&prefix)).collect();
        let mut l: Vec<Line> = vec![];
        let title = if dir.is_empty() { self.repo_name.clone() } else { format!("{dir}/") };
        l.push(Line::from(Span::styled(title, Style::new().bold().fg(ACCENT))));
        if let Some(p) = ctx.dir_purpose(dir) {
            l.push(Line::from(Span::styled(p.replace('`', ""), Style::new().italic())));
        }
        let lines: u32 = all_below.iter().map(|f| f.lines).sum();
        let syms = g.syms.iter().filter(|s| s.file.starts_with(&prefix) && !s.sym.test && !(s.sym.kind == Kind::Module && s.sym.start_line == 1)).count();
        l.push(Line::from(format!("{} · {} lines · {} functions and types", plural(all_below.len(), "file"), lines, syms).fg(DIM)));

        // where this folder sits: what it uses, who uses it
        let width = self.detail_width as usize;
        let own_code = files.iter().any(|f| f.parsed);
        if !dir.is_empty() && own_code {
            let d = g.dir_deps(dir);
            l.push(Line::raw(""));
            for (label, arrow, v) in [("uses", "→", &d.depends_on), ("used by", "←", &d.used_by)] {
                if v.is_empty() {
                    l.push(Line::from(vec![Span::styled(format!("  {}", pad(label, 10)), Style::new().fg(DIM)), Span::styled(format!("{arrow} "), Style::new().fg(RELATION)), Span::styled("nothing in this repo", Style::new().fg(DIM))]));
                    continue;
                }
                let w = v.iter().take(6).map(|(d, _)| d.chars().count()).max().unwrap_or(0) + 2;
                for (k, (target, n)) in v.iter().take(6).enumerate() {
                    l.push(Line::from(vec![
                        Span::styled(format!("  {}", pad(if k == 0 { label } else { "" }, 10)), Style::new().fg(DIM)),
                        Span::styled(format!("{arrow} "), Style::new().fg(RELATION)),
                        Span::raw(pad(target, w)),
                        Span::styled(plural(*n, "reference"), Style::new().fg(DIM)),
                    ]));
                }
            }
        } else if dir.is_empty() {
            // whole repo: the layers, foundations at the bottom
            let ly = gitnext_ops::gitnext_docs::layers(g);
            if ly.layers.len() > 1 || !ly.uses.is_empty() {
                l.push(Line::raw(""));
                l.push(Line::from("How the code is layered (each level uses only the levels below it)".bold()));
                let w = ly.layers.iter().flatten().map(|a| a.chars().count()).max().unwrap_or(4) + 2;
                for (k, layer) in ly.layers.iter().enumerate().rev() {
                    let label = if k == 0 { "foundation" } else if k + 1 == ly.layers.len() { "top" } else { "middle" };
                    for (j, a) in layer.iter().enumerate() {
                        let uses = ly.uses.get(a).map(|v| v.iter().take(4).map(|(t, n)| format!("{t} ({n})")).collect::<Vec<_>>().join(", ")).unwrap_or_default();
                        let mut spans = vec![
                            Span::styled(format!("  {}", pad(if j == 0 { label } else { "" }, 12)), Style::new().fg(DIM)),
                            Span::styled(pad(a, w), Style::new().bold()),
                        ];
                        if !uses.is_empty() {
                            spans.push(Span::styled("uses → ", Style::new().fg(RELATION)));
                            spans.push(Span::raw(uses));
                        }
                        l.push(Line::from(spans));
                    }
                }
                for c in &ly.cycles {
                    l.push(Line::from(Span::styled(format!("  cycle: {} use each other — hard to change one alone", c.join(" ⇄ ")), Style::new().fg(Color::Yellow))));
                }
            }
        }

        // sub-folders
        let subdirs: Vec<String> = g.dirs().into_iter().filter(|d| !d.is_empty() && gitnext_ops::gitnext_index::dir_of(d) == dir).collect();
        if !subdirs.is_empty() {
            l.push(Line::raw(""));
            l.push(Line::from("Folders".bold()));
            let w = subdirs.iter().map(|d| d.rsplit('/').next().unwrap_or(d).chars().count() + 1).max().unwrap_or(0) + 2;
            for d in &subdirs {
                let name = format!("{}/", d.rsplit('/').next().unwrap_or(d));
                let n = g.files.iter().filter(|f| f.path.starts_with(&format!("{d}/"))).count();
                l.push(Line::from(vec![
                    Span::raw(format!("  {}", pad(&name, w))),
                    Span::styled(pad(&plural(n, "file"), 10), Style::new().fg(DIM)),
                    Span::raw(fit(&ctx.dir_purpose(d).unwrap_or_default(), width.saturating_sub(w + 12))),
                ]));
            }
        }

        // files: aligned name · size · purpose
        if !files.is_empty() {
            l.push(Line::raw(""));
            l.push(Line::from("Files".bold()));
            let w = files.iter().map(|f| f.path.rsplit('/').next().unwrap_or(&f.path).chars().count()).max().unwrap_or(0) + 2;
            for f in &files {
                let name = f.path.rsplit('/').next().unwrap_or(&f.path).to_string();
                let purpose = f.module_doc.as_deref().and_then(gitnext_ops::gitnext_parse::first_sentence).unwrap_or_default();
                l.push(Line::from(vec![
                    Span::raw(format!("  {}", pad(&name, w))),
                    Span::styled(pad(&format!("{} lines", f.lines), 11), Style::new().fg(DIM)),
                    Span::raw(fit(&purpose, width.saturating_sub(w + 13))),
                ]));
            }
        }

        // what each file offers: public functions and types with input → output
        for f in files.iter().filter(|f| f.parsed) {
            let ids: Vec<usize> = g.in_file(&f.path).into_iter().filter(|&i| {
                let s = &g.syms[i].sym;
                !s.test && !(s.kind == Kind::Module && s.start_line == 1) && s.kind != Kind::Method
            }).collect();
            if ids.is_empty() {
                continue;
            }
            l.push(Line::raw(""));
            l.push(Line::from(Span::styled(f.path.rsplit('/').next().unwrap_or(&f.path).to_string(), Style::new().bold())));
            let (public, private): (Vec<usize>, Vec<usize>) = ids.into_iter().partition(|&i| g.syms[i].sym.is_public());
            for &i in &public {
                self.api_lines(&g.syms[i], i, &mut l);
            }
            if !private.is_empty() {
                let names: Vec<String> = private.iter().map(|&i| g.syms[i].sym.name.clone()).collect();
                l.push(Line::from(vec![Span::styled("  also inside: ", Style::new().fg(DIM)), Span::styled(names.join(", "), Style::new().fg(DIM))]));
            }
        }
        Text::from(l)
    }

    /// A function as `name(inputs) → output` + its one-line meaning; a type with
    /// its fields and relations.
    fn api_lines(&self, r: &SymbolRow, i: usize, l: &mut Vec<Line<'static>>) {
        let s = &r.sym;
        let mut head = vec![Span::styled(format!("  {} ", pad(kind_word(s.kind), 6)), Style::new().fg(DIM))];
        head.extend(call_spans(s, Style::new().bold()));
        l.push(Line::from(head));
        if let Some(sum) = &s.summary {
            let mark = if s.inferred { "≈ " } else { "" };
            l.push(Line::from(Span::styled(format!("         {}", fit(&format!("{mark}{sum}"), (self.detail_width as usize).saturating_sub(9))), Style::new().fg(DIM))));
        }
        if s.kind.is_type() {
            if !s.params.is_empty() && matches!(s.kind, Kind::Struct | Kind::Class | Kind::Union | Kind::Enum) {
                let label = if s.kind == Kind::Enum { "variants" } else { "fields" };
                let fields: Vec<String> = s.params.iter().take(8).map(|p| p.name.clone()).collect();
                let more = s.params.len().saturating_sub(8);
                l.push(Line::from(vec![
                    Span::styled(format!("         {label}: "), Style::new().fg(DIM)),
                    Span::styled(fields.join(", ") + &if more > 0 { format!(" +{more}") } else { String::new() }, Style::new().fg(INPUT)),
                ]));
            }
            for rel in self.type_relations(i) {
                let mut spans = vec![Span::raw("       ")];
                spans.extend(rel.spans);
                l.push(Line::from(spans));
            }
            let methods: Vec<String> = self.g.syms.iter().filter(|m| m.sym.kind == Kind::Method && m.sym.parent.as_deref() == Some(s.qualified.as_str()) && m.sym.is_public() && m.sym.trait_impl.is_none()).map(|m| m.sym.name.clone()).collect();
            if !methods.is_empty() {
                l.push(Line::from(vec![Span::styled("         methods: ", Style::new().fg(DIM)), Span::raw(methods.join(", "))]));
            }
        }
    }

    pub(crate) fn symbol_view(&self, i: usize) -> (Vec<Line<'static>>, Vec<(usize, usize)>) {
        let g = &self.g;
        let r = &g.syms[i];
        let s = &r.sym;
        let mut l: Vec<Line> = vec![];
        let mut links: Vec<(usize, usize)> = vec![];
        l.push(Line::from(vec![
            Span::styled(format!("{} ", kind_word(s.kind)), Style::new().fg(DIM)),
            Span::styled(g.short_name(i).to_string(), Style::new().bold()),
        ]));
        l.push(Line::from(Span::styled(format!("{} · lines {}–{}", r.file, s.start_line, s.end_line), Style::new().fg(DIM))));
        if let Some(t) = &s.trait_impl {
            l.push(Line::from(vec![Span::styled("part of ", Style::new().fg(DIM)), Span::raw(format!("its {t} implementation"))]));
        }
        if let Some(d) = &s.doc {
            l.push(Line::raw(""));
            for x in d.lines() {
                let clean = x.replace("[`", "").replace("`]", "").replace('`', "");
                l.push(Line::from(Span::styled(clean, Style::new().italic())));
            }
        } else if let Some(sum) = &s.summary {
            l.push(Line::raw(""));
            l.push(Line::from(Span::styled(format!("≈ {sum}"), Style::new().italic().fg(DIM))));
        }

        // inputs and output, one per line, the most important part
        if s.kind.is_callable() {
            l.push(Line::raw(""));
            let params: Vec<&gitnext_ops::gitnext_parse::Param> = s.params.iter().filter(|p| !is_self(p)).collect();
            if params.is_empty() {
                l.push(Line::from(vec![Span::styled("  takes    ", Style::new().fg(DIM)), Span::styled("nothing", Style::new().fg(DIM))]));
            }
            let w = params.iter().map(|p| p.name.chars().count()).max().unwrap_or(0) + 2;
            for (k, p) in params.iter().enumerate() {
                let label = if k == 0 { "  takes    " } else { "           " };
                let ty = p.ty.trim_start_matches('=').trim().to_string();
                let default = p.ty.starts_with('=');
                l.push(Line::from(vec![
                    Span::styled(label, Style::new().fg(DIM)),
                    Span::styled(pad(&p.name, w), Style::new().fg(INPUT).bold()),
                    Span::styled(if default { format!("default {ty}") } else { ty }, Style::new().fg(INPUT)),
                ]));
            }
            let ret = s.returns.clone().filter(|r| !r.is_empty());
            l.push(Line::from(vec![
                Span::styled("  returns  ", Style::new().fg(DIM)),
                Span::styled("→ ", Style::new().fg(DIM)),
                match ret {
                    Some(r) if r != "None" && r != "void" && r != "()" => Span::styled(r, Style::new().fg(OUTPUT).bold()),
                    _ => Span::styled("nothing", Style::new().fg(DIM)),
                },
            ]));
            l.push(Line::from(vec![Span::styled("  signature ", Style::new().fg(DIM)), Span::styled(s.signature.clone(), Style::new().fg(DIM))]));
        } else if !s.params.is_empty() {
            let label = match s.kind {
                Kind::Enum => "can be",
                _ => "holds",
            };
            l.push(Line::raw(""));
            let w = s.params.iter().map(|p| p.name.chars().count()).max().unwrap_or(0) + 2;
            for (k, p) in s.params.iter().enumerate() {
                let lab = if k == 0 { format!("  {} ", pad(label, 8)) } else { " ".repeat(11) };
                l.push(Line::from(vec![
                    Span::styled(lab, Style::new().fg(DIM)),
                    Span::styled(pad(&p.name, w), Style::new().fg(INPUT).bold()),
                    Span::styled(p.ty.trim_start_matches('=').trim().to_string(), Style::new().fg(INPUT)),
                ]));
            }
        } else if let Some(r) = &s.returns {
            l.push(Line::raw(""));
            l.push(Line::from(vec![Span::styled("  is       ", Style::new().fg(DIM)), Span::styled(r.clone(), Style::new().fg(OUTPUT))]));
        }

        if s.kind.is_type() {
            let rels = self.type_relations(i);
            if !rels.is_empty() {
                l.push(Line::raw(""));
                l.extend(rels);
            }
        }

        // navigable lists: one line each, arrows say the direction
        let section = |l: &mut Vec<Line<'static>>, links: &mut Vec<(usize, usize)>, title: String, ids: Vec<usize>| {
            if ids.is_empty() {
                return;
            }
            l.push(Line::raw(""));
            l.push(Line::from(Span::styled(title, Style::new().fg(RELATION))));
            let w = ids.iter().take(40).map(|&c| g.short_name(c).chars().count()).max().unwrap_or(0) + 2;
            for c in ids.into_iter().take(40) {
                let sym = &g.syms[c];
                links.push((l.len(), c));
                l.push(Line::from(vec![
                    Span::raw("  "),
                    Span::raw(pad(g.short_name(c), w)),
                    Span::styled(format!("{}:{}", sym.file.rsplit('/').next().unwrap_or(&sym.file), sym.sym.start_line), Style::new().fg(DIM)),
                ]));
            }
        };
        if s.kind.is_callable() || s.kind.is_type() {
            let all: Vec<usize> = g.callers(i).into_iter().map(|x| x.0).collect();
            let (tests, real): (Vec<usize>, Vec<usize>) = all.into_iter().partition(|&c| g.syms[c].sym.test);
            let tests_note = if tests.is_empty() { String::new() } else { format!(" (+{} in tests)", tests.len()) };
            let verb = if s.kind.is_type() { "used by" } else { "called by" };
            if real.is_empty() && !tests.is_empty() {
                l.push(Line::raw(""));
                l.push(Line::from(Span::styled(format!("← {verb} tests only ({})", tests.len()), Style::new().fg(RELATION))));
            }
            section(&mut l, &mut links, format!("← {verb} {}{tests_note}", real.len()), real);
        }
        if s.kind.is_callable() {
            let c = g.callees(i);
            section(&mut l, &mut links, format!("→ calls {}", c.resolved.len()), c.resolved.iter().map(|x| x.0).collect());
            for (name, cands) in &c.ambiguous {
                l.push(Line::from(vec![Span::raw(format!("  {name}")), Span::styled(format!("  one of {} with this name", cands.len()), Style::new().fg(DIM))]));
            }
            let libs = g.libraries(i);
            if !libs.is_empty() {
                let deps = gitnext_ops::libs::deps(&self.ws.root, g);
                l.push(Line::raw(""));
                l.push(Line::from(Span::styled(format!("→ libraries {}", libs.len()), Style::new().fg(RELATION))));
                for lib in libs.iter().take(8) {
                    let m: Vec<&str> = lib.members.iter().take(5).map(String::as_str).collect();
                    l.push(Line::from(vec![Span::raw(format!("  {}", gitnext_ops::libs::label(lib, &deps))), Span::styled(format!("  {}", m.join(", ")), Style::new().fg(DIM))]));
                }
            }
            // what is left outside the repo: builtins and methods on values of unknown type
            let in_libs: std::collections::HashSet<&str> = libs.iter().flat_map(|x| x.members.iter().map(String::as_str)).collect();
            let rest: Vec<&str> = c.external.iter().map(String::as_str).filter(|x| !in_libs.contains(x.rsplit("::").next().unwrap_or(x))).collect();
            if !rest.is_empty() {
                let shown: Vec<&str> = rest.iter().take(10).copied().collect();
                let more = rest.len().saturating_sub(10);
                l.push(Line::from(Span::styled(format!("  other outside calls: {}{}", shown.join(", "), if more > 0 { format!(" +{more}") } else { String::new() }), Style::new().fg(DIM))));
            }
        }
        let uses = g.uses(i);
        section(&mut l, &mut links, format!("→ uses types {}", uses.len()), uses);
        if s.kind.is_type() {
            let h = g.hierarchy(i);
            let to_ids = |v: &[SymRef]| v.iter().filter_map(|x| self.ref_index(x)).collect::<Vec<_>>();
            for (imp, ms) in &h.methods {
                let t = if imp.is_empty() { "methods".to_string() } else { format!("methods for {imp}") };
                section(&mut l, &mut links, t, to_ids(ms));
            }
        }
        // the code itself
        if s.kind != Kind::Module {
            if let Ok(text) = std::fs::read_to_string(self.ws.root.join(&r.file)) {
                let lines: Vec<&str> = text.lines().collect();
                let (a, b) = (s.start_line as usize, (s.end_line as usize).min(lines.len()));
                if a >= 1 && a <= b {
                    l.push(Line::raw(""));
                    l.push(Line::from("code".fg(DIM)));
                    let lang = lang_for(std::path::Path::new(&r.file));
                    let mut state = HlState::default();
                    let cap = 400;
                    for (n, line) in lines[a - 1..b].iter().enumerate().take(cap) {
                        let mut spans = vec![Span::styled(format!("{:>4}  ", a + n), Style::new().fg(DIM))];
                        spans.extend(highlight(line, lang, &mut state));
                        l.push(Line::from(spans));
                    }
                    if b - a + 1 > cap {
                        l.push(Line::from(format!("      … {} more lines", b - a + 1 - cap).fg(DIM)));
                    }
                }
            }
        }
        (l, links)
    }
}
