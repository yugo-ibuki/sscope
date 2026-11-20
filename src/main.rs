mod agents;
mod codex;
mod copilot;
mod pi;
mod rpc;
mod scan;
mod settings;

use std::env;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use scan::{Collision, Root};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scope {
    Project,
    Global,
    Unspecified,
}

#[derive(Debug)]
pub struct Skill {
    pub name: String,
    pub path: PathBuf,
    pub scope: Scope,
    pub note: String,
}

#[derive(Default, Debug)]
pub struct Report {
    pub skills: Vec<Skill>,
    pub warnings: Vec<String>,
}

// Use the same executable check for detection and native list commands.
fn on_path(name: &str) -> bool {
    env::var_os("PATH").is_some_and(|path| {
        env::split_paths(&path).any(|dir| {
            #[cfg(windows)]
            let program = dir.join(format!("{name}.exe"));
            #[cfg(not(windows))]
            let program = dir.join(name);
            let Ok(metadata) = program.metadata() else {
                return false;
            };
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
            }
            #[cfg(not(unix))]
            metadata.is_file()
        })
    })
}

const HELP: &str = "sscope — 現在地で利用できるエージェントスキルを一覧表示

使い方: sscope [--agent 名前]

  引数なし       検出したエージェントのスキルをプロジェクト・グローバル別に表示
  --agent 名前   1つに絞り、スキルのパスと呼び出し条件も表示（例: sscope --agent codex）
  -h, --help     このヘルプを表示
  -V, --version  バージョンを表示

「本体で確認」はエージェントの一覧 API の結果です。
「ローカル配置」は設定ディレクトリで見つかった候補で、実行時の有効状態は未確認です。";

fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let filter = match args.as_slice() {
        [] => None,
        [a] if a == "--help" || a == "-h" => {
            println!("{HELP}");
            return ExitCode::SUCCESS;
        }
        [a] if a == "--version" || a == "-V" => {
            println!("sscope {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        [a, name] if a == "--agent" && !name.is_empty() => {
            Some(name.to_string_lossy().into_owned())
        }
        _ => {
            eprintln!("使い方: sscope [--agent 名前]  （詳細は --help）");
            return ExitCode::from(2);
        }
    };
    match run(filter.as_deref()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) if err.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("sscope: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run(filter: Option<&str>) -> io::Result<()> {
    let cwd = env::current_dir()?.canonicalize()?;
    let home =
        env::home_dir().ok_or_else(|| io::Error::other("ホームディレクトリを取得できません"))?;
    let home = home.canonicalize().unwrap_or(home);
    let repo = cwd.ancestors().find(|dir| {
        let marker = dir.join(".git");
        marker.is_dir() || marker.is_file()
    });
    let mut agents = agents::detected(&cwd, repo, &home);
    let native_codex = on_path("codex");
    let codex_roots = agents::codex_roots(&cwd, repo, &home);
    let codex_home = env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".codex"));
    if native_codex || codex_home.is_dir() || repo.unwrap_or(&cwd).join(".codex").is_dir() {
        agents.push(agents::Agent {
            name: "Codex",
            roots: codex_roots,
            collision: Collision::Keep,
        });
    }

    if let Some(requested) = filter {
        let id = normalize(requested);
        agents.retain(|a| {
            let name = normalize(a.name);
            name == id || (name == "githubcopilotcli" && id == "copilot")
        });
        if agents.is_empty() {
            return Err(io::Error::other(format!(
                "指定されたエージェントを検出できません: {}",
                safe(requested)
            )));
        }
    }
    let mut out = io::BufWriter::new(io::stdout().lock());
    writeln!(out, "現在地  {}", safe(&cwd.to_string_lossy()))?;
    let mut results = Vec::new();
    for agent in &agents {
        let native = match agent.name {
            "Codex" if native_codex => Some(codex::list(&cwd)),
            "Pi" if on_path("pi") => Some(pi::list(&cwd)),
            "GitHub Copilot CLI" if on_path("copilot") => Some(copilot::list(&cwd)),
            _ => None,
        };
        let (label, report) = match native {
            Some(Ok(report)) => ("本体で確認", report),
            result => {
                let mut report = settings::local(agent, &cwd, repo, &home);
                if let Some(Err(err)) = result {
                    report.warnings.insert(0, err);
                }
                ("ローカル配置・有効状態未確認", report)
            }
        };
        results.push((agent.name, label, report));
    }
    results.sort_by_key(|(name, label, _)| (*label != "本体で確認", *name));
    if filter.is_none() && !results.is_empty() {
        writeln!(out, "パスと呼び出し条件: sscope --agent <名前>")?;
    }
    for (name, label, report) in results {
        render(
            &mut out,
            name,
            label,
            report,
            repo.unwrap_or(&cwd),
            &home,
            filter.is_some(),
        )?;
    }
    if agents.is_empty() {
        writeln!(out, "\n対応エージェントを検出できませんでした。")?;
        let mut roots = Vec::new();
        for parent in cwd.ancestors() {
            if parent != home {
                roots.push(Root {
                    path: parent.join(".agents/skills"),
                    scope: Scope::Project,
                    depth: 1,
                });
            }
            if repo.is_none() || repo == Some(parent) {
                break;
            }
        }
        roots.push(Root {
            path: home.join(".agents/skills"),
            scope: Scope::Global,
            depth: 1,
        });
        let shared = scan::scan(&roots, Collision::Keep);
        if !shared.skills.is_empty() || !shared.warnings.is_empty() {
            render(
                &mut out,
                "共有スキル",
                "利用エージェント未特定",
                shared,
                repo.unwrap_or(&cwd),
                &home,
                true,
            )?;
        }
    }
    out.flush()
}

fn normalize(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn safe(value: &str) -> String {
    let mut text = String::new();
    for c in value.chars() {
        if c.is_control() {
            text.extend(c.escape_default());
        } else {
            text.push(c);
        }
    }
    text
}

fn display_path(path: &Path, scope: Scope, project: &Path, home: &Path) -> String {
    let short = match scope {
        Scope::Unspecified => None,
        Scope::Project => path
            .strip_prefix(project)
            .ok()
            .map(|rest| Path::new(".").join(rest)),
        Scope::Global => path
            .strip_prefix(home)
            .ok()
            .map(|rest| Path::new("~").join(rest)),
    };
    safe(&short.as_deref().unwrap_or(path).to_string_lossy())
}

fn render(
    out: &mut impl Write,
    name: &str,
    label: &str,
    mut report: Report,
    project: &Path,
    home: &Path,
    detailed: bool,
) -> io::Result<()> {
    writeln!(out, "\n{name}  [{label}]")?;
    report
        .skills
        .sort_by(|a, b| (&a.scope, &a.name, &a.path).cmp(&(&b.scope, &b.name, &b.path)));
    if report.skills.is_empty() {
        let empty = if !report.warnings.is_empty() {
            "一覧を確定できませんでした"
        } else if label == "本体で確認" {
            "有効なスキルはありません"
        } else {
            "表示対象のローカルスキルはありません"
        };
        writeln!(out, "  {empty}")?;
    }
    for (scope, heading) in [
        (Scope::Project, "プロジェクト"),
        (Scope::Global, "グローバル"),
        (Scope::Unspecified, "範囲未特定"),
    ] {
        let entries: Vec<_> = report.skills.iter().filter(|s| s.scope == scope).collect();
        if entries.is_empty() {
            continue;
        }
        writeln!(out, "  {heading} ({}件)", entries.len())?;
        if !detailed {
            compact_names(out, &entries)?;
            continue;
        }
        let width = entries
            .iter()
            .map(|s| safe(&s.name).chars().count())
            .max()
            .unwrap_or(0)
            .min(40);
        for skill in entries {
            let note = if skill.note.is_empty() {
                String::new()
            } else {
                format!("  ({})", safe(&skill.note))
            };
            writeln!(
                out,
                "    {:width$}  {}{note}",
                safe(&skill.name),
                display_path(&skill.path, scope, project, home)
            )?;
        }
    }
    for warning in report.warnings {
        writeln!(out, "  注意: {}", safe(&warning))?;
    }
    Ok(())
}

// Same-name definitions at different paths remain individually visible in --agent.
fn compact_names(out: &mut impl Write, entries: &[&Skill]) -> io::Result<()> {
    let mut line = String::new();
    for equal in entries.chunk_by(|a, b| a.name == b.name) {
        let mut item = safe(&equal[0].name);
        if equal.len() > 1 {
            item.push_str(&format!(" ×{}", equal.len()));
        }
        if !line.is_empty() && line.chars().count() + item.chars().count() + 2 > 76 {
            writeln!(out, "    {line}")?;
            line.clear();
        }
        if !line.is_empty() {
            line.push_str(", ");
        }
        line.push_str(&item);
    }
    if !line.is_empty() {
        writeln!(out, "    {line}")?;
    }
    Ok(())
}
