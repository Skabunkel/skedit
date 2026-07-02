//! skedit — umbrella CLI. For now: the template manager
//! (docs/02-template-system.md). Starlark editing lives in skedit-cli.

use std::path::PathBuf;
use std::process::{Command, ExitCode};

const USAGE: &str = "\
usage:
  skedit template add github:USER/REPO[@REF] [--name NAME]
  skedit template list
  skedit template update NAME
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args_ref: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = match args_ref.as_slice() {
        ["template", "add", spec, rest @ ..] => add(spec, rest),
        ["template", "list"] => list(),
        ["template", "update", name] => update(name),
        _ => {
            eprint!("{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn templates_dir() -> PathBuf {
    ske::dirs::templates_dir()
}

fn add(spec: &str, rest: &[&str]) -> Result<(), String> {
    let repo = spec
        .strip_prefix("github:")
        .ok_or("template spec must look like github:user/repo[@ref]")?;
    let (repo, git_ref) = match repo.split_once('@') {
        Some((r, g)) => (r, Some(g)),
        None => (repo, None),
    };
    if repo.split('/').count() != 2 {
        return Err("expected github:user/repo".into());
    }

    let default_name = repo
        .rsplit('/')
        .next()
        .unwrap_or(repo)
        .trim_start_matches("skedit-")
        .to_string();
    let name = match rest {
        [] => default_name,
        ["--name", n] => (*n).to_string(),
        _ => return Err(format!("unexpected arguments: {rest:?}")),
    };

    let dest = templates_dir().join(&name);
    if dest.exists() {
        return Err(format!(
            "'{name}' already installed at {} (use `skedit template update {name}`)",
            dest.display()
        ));
    }
    std::fs::create_dir_all(dest.parent().unwrap())
        .map_err(|e| format!("cannot create templates dir: {e}"))?;

    let url = format!("https://github.com/{repo}.git");
    let mut cmd = Command::new("git");
    cmd.args(["clone", "--depth", "1"]);
    if let Some(r) = git_ref {
        cmd.args(["--branch", r]);
    }
    cmd.arg(&url).arg(&dest);
    let status = cmd
        .status()
        .map_err(|e| format!("failed to run git: {e}"))?;
    if !status.success() {
        return Err(format!("git clone of {url} failed"));
    }

    // Show what the template will do before the user runs it (audit step).
    let set = ske::template::TemplateSet::load(&[dest.clone()]);
    if set.is_empty() {
        eprintln!(
            "warning: no *.skedit.toml manifest found in {} — is this a skedit template repo?",
            dest.display()
        );
    }
    for w in &set.warnings {
        eprintln!("warning: {w}");
    }
    for (m, path) in &set.manifests {
        println!(
            "installed '{}' (engine {}, {} rules) from {}",
            m.template.name,
            m.template.engine,
            m.lark.rules.len(),
            path.display()
        );
        let tc = &m.toolchain;
        for (what, cmd) in [("lsp", &tc.lsp), ("fmt", &tc.fmt), ("lint", &tc.lint)] {
            if let Some(c) = cmd {
                println!("  {what}: runs `{} {}`", c.cmd, c.args.join(" "));
            }
        }
    }
    Ok(())
}

fn list() -> Result<(), String> {
    let dir = templates_dir();
    let set = ske::template::TemplateSet::load(&[dir.clone()]);
    for w in &set.warnings {
        eprintln!("warning: {w}");
    }
    if set.is_empty() {
        println!("no templates installed in {}", dir.display());
        return Ok(());
    }
    for (m, path) in &set.manifests {
        println!(
            "{}\t{}\t{} rules\t{}",
            m.template.name,
            m.template.engine,
            m.lark.rules.len(),
            path.display()
        );
    }
    Ok(())
}

fn update(name: &str) -> Result<(), String> {
    let dest = templates_dir().join(name);
    if !dest.is_dir() {
        return Err(format!("no template '{name}' at {}", dest.display()));
    }
    let status = Command::new("git")
        .args(["-C"])
        .arg(&dest)
        .args(["pull", "--ff-only"])
        .status()
        .map_err(|e| format!("failed to run git: {e}"))?;
    if !status.success() {
        return Err("git pull failed".into());
    }
    Ok(())
}
