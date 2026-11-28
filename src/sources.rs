//! Which of your sources a folder syncs ([#43]). Your own config lists the
//! sources you trust (the catalog); `<folder>/.salash/config.toml` picks from
//! them. `salash sources` and the first start in a folder write that pick.
//!
//! [#43]: https://github.com/martin-dekastello/salash/issues/43

use crate::config::{self, same_repo, same_site, Config};
use crate::instance::Instance;
use anyhow::{bail, Context, Result};
use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(clap::Subcommand)]
pub enum SourcesCmd {
    /// List the sources in your config and which of them this folder syncs (the default).
    List,
    /// Sync these sources here: owner/repo, a Jira URL, or a number from `salash sources`.
    Add {
        #[arg(required = true)]
        sources: Vec<String>,
    },
    /// Stop syncing these sources here. Their notes stay in the vault.
    Remove {
        #[arg(required = true)]
        sources: Vec<String>,
    },
}

/// A source in your own config.
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    pub kind: Kind,
    /// GitHub `repo` or Jira `url`, as your config spells it.
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Github,
    Jira,
}

impl Kind {
    /// The folder config's array, and the key that names a source in it.
    fn keys(self) -> (&'static str, &'static str) {
        match self {
            Kind::Github => ("github", "repo"),
            Kind::Jira => ("jira", "url"),
        }
    }
}

impl Source {
    fn same(&self, kind: Kind, name: &str) -> bool {
        self.kind == kind
            && match kind {
                Kind::Github => same_repo(&self.name, name),
                Kind::Jira => same_site(&self.name, name),
            }
    }
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let kind = match self.kind {
            Kind::Github => "github",
            Kind::Jira => "jira",
        };
        write!(f, "{kind:<7} {}", self.name)
    }
}

/// Your sources, GitHub first, numbered from 1 in this order everywhere.
pub fn catalog(cfg: &Config) -> Vec<Source> {
    let gh = cfg.catalog_github.iter().map(|g| Source { kind: Kind::Github, name: g.repo.clone() });
    let jira = cfg.catalog_jira.iter().map(|j| Source { kind: Kind::Jira, name: j.url.clone() });
    gh.chain(jira).collect()
}

/// Whether the folder already syncs this source.
fn synced(cfg: &Config, s: &Source) -> bool {
    match s.kind {
        Kind::Github => cfg.github.iter().any(|g| same_repo(&g.repo, &s.name)),
        Kind::Jira => cfg.jira.iter().any(|j| same_site(&j.url, &s.name)),
    }
}

/// "owner/repo" for a GitHub remote URL: `git@github.com:o/r.git`,
/// `https://github.com/o/r`, `ssh://git@github.com/o/r.git`.
pub fn github_repo(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("git@github.com:")
        .or_else(|| url.split_once("github.com/").map(|(_, r)| r))?;
    let rest = rest.trim_end_matches('/');
    let rest = rest.strip_suffix(".git").unwrap_or(rest);
    let (owner, name) = rest.split_once('/')?;
    if owner.is_empty() || name.is_empty() || name.contains('/') {
        return None;
    }
    Some(format!("{owner}/{name}"))
}

/// The GitHub repos the folder's git remotes point at.
fn remotes(folder: &Path) -> Vec<String> {
    let Ok(out) = Command::new("git").arg("-C").arg(folder).args(["remote", "-v"]).output() else {
        return vec![];
    };
    let mut repos: Vec<String> = vec![];
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if let Some(r) = line.split_whitespace().nth(1).and_then(github_repo) {
            if !repos.iter().any(|x| same_repo(x, &r)) {
                repos.push(r);
            }
        }
    }
    repos
}

/// Whether `folder` is `path` or inside it.
fn inside(folder: &Path, path: &Path) -> bool {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    folder.starts_with(path)
}

/// The catalog entries that belong to this folder: a git remote names the
/// repo, or the folder is the source's `path` (or inside it).
fn matching(folder: &Path, cfg: &Config, remotes: &[String]) -> Vec<Source> {
    let mut out = vec![];
    for g in &cfg.catalog_github {
        let by_path = g.path.as_deref().is_some_and(|p| inside(folder, p));
        if by_path || remotes.iter().any(|r| same_repo(r, &g.repo)) {
            out.push(Source { kind: Kind::Github, name: g.repo.clone() });
        }
    }
    for j in &cfg.catalog_jira {
        if j.path.as_deref().is_some_and(|p| inside(folder, p)) {
            out.push(Source { kind: Kind::Jira, name: j.url.clone() });
        }
    }
    out
}

/// An argument to `add` / `remove`: a number from the list, `github:`/`jira:`
/// prefixed or bare. `https://…` is Jira, anything else a GitHub repo.
fn parse_arg(arg: &str, catalog: &[Source]) -> Result<(Kind, String)> {
    if let Ok(n) = arg.parse::<usize>() {
        return match n.checked_sub(1).and_then(|i| catalog.get(i)) {
            Some(s) => Ok((s.kind, s.name.clone())),
            None => bail!("no source {n}: `salash sources` lists {}", catalog.len()),
        };
    }
    if let Some(r) = arg.strip_prefix("github:") {
        return Ok((Kind::Github, r.to_string()));
    }
    if let Some(u) = arg.strip_prefix("jira:") {
        return Ok((Kind::Jira, u.to_string()));
    }
    if arg.starts_with("https://") || arg.starts_with("http://") {
        return Ok((Kind::Jira, arg.to_string()));
    }
    if arg.split('/').count() != 2 || arg.split('/').any(str::is_empty) {
        bail!("{arg:?} is neither owner/repo nor a Jira URL");
    }
    Ok((Kind::Github, arg.to_string()))
}

// ---------- the folder's config.toml ----------

const HEADER: &str = "\
# salash for this folder. [[github]] / [[jira]] pick which of the sources in your own
# config sync here (`salash sources`); they may add search / jql / id_prefix / worktrees.
# vault, notes_dir, prefix, local_prefix, bell, mouse and worktrees override yours.
";

fn folder_config(inst: &Instance) -> PathBuf {
    inst.dir.join("config.toml")
}

fn read_doc(path: &Path) -> Result<toml_edit::DocumentMut> {
    match std::fs::read_to_string(path) {
        Ok(text) => text.parse().with_context(|| format!("parsing {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Default::default()),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

fn write_doc(inst: &Instance, doc: &toml_edit::DocumentMut) -> Result<()> {
    inst.ensure_dir()?;
    let path = folder_config(inst);
    let mut text = doc.to_string();
    if !path.exists() {
        text = format!("{HEADER}\n{text}");
    }
    std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))
}

fn has(doc: &toml_edit::DocumentMut, kind: Kind, name: &str) -> bool {
    let (arr, key) = kind.keys();
    doc.get(arr).and_then(|a| a.as_array_of_tables()).into_iter().flat_map(|a| a.iter()).any(|t| {
        t.get(key).and_then(|v| v.as_str()).is_some_and(|v| Source { kind, name: v.into() }.same(kind, name))
    })
}

/// Add `[[github]] repo = …` / `[[jira]] url = …`; false if already there.
fn pick(doc: &mut toml_edit::DocumentMut, kind: Kind, name: &str) -> Result<bool> {
    if has(doc, kind, name) {
        return Ok(false);
    }
    let (arr, key) = kind.keys();
    let item = doc.entry(arr).or_insert_with(|| toml_edit::Item::ArrayOfTables(Default::default()));
    let Some(tables) = item.as_array_of_tables_mut() else {
        bail!("`{arr}` in the folder's config.toml isn't a list of [[{arr}]] tables");
    };
    let mut t = toml_edit::Table::new();
    t.insert(key, toml_edit::value(name));
    tables.push(t);
    Ok(true)
}

/// Remove every entry for the source; false if there was none.
fn unpick(doc: &mut toml_edit::DocumentMut, kind: Kind, name: &str) -> bool {
    let (arr, key) = kind.keys();
    let Some(tables) = doc.get_mut(arr).and_then(|a| a.as_array_of_tables_mut()) else { return false };
    let before = tables.len();
    tables.retain(|t| {
        !t.get(key).and_then(|v| v.as_str()).is_some_and(|v| Source { kind, name: v.into() }.same(kind, name))
    });
    let removed = tables.len() != before;
    if tables.is_empty() {
        doc.remove(arr);
    }
    removed
}

/// Add a GitHub repo to your own config, with this folder as its clone.
/// Appended as text, so your comments stay.
fn catalog_github(repo: &str, folder: &Path) -> Result<()> {
    let path = config::config_path();
    let mut text = std::fs::read_to_string(&path).unwrap_or_default();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    let quote = |s: &str| toml_edit::Value::from(s).to_string();
    text.push_str(&format!(
        "\n[[github]]\nrepo = {}\npath = {}\n",
        quote(repo).trim(),
        quote(&folder.to_string_lossy()).trim()
    ));
    std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))
}

fn ask(input: &mut impl BufRead, out: &mut impl Write, q: &str) -> Result<Option<String>> {
    write!(out, "{q}")?;
    out.flush()?;
    let mut line = String::new();
    if input.read_line(&mut line)? == 0 {
        writeln!(out)?;
        return Ok(None);
    }
    Ok(Some(line.trim().to_string()))
}

fn yes(answer: Option<&str>, default: bool) -> bool {
    match answer.map(str::to_ascii_lowercase).as_deref() {
        None | Some("") => default,
        Some(a) => a.starts_with('y'),
    }
}

fn interactive() -> bool {
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

// ---------- `salash sources` ----------

pub fn run(cmd: Option<SourcesCmd>, inst: &Instance) -> Result<()> {
    let load = || Ok::<_, anyhow::Error>(Config::load_for(Some(&inst.dir))?.0);
    let cfg = load()?;
    let cat = catalog(&cfg);
    let path = folder_config(inst);
    match cmd.unwrap_or(SourcesCmd::List) {
        SourcesCmd::List => {
            if cat.is_empty() {
                println!("{}", cfg.no_sources_hint());
                return Ok(());
            }
            println!("Sources in {} (* = synced in {}):", config::shorten(&config::config_path()), config::shorten(&inst.folder));
            for (i, s) in cat.iter().enumerate() {
                let mark = if synced(&cfg, s) { '*' } else { ' ' };
                println!("{mark} {:>2}  {s}", i + 1);
            }
        }
        SourcesCmd::Add { sources } => {
            let mut doc = read_doc(&path)?;
            let stdin = std::io::stdin();
            // Every argument must name a source before anything is written.
            let args = sources.iter().map(|a| parse_arg(a, &cat)).collect::<Result<Vec<_>>>()?;
            for (kind, name) in args {
                let known = cat.iter().find(|s| s.same(kind, &name)).cloned();
                let s = match (known, kind) {
                    (Some(s), _) => s,
                    (None, Kind::Jira) => bail!(
                        "{name} isn't in {}: add a [[jira]] with url and email there first",
                        config::shorten(&config::config_path())
                    ),
                    (None, Kind::Github) => {
                        let q = format!("{name} isn't in your config. Add it there, with this folder as its clone? [y/N] ");
                        let answer = if interactive() { ask(&mut stdin.lock(), &mut std::io::stdout(), &q)? } else { None };
                        if !yes(answer.as_deref(), false) {
                            bail!(
                                "{name} isn't in {}: add [[github]] repo = \"{name}\" there first",
                                config::shorten(&config::config_path())
                            );
                        }
                        catalog_github(&name, &inst.folder)?;
                        println!("Added {name} to {}", config::shorten(&config::config_path()));
                        Source { kind, name }
                    }
                };
                if pick(&mut doc, s.kind, &s.name)? {
                    println!("+ {s}");
                } else {
                    println!("  {s} (already synced)");
                }
            }
            write_doc(inst, &doc)?;
            println!("Wrote {}; a running salash picks it up when it starts again", config::shorten(&path));
        }
        SourcesCmd::Remove { sources } => {
            if !path.exists() {
                println!("{} syncs nothing yet", config::shorten(&inst.folder));
                return Ok(());
            }
            let mut doc = read_doc(&path)?;
            let args = sources.iter().map(|a| parse_arg(a, &cat)).collect::<Result<Vec<_>>>()?;
            for (kind, name) in args {
                if unpick(&mut doc, kind, &name) {
                    println!("- {name}");
                } else {
                    println!("  {name} (wasn't synced)");
                }
            }
            write_doc(inst, &doc)?;
            println!("Wrote {}", config::shorten(&path));
        }
    }
    Ok(())
}

// ---------- first start in a folder ----------

/// On the first `salash` in a folder (no `.salash/config.toml` yet) and in a
/// terminal, ask which vault it uses and which sources it syncs, and write
/// the answer. Several folders can pick the same vault and the same source.
pub fn init_if_new(inst: &Instance) -> Result<()> {
    if folder_config(inst).exists() || !interactive() {
        return Ok(());
    }
    let created = !config::config_path().exists();
    let cfg = Config::load_for(Some(&inst.dir))?.0;
    let mut out = std::io::stdout();
    if created {
        writeln!(out, "Created {}: your settings and sources go there", config::shorten(&config::config_path()))?;
    }
    let remotes = remotes(&inst.folder);
    init(inst, &cfg, &remotes, &mut std::io::stdin().lock(), &mut out, &mut |repo| catalog_github(repo, &inst.folder))
}

/// The prompt behind `init_if_new`, with its input, output and the write to
/// your own config passed in.
fn init(
    inst: &Instance,
    cfg: &Config,
    remotes: &[String],
    input: &mut impl BufRead,
    out: &mut impl Write,
    add_to_own: &mut dyn FnMut(&str) -> Result<()>,
) -> Result<()> {
    writeln!(out, "New salash folder: {}", config::shorten(&inst.folder))?;
    let mut doc = read_doc(&folder_config(inst))?;

    let default_vault = config::shorten(&cfg.vault);
    let vault = ask(input, out, &format!("Obsidian vault for its notes [{default_vault}]: "))?;
    let vault = vault.filter(|v| !v.is_empty()).unwrap_or(default_vault);
    doc.insert("vault", toml_edit::value(vault));

    let mut cat = catalog(cfg);
    let mut picked = matching(&inst.folder, cfg, remotes);
    let new: Vec<&String> = remotes.iter().filter(|r| !cat.iter().any(|s| s.same(Kind::Github, r))).collect();
    for r in new {
        let q = format!("This folder is a clone of {r}, which isn't in your config. Add it there? [Y/n] ");
        if yes(ask(input, out, &q)?.as_deref(), true) {
            add_to_own(r)?;
            writeln!(out, "Added {r} to {}", config::shorten(&config::config_path()))?;
            let s = Source { kind: Kind::Github, name: r.clone() };
            let at = cat.iter().filter(|s| s.kind == Kind::Github).count();
            cat.insert(at, s.clone());
            picked.push(s);
        }
    }

    if cat.is_empty() {
        writeln!(out, "{}", cfg.no_sources_hint())?;
        writeln!(out, "Add them there, then run `salash sources add <repo|url>` here.")?;
    } else {
        writeln!(out, "Sources in your config ([x] = this folder's repo):")?;
        for (i, s) in cat.iter().enumerate() {
            let mark = if picked.contains(s) { 'x' } else { ' ' };
            writeln!(out, "  [{mark}] {:>2}  {s}", i + 1)?;
        }
        let default: Vec<String> =
            cat.iter().enumerate().filter(|(_, s)| picked.contains(s)).map(|(i, _)| (i + 1).to_string()).collect();
        let default = if default.is_empty() { "none".to_string() } else { default.join(" ") };
        let chosen = loop {
            let Some(a) = ask(input, out, &format!("Sync which here? Numbers, or none [{default}]: "))? else {
                break picked.clone();
            };
            let a = if a.is_empty() { default.clone() } else { a };
            match choose(&a, &cat) {
                Ok(c) => break c,
                Err(e) => writeln!(out, "{e}")?,
            }
        };
        for s in &chosen {
            pick(&mut doc, s.kind, &s.name)?;
        }
    }
    write_doc(inst, &doc)?;
    writeln!(out, "Wrote {} (change it with `salash sources`)", config::shorten(&folder_config(inst)))?;
    Ok(())
}

/// "1 3", "1,3" or "none".
fn choose(answer: &str, cat: &[Source]) -> Result<Vec<Source>> {
    if answer.eq_ignore_ascii_case("none") {
        return Ok(vec![]);
    }
    let mut out: Vec<Source> = vec![];
    for t in answer.split(|c: char| c == ',' || c.is_whitespace()).filter(|t| !t.is_empty()) {
        let s = t
            .parse::<usize>()
            .ok()
            .and_then(|n| n.checked_sub(1))
            .and_then(|i| cat.get(i))
            .with_context(|| format!("{t:?} isn't one of 1–{}", cat.len()))?;
        if !out.contains(s) {
            out.push(s.clone());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{GithubSource, JiraSource};

    fn temp() -> Instance {
        let dir = std::env::temp_dir().join(format!("salash-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        Instance::new(Some(&dir)).unwrap()
    }

    fn gh(repo: &str, path: Option<&Path>) -> GithubSource {
        GithubSource { repo: repo.into(), path: path.map(Path::to_path_buf), id_prefix: None, search: None, token_env: None, worktrees: None }
    }

    fn jira(url: &str, path: Option<&Path>) -> JiraSource {
        JiraSource { url: url.into(), email: "me@example.com".into(), token_env: None, jql: None, path: path.map(Path::to_path_buf), worktrees: None }
    }

    fn cfg(github: Vec<GithubSource>, jira: Vec<JiraSource>) -> Config {
        Config { vault: "/vault".into(), catalog_github: github, catalog_jira: jira, ..Config::default() }
    }

    /// Run the prompt with `answers` as its input; returns what it printed and
    /// the repos it added to your config.
    fn run_init(inst: &Instance, cfg: &Config, remotes: &[&str], answers: &str) -> (String, Vec<String>) {
        let remotes: Vec<String> = remotes.iter().map(|r| r.to_string()).collect();
        let mut out = vec![];
        let mut added = vec![];
        init(inst, cfg, &remotes, &mut answers.as_bytes(), &mut out, &mut |r| {
            added.push(r.to_string());
            Ok(())
        })
        .unwrap();
        (String::from_utf8(out).unwrap(), added)
    }

    fn written(inst: &Instance) -> toml::Table {
        toml::from_str(&std::fs::read_to_string(folder_config(inst)).unwrap()).unwrap()
    }

    fn names(t: &toml::Table, arr: &str, key: &str) -> Vec<String> {
        t.get(arr)
            .and_then(|a| a.as_array())
            .into_iter()
            .flatten()
            .map(|e| e[key].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn github_remotes_parse() {
        for url in [
            "git@github.com:owner/repo.git",
            "git@github.com:owner/repo",
            "https://github.com/owner/repo",
            "https://github.com/owner/repo.git",
            "https://github.com/owner/repo/",
            "ssh://git@github.com/owner/repo.git",
            "https://user@github.com/owner/repo.git",
        ] {
            assert_eq!(github_repo(url).as_deref(), Some("owner/repo"), "{url}");
        }
        for url in ["git@gitlab.com:owner/repo.git", "https://github.com/owner", "https://github.com/a/b/c", "/local/path"] {
            assert_eq!(github_repo(url), None, "{url}");
        }
    }

    #[test]
    fn args_name_sources() {
        let cat = vec![Source { kind: Kind::Github, name: "a/one".into() }, Source { kind: Kind::Jira, name: "https://co.atlassian.net".into() }];
        assert_eq!(parse_arg("2", &cat).unwrap(), (Kind::Jira, "https://co.atlassian.net".into()));
        assert!(parse_arg("0", &cat).is_err());
        assert!(parse_arg("3", &cat).is_err());
        assert_eq!(parse_arg("a/two", &cat).unwrap(), (Kind::Github, "a/two".into()));
        assert_eq!(parse_arg("github:a/two", &cat).unwrap(), (Kind::Github, "a/two".into()));
        assert_eq!(parse_arg("https://x.net", &cat).unwrap().0, Kind::Jira);
        assert_eq!(parse_arg("jira:https://x.net", &cat).unwrap().0, Kind::Jira);
        for bad in ["nonsense", "a/", "/b", "a/b/c"] {
            assert!(parse_arg(bad, &cat).is_err(), "{bad}");
        }
    }

    #[test]
    fn pick_and_unpick_are_idempotent_and_keep_the_rest() {
        let mut doc: toml_edit::DocumentMut =
            "# mine\nvault = \"/team\"  # shared\n[[github]]\nrepo = \"a/one\"\nsearch = \"label:bug\"\n".parse().unwrap();
        assert!(!pick(&mut doc, Kind::Github, "A/One").unwrap(), "already there, any case");
        assert!(pick(&mut doc, Kind::Jira, "https://co.atlassian.net").unwrap());
        assert!(!pick(&mut doc, Kind::Jira, "https://co.atlassian.net/").unwrap());
        let text = doc.to_string();
        assert!(text.contains("# mine") && text.contains("# shared"), "comments kept: {text}");
        assert!(text.contains("search = \"label:bug\""));
        assert!(unpick(&mut doc, Kind::Github, "a/one"));
        assert!(!unpick(&mut doc, Kind::Github, "a/one"));
        assert!(!unpick(&mut doc, Kind::Github, "never/there"));
        let t: toml::Table = toml::from_str(&doc.to_string()).unwrap();
        assert!(t.get("github").is_none());
        assert_eq!(names(&t, "jira", "url"), ["https://co.atlassian.net"]);
        assert_eq!(t["vault"].as_str(), Some("/team"));
    }

    #[test]
    fn a_folder_config_with_a_scalar_github_is_refused() {
        let mut doc: toml_edit::DocumentMut = "github = \"oops\"\n".parse().unwrap();
        assert!(pick(&mut doc, Kind::Github, "a/one").is_err());
    }

    #[test]
    fn sources_match_by_remote_or_path() {
        let inst = temp();
        let sub = inst.folder.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let c = cfg(
            vec![gh("a/one", None), gh("a/two", Some(&inst.folder)), gh("a/three", Some(Path::new("/elsewhere")))],
            vec![jira("https://co.atlassian.net", Some(&inst.folder)), jira("https://other.net", None)],
        );
        let m: Vec<String> = matching(&inst.folder, &c, &["A/ONE".into()]).iter().map(|s| s.name.clone()).collect();
        assert_eq!(m, ["a/one", "a/two", "https://co.atlassian.net"]);
        let sub = std::fs::canonicalize(&sub).unwrap();
        let m: Vec<String> = matching(&sub, &c, &[]).iter().map(|s| s.name.clone()).collect();
        assert_eq!(m, ["a/two", "https://co.atlassian.net"], "a folder inside a source's path");
        std::fs::remove_dir_all(&inst.folder).ok();
    }

    #[test]
    fn init_defaults_to_your_vault_and_this_folders_sources() {
        let inst = temp();
        let c = cfg(vec![gh("a/one", None), gh("a/two", None)], vec![jira("https://co.atlassian.net", None)]);
        let (out, added) = run_init(&inst, &c, &["a/two"], "\n\n");
        assert!(out.contains("[x]  2  github  a/two"), "{out}");
        assert!(out.contains("[2]: "), "{out}");
        assert!(added.is_empty());
        let t = written(&inst);
        assert_eq!(t["vault"].as_str(), Some("/vault"));
        assert_eq!(names(&t, "github", "repo"), ["a/two"]);
        assert!(t.get("jira").is_none());
        std::fs::remove_dir_all(&inst.folder).ok();
    }

    #[test]
    fn init_takes_a_shared_vault_and_any_sources() {
        let inst = temp();
        let c = cfg(vec![gh("a/one", None)], vec![jira("https://co.atlassian.net", None)]);
        let (out, _) = run_init(&inst, &c, &[], "~/Shared vault\n7\n1 x\n2,1,2\n");
        assert_eq!(out.matches("isn't one of 1–2").count(), 2, "bad answers are asked again: {out}");
        let t = written(&inst);
        assert_eq!(t["vault"].as_str(), Some("~/Shared vault"));
        assert_eq!(names(&t, "github", "repo"), ["a/one"]);
        assert_eq!(names(&t, "jira", "url"), ["https://co.atlassian.net"]);
        // Reading it back: both resolve against the catalog.
        std::fs::remove_dir_all(&inst.folder).ok();
    }

    #[test]
    fn init_none_and_eof_still_write_a_config() {
        let inst = temp();
        let c = cfg(vec![gh("a/one", None)], vec![]);
        run_init(&inst, &c, &["a/one"], "\nnone\n");
        let t = written(&inst);
        assert!(t.get("github").is_none());
        assert_eq!(t["vault"].as_str(), Some("/vault"));
        std::fs::remove_dir_all(&inst.folder).ok();

        // Input ends at once (ctrl-d): the defaults, so the folder's own repo.
        let inst = temp();
        let (out, _) = run_init(&inst, &c, &["a/one"], "");
        assert!(out.contains("Wrote"), "{out}");
        assert_eq!(names(&written(&inst), "github", "repo"), ["a/one"]);
        std::fs::remove_dir_all(&inst.folder).ok();
    }

    #[test]
    fn init_offers_to_add_an_uncatalogued_clone_to_your_config() {
        let inst = temp();
        let c = cfg(vec![gh("a/one", None)], vec![jira("https://co.atlassian.net", None)]);
        let (out, added) = run_init(&inst, &c, &["me/new"], "\n\n\n");
        assert_eq!(added, ["me/new"]);
        assert!(out.contains("[x]  2  github  me/new"), "listed after your GitHub sources: {out}");
        assert!(out.contains("[ ]  3  jira"), "{out}");
        assert_eq!(names(&written(&inst), "github", "repo"), ["me/new"]);
        std::fs::remove_dir_all(&inst.folder).ok();

        let inst = temp();
        let (_, added) = run_init(&inst, &c, &["me/new"], "\nn\n\n");
        assert!(added.is_empty());
        assert!(written(&inst).get("github").is_none());
        std::fs::remove_dir_all(&inst.folder).ok();
    }

    #[test]
    fn init_with_an_empty_catalog_writes_the_vault_and_says_where_sources_go() {
        let inst = temp();
        let (out, _) = run_init(&inst, &cfg(vec![], vec![]), &[], "\n");
        assert!(out.contains("No [[github]] or [[jira]] sources"), "{out}");
        assert!(!out.contains("Sync which"), "{out}");
        assert_eq!(written(&inst)["vault"].as_str(), Some("/vault"));
        assert!(inst.dir.join(".gitignore").exists(), "the .salash dir ignores itself");
        let text = std::fs::read_to_string(folder_config(&inst)).unwrap();
        assert!(text.starts_with(HEADER), "header on top: {text}");
        std::fs::remove_dir_all(&inst.folder).ok();
    }
}
