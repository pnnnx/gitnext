//! `f().m()` where `f` returns a library type: the one repo method named `m` is only a guess
//! (chi `r.Context().Value(…)` → `endpoints.Value`, cobra `c.PersistentFlags().HasFlags()` →
//! `Command.HasFlags`). A result of a repo type, or one the graph cannot tell, keeps the call sure.

use gitnext_index::{Graph, Index};

fn graph(name: &str, files: &[(&str, &str)]) -> Graph {
    let dir = std::env::temp_dir().join(format!("gitnext-libres-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (path, code) in files {
        let p = dir.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, code).unwrap();
    }
    let mut idx = Index::open(&dir).unwrap();
    idx.refresh().unwrap();
    let g = idx.graph().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    g
}

/// (sure callers, guessed callers) of `target`, by name.
fn callers(g: &Graph, target: &str) -> (Vec<String>, Vec<String>) {
    let found = g.find(target);
    assert_eq!(found.len(), 1, "{target}: {found:?}");
    let (mut sure, mut guess) = (Vec::new(), Vec::new());
    for (c, _) in g.callers(found[0]) {
        let name = g.syms[c].sym.name.clone();
        if g.is_guess(c, found[0]) { guess.push(name) } else { sure.push(name) }
    }
    sure.sort();
    sure.dedup();
    guess.sort();
    guess.dedup();
    (sure, guess)
}

const GO: &str = r#"package app

import (
	"net/http"

	"github.com/spf13/pflag"
)

type endpoints struct{}

func (e endpoints) Value(k string) string { return k }

type Options struct{}

func (o *Options) Changed() bool { return true }

type Command struct{ opts *Options }

func (c *Command) Opts() *Options          { return c.opts }
func (c *Command) PFlags() *pflag.FlagSet { return nil }

func fromLibrary(r *http.Request) string { return r.Context().Value("k").(string) }
func fromRepoMethod(c *Command) bool      { return c.PFlags().Changed() }
func fromRepoType(c *Command) bool        { return c.Opts().Changed() }
func direct(e endpoints) string           { return e.Value("k") }
"#;

#[test]
fn library_result_is_a_guess() {
    let g = graph("go-lib", &[("app/app.go", GO)]);
    // `r.Context()`: not in the repo
    assert_eq!(callers(&g, "endpoints::Value"), (vec!["direct".to_string()], vec!["fromLibrary".to_string()]));
    // `c.PFlags()` is the repo's, but declares `*pflag.FlagSet`; `c.Opts()` returns the repo's `*Options`
    assert_eq!(callers(&g, "Options::Changed"), (vec!["fromRepoType".to_string()], vec!["fromRepoMethod".to_string()]));
}

#[test]
fn unknown_result_of_a_repo_function_stays_sure() {
    // jsoup `el.tag().isFormSubmittable()`: several `tag`s in the repo, the graph cannot tell
    // which one returns what — that is no evidence of a library type, the call stays sure
    let g = graph(
        "go-amb",
        &[(
            "app/app.go",
            r#"package app

type Tag struct{}

func (t *Tag) Submittable() bool { return true }

type Element struct{ t *Tag }

func (e *Element) Tag() *Tag { return e.t }

type Token struct{}

func (k *Token) Tag() string { return "" }

func formData(e *Element) bool { return e.Tag().Submittable() }
func made() bool                { p := new(Tag); return p.Submittable() }
"#,
        )],
    );
    let (sure, _) = callers(&g, "Tag::Submittable");
    assert!(sure.contains(&"formData".to_string()), "{sure:?}");
    assert!(sure.contains(&"made".to_string()), "{sure:?}");
}
