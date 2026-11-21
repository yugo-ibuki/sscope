mod agents;
mod codex;
mod copilot;
mod pi;
mod rpc;
mod scan;
mod settings;

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use scan::{Collision, Root};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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

const CONFIRMED_LABEL: &str = "本体で確認";
const USAGE: &str = "使い方: sscope [--agent 名前] [--json]  （詳細は --help）";

const HELP: &str = "sscope — 現在地で利用できるエージェントスキルを一覧表示

使い方: sscope [--agent 名前] [--json]

  引数なし       検出したエージェントのスキルをプロジェクト・グローバル別に表示
  --agent 名前   1つに絞り、スキルのパスと呼び出し条件も表示（例: sscope --agent codex）
  --json         JSON を標準出力に表示（注意・診断は標準エラー出力）
  -h, --help     このヘルプを表示
  -V, --version  バージョンを表示

例: sscope --json
    sscope --agent codex --json

JSON: cwd は現在地、agents は検出したエージェントです。
name は製品名（共有スキルのみの場合は null）、evidence は confirmed / candidate。
skills の name はスキル名、scope は project / global / unspecified。
sources の path は省略しない読み込み元、note は呼び出し条件（なければ null）です。

「本体で確認」はエージェントの一覧 API の結果です。
「ローカルで検出」は設定ディレクトリで見つかった候補で、実行時の有効状態は未確認です。";

fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() == 1 {
        if args[0] == "--help" || args[0] == "-h" {
            println!("{HELP}");
            return ExitCode::SUCCESS;
        }
        if args[0] == "--version" || args[0] == "-V" {
            println!("sscope {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
    }
    let mut filter = None;
    let mut json = false;
    let mut arguments = args.iter();
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--json") if !json => json = true,
            Some("--agent") if filter.is_none() => {
                let Some(name) = arguments.next().and_then(|name| name.to_str()) else {
                    eprintln!("{USAGE}");
                    return ExitCode::from(2);
                };
                if name.is_empty() || name.starts_with('-') || normalize(name).is_empty() {
                    eprintln!("{USAGE}");
                    return ExitCode::from(2);
                }
                filter = Some(name);
            }
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    match run(filter, json) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) if err.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("sscope: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run(filter: Option<&str>, json: bool) -> io::Result<()> {
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
    let mut results = Vec::new();
    for agent in &agents {
        let native = match agent.name {
            "Codex" if native_codex => Some(codex::list(&cwd)),
            "Pi" if on_path("pi") => Some(pi::list(&cwd)),
            "GitHub Copilot CLI" if on_path("copilot") => Some(copilot::list(&cwd)),
            _ => None,
        };
        let (label, report) = match native {
            Some(Ok(report)) => (CONFIRMED_LABEL, report),
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
    results.sort_by_key(|(name, label, _)| (*label != CONFIRMED_LABEL, *name));
    let shared = if agents.is_empty() {
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
        let report = scan::scan(&roots, Collision::Keep);
        Some((roots, report))
    } else {
        None
    };
    if json {
        // Reuse discovery reports; the table renderer intentionally shortens and styles paths.
        let document = json_document(&cwd, &results, shared.as_ref().map(|(_, report)| report))?;
        let mut diagnostics = io::stderr().lock();
        let plain = RenderStyle {
            width: 100,
            color: false,
        };
        for (name, _, report) in &results {
            render_warnings(&mut diagnostics, name, report, plain)?;
        }
        if let Some((_, report)) = &shared {
            render_warnings(&mut diagnostics, "共有スキル", report, plain)?;
        }
        let mut out = io::BufWriter::new(io::stdout().lock());
        out.write_all(&document)?;
        writeln!(out)?;
        return out.flush();
    }
    let style = RenderStyle {
        width: terminal_size::terminal_size()
            .map(|(width, _)| usize::from(width.0))
            .or_else(|| env::var("COLUMNS").ok()?.parse().ok())
            .unwrap_or(100)
            .max(24),
        color: io::stdout().is_terminal()
            && env::var_os("NO_COLOR").is_none_or(|value| value.is_empty())
            && env::var_os("TERM").is_none_or(|value| value != "dumb"),
    };
    let mut out = io::BufWriter::new(io::stdout().lock());
    writeln!(out, "現在地  {}", safe(&cwd.to_string_lossy()))?;
    if filter.is_some() {
        for (name, label, report) in &results {
            render_detail(
                &mut out,
                name,
                label,
                report,
                &agents
                    .iter()
                    .find(|agent| agent.name == *name)
                    .unwrap()
                    .roots,
                repo.unwrap_or(&cwd),
                &home,
                style,
            )?;
        }
    } else {
        render_overview(&mut out, &results, style)?;
    }
    if let Some((roots, shared)) = shared {
        writeln!(out, "\n対応エージェントを検出できませんでした。")?;
        if !shared.skills.is_empty() || !shared.warnings.is_empty() {
            render_detail(
                &mut out,
                "共有スキル",
                "利用エージェント未特定",
                &shared,
                &roots,
                repo.unwrap_or(&cwd),
                &home,
                style,
            )?;
        }
    }
    out.flush()
}

fn json_path(path: &Path) -> io::Result<&str> {
    path.to_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "UTF-8 で表現できないパスがあるため JSON を出力できません",
        )
    })
}

fn json_agent(
    name: Option<&str>,
    evidence: &str,
    report: &Report,
) -> io::Result<serde_json::Value> {
    let mut groups: BTreeMap<(Scope, &str), Vec<(&str, &str)>> = BTreeMap::new();
    for skill in &report.skills {
        groups
            .entry((skill.scope, &skill.name))
            .or_default()
            .push((json_path(&skill.path)?, &skill.note));
    }
    let mut skills = Vec::new();
    for ((scope, name), mut sources) in groups {
        sources.sort_unstable();
        let sources: Vec<_> = sources
            .into_iter()
            .map(|(path, note)| {
                serde_json::json!({"path": path, "note": if note.is_empty() { None } else { Some(note) }})
            })
            .collect();
        let scope = match scope {
            Scope::Project => "project",
            Scope::Global => "global",
            Scope::Unspecified => "unspecified",
        };
        skills.push(serde_json::json!({"name": name, "scope": scope, "sources": sources}));
    }
    Ok(serde_json::json!({"name": name, "evidence": evidence, "skills": skills}))
}

fn json_document(
    cwd: &Path,
    results: &[AgentResult<'_>],
    shared: Option<&Report>,
) -> io::Result<Vec<u8>> {
    let mut agents = Vec::new();
    for (name, label, report) in results {
        let evidence = if *label == CONFIRMED_LABEL {
            "confirmed"
        } else {
            "candidate"
        };
        agents.push(json_agent(Some(name), evidence, report)?);
    }
    if let Some(report) = shared.filter(|report| !report.skills.is_empty()) {
        agents.push(json_agent(None, "candidate", report)?);
    }
    serde_json::to_vec_pretty(&serde_json::json!({"cwd": json_path(cwd)?, "agents": agents}))
        .map_err(io::Error::other)
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

#[derive(Clone, Copy)]
struct RenderStyle {
    width: usize,
    color: bool,
}

impl RenderStyle {
    // No existing table renderer handles display-cell widths or ANSI styling.
    fn paint(self, text: &str, code: &str) -> String {
        if self.color && !code.is_empty() {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_owned()
        }
    }
}

// Keep a small terminal-native palette stable across filtering and result order.
fn agent_color(name: &str) -> &'static str {
    match name {
        "Codex" => "36",
        "Pi" => "35",
        _ => {
            let hash = normalize(name).bytes().fold(0usize, |hash, byte| {
                hash.wrapping_mul(31).wrapping_add(byte as usize)
            });
            ["36", "35", "32", "33"][hash % 4]
        }
    }
}

#[derive(Default)]
struct StyledText(Vec<(String, &'static str)>);

impl StyledText {
    fn push(&mut self, text: impl Into<String>, code: &'static str) {
        self.0.push((text.into(), code));
    }

    fn wrap(&self, width: usize, style: RenderStyle) -> Vec<String> {
        let plain: String = self.0.iter().map(|(text, _)| text.as_str()).collect();
        let mut offset = 0;
        // Reuse the Unicode-aware wrapping before adding ANSI escape sequences.
        wrap_cells(&plain, width)
            .into_iter()
            .map(|line| {
                let end = offset + line.len();
                let mut span_start = 0;
                let mut painted = String::new();
                for (text, code) in &self.0 {
                    let span_end = span_start + text.len();
                    let start = offset.max(span_start);
                    let stop = end.min(span_end);
                    if start < stop {
                        painted.push_str(
                            &style.paint(&text[start - span_start..stop - span_start], code),
                        );
                    }
                    span_start = span_end;
                }
                offset = end;
                painted
            })
            .collect()
    }
}

// Use the actual discovery roots: compatibility sources describe their layout,
// while custom product roots get a neutral label rather than claiming ownership.
fn source_badge(
    path: &Path,
    scope: Scope,
    name: &str,
    roots: &[Root],
) -> Option<(&'static str, &'static str)> {
    let root = roots
        .iter()
        .filter(|root| root.scope == scope && path.starts_with(&root.path))
        .max_by_key(|root| root.path.components().count())?;
    if scope == Scope::Unspecified {
        return None;
    }
    [
        (".agents/skills", "共通", ""),
        ("agents/skills", "共通", ""),
        (".codex/skills", "Codex", agent_color("Codex")),
        (".pi/agent/skills", "Pi", agent_color("Pi")),
        (".pi/skills", "Pi", agent_color("Pi")),
        (".claude/skills", "Claude Code", agent_color("Claude Code")),
        (".cursor/skills", "Cursor", agent_color("Cursor")),
        (
            ".copilot/skills",
            "GitHub Copilot CLI",
            agent_color("GitHub Copilot CLI"),
        ),
        (
            ".github/skills",
            "GitHub Copilot CLI",
            agent_color("GitHub Copilot CLI"),
        ),
        (".gemini/skills", "Gemini CLI", agent_color("Gemini CLI")),
    ]
    .into_iter()
    .find_map(|(layout, label, code)| root.path.ends_with(layout).then_some((label, code)))
    .or(Some(("設定先", agent_color(name))))
}

const SCOPES: [(Scope, &str, &str); 3] = [
    (Scope::Project, "プロジェクト", "32"),
    (Scope::Global, "グローバル", "35"),
    (Scope::Unspecified, "範囲未特定", "33"),
];

type AgentResult<'a> = (&'a str, &'a str, Report);

fn render_overview(
    out: &mut impl Write,
    results: &[AgentResult<'_>],
    style: RenderStyle,
) -> io::Result<()> {
    let left_width = name_column_width(
        results
            .iter()
            .flat_map(|(_, _, report)| report.skills.iter().map(|skill| skill.name.as_str())),
        style,
    );
    for confirmed in [true, false] {
        let reports: Vec<_> = results
            .iter()
            .filter(|(_, label, _)| (*label == CONFIRMED_LABEL) == confirmed)
            .collect();
        if reports
            .iter()
            .all(|(_, _, report)| report.skills.is_empty())
        {
            continue;
        }
        let title = if confirmed {
            "本体で確認"
        } else {
            "ローカルで検出（有効状態未確認）"
        };
        writeln!(out, "\n{}", style.paint(title, "1"))?;
        let mut rendered_scope = false;
        for (scope, heading, code) in SCOPES {
            let mut skills: BTreeMap<&str, BTreeMap<&str, BTreeSet<&Path>>> = BTreeMap::new();
            for (name, _, report) in &reports {
                for skill in report.skills.iter().filter(|skill| skill.scope == scope) {
                    skills
                        .entry(&skill.name)
                        .or_default()
                        .entry(name)
                        .or_default()
                        .insert(skill.path.as_path());
                }
            }
            if skills.is_empty() {
                continue;
            }
            if rendered_scope {
                writeln!(out)?;
            }
            rendered_scope = true;
            writeln!(
                out,
                "\n{} ({}スキル)",
                style.paint(heading, code),
                skills.len()
            )?;
            let rows: Vec<_> = skills
                .into_iter()
                .map(|(skill, agents)| {
                    let mut rendered = StyledText::default();
                    for (index, (name, paths)) in agents.into_iter().enumerate() {
                        if index > 0 {
                            rendered.push(" · ", "");
                        }
                        rendered.push(safe(name), agent_color(name));
                        if paths.len() > 1 {
                            rendered.push(format!("（{}か所）", paths.len()), "1;39");
                        }
                    }
                    (safe(skill), rendered)
                })
                .collect();
            let heading = if confirmed {
                "利用できるエージェント"
            } else {
                "候補のエージェント"
            };
            render_styled_table(out, &rows, heading, left_width, "", style)?;
        }
    }
    for (name, label, report) in results {
        if report.skills.is_empty() {
            writeln!(
                out,
                "\n{} [{label}]: {}",
                style.paint(name, agent_color(name)),
                empty_message(label, report)
            )?;
        }
        render_warnings(out, name, report, style)?;
    }
    Ok(())
}

fn empty_message(label: &str, report: &Report) -> &'static str {
    if !report.warnings.is_empty() {
        "一覧を確定できませんでした"
    } else if label == CONFIRMED_LABEL {
        "有効なスキルはありません"
    } else {
        "表示対象のローカルスキルはありません"
    }
}

#[allow(clippy::too_many_arguments)]
fn render_detail(
    out: &mut impl Write,
    name: &str,
    label: &str,
    report: &Report,
    roots: &[Root],
    project: &Path,
    home: &Path,
    style: RenderStyle,
) -> io::Result<()> {
    writeln!(
        out,
        "\n{}  [{}]",
        style.paint(name, &format!("1;{}", agent_color(name))),
        label
    )?;
    if report.skills.is_empty() {
        writeln!(out, "{}", empty_message(label, report))?;
    }
    let left_width =
        name_column_width(report.skills.iter().map(|skill| skill.name.as_str()), style);
    let mut rendered_scope = false;
    for (scope, heading, code) in SCOPES {
        let mut entries: Vec<_> = report
            .skills
            .iter()
            .filter(|skill| skill.scope == scope)
            .collect();
        entries.sort_by(|a, b| (&a.name, &a.path).cmp(&(&b.name, &b.path)));
        if entries.is_empty() {
            continue;
        }
        if rendered_scope {
            writeln!(out)?;
        }
        rendered_scope = true;
        writeln!(
            out,
            "\n{} ({}定義)",
            style.paint(heading, code),
            entries.len()
        )?;
        let mut previous = None;
        let rows: Vec<_> = entries
            .into_iter()
            .map(|skill| {
                let row_name = if previous == Some(&skill.name) {
                    String::new()
                } else {
                    safe(&skill.name)
                };
                previous = Some(&skill.name);
                let mut source = StyledText::default();
                if let Some((label, code)) = source_badge(&skill.path, scope, name, roots) {
                    source.push(format!("[{label}]"), code);
                    source.push(" ", "");
                }
                source.push(display_path(&skill.path, scope, project, home), "");
                if !skill.note.is_empty() {
                    source.push(format!("  ({})", safe(&skill.note)), "2");
                }
                (row_name, source)
            })
            .collect();
        render_styled_table(out, &rows, "読み込み元", left_width, "1", style)?;
    }
    render_warnings(out, name, report, style)
}

fn render_warnings(
    out: &mut impl Write,
    name: &str,
    report: &Report,
    style: RenderStyle,
) -> io::Result<()> {
    for warning in &report.warnings {
        writeln!(
            out,
            "{}{}{}",
            style.paint("注意（", "33"),
            style.paint(name, agent_color(name)),
            style.paint(&format!("）: {}", safe(warning)), "33")
        )?;
    }
    Ok(())
}

fn name_column_width<'a>(names: impl Iterator<Item = &'a str>, style: RenderStyle) -> usize {
    names
        .map(|name| safe(name).width())
        .max()
        .unwrap_or(0)
        .max("スキル名".width())
        .min(40)
        .min((style.width - 2) / 2)
}

fn render_styled_table(
    out: &mut impl Write,
    rows: &[(String, StyledText)],
    right_heading: &str,
    left_width: usize,
    left_code: &str,
    style: RenderStyle,
) -> io::Result<()> {
    let right_width = style.width - left_width - 2;
    let heading = StyledText(vec![(right_heading.to_owned(), "1")]);
    for (row, (left, right)) in std::iter::once(("スキル名", &heading))
        .chain(rows.iter().map(|(left, right)| (left.as_str(), right)))
        .enumerate()
    {
        let left = wrap_cells(left, left_width);
        let right = right.wrap(right_width, style);
        for index in 0..left.len().max(right.len()) {
            let left = left.get(index).map(String::as_str).unwrap_or("");
            let right = right.get(index).map(String::as_str).unwrap_or("");
            writeln!(
                out,
                "{}{}  {}",
                style.paint(left, if row == 0 { "1" } else { left_code }),
                " ".repeat(left_width.saturating_sub(left.width())),
                right
            )?;
        }
        if row == 0 {
            writeln!(out)?;
        }
    }
    Ok(())
}

fn wrap_cells(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut characters = text.char_indices().peekable();
    while let Some((start, _)) = characters.next() {
        // Keep zero-width modifiers with their base; selectors can change its width.
        while characters.peek().is_some_and(|(_, c)| c.width() == Some(0)) {
            characters.next();
        }
        let end = characters.peek().map_or(text.len(), |(index, _)| *index);
        let previous_end = line.len();
        line.push_str(&text[start..end]);
        // Use the same string-width calculation as column sizing and padding.
        if line.width() > width && previous_end > 0 {
            let next = line.split_off(previous_end);
            lines.push(std::mem::replace(&mut line, next));
        }
    }
    lines.push(line);
    lines
}

#[cfg(test)]
mod json_tests {
    use super::*;

    #[test]
    fn confirmed_json_preserves_raw_names_paths_notes_and_scopes() {
        let report = Report {
            skills: vec![
                Skill {
                    name: "raw\nname".into(),
                    path: "virtual://skill".into(),
                    scope: Scope::Unspecified,
                    note: "use \"quotes\"\nnow".into(),
                },
                Skill {
                    name: "raw\nname".into(),
                    path: "relative/./skill".into(),
                    scope: Scope::Project,
                    note: String::new(),
                },
                Skill {
                    name: "raw\nname".into(),
                    path: "relative/second".into(),
                    scope: Scope::Project,
                    note: "condition".into(),
                },
            ],
            warnings: vec![],
        };
        let results = vec![("Codex", CONFIRMED_LABEL, report)];
        let bytes = json_document(Path::new("/workspace"), &results, None).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(!bytes.contains(&0x1b));
        assert_eq!(value["agents"][0]["evidence"], "confirmed");
        assert_eq!(
            value["agents"][0]["skills"],
            serde_json::json!([
                {"name":"raw\nname", "scope":"project", "sources":[
                    {"path":"relative/./skill", "note":null},
                    {"path":"relative/second", "note":"condition"}
                ]},
                {"name":"raw\nname", "scope":"unspecified", "sources":[
                    {"path":"virtual://skill", "note":"use \"quotes\"\nnow"}
                ]}
            ])
        );
        let reversed = Report {
            skills: results
                .into_iter()
                .next()
                .unwrap()
                .2
                .skills
                .into_iter()
                .rev()
                .collect(),
            warnings: vec![],
        };
        assert_eq!(
            json_document(
                Path::new("/workspace"),
                &[("Codex", CONFIRMED_LABEL, reversed)],
                None
            )
            .unwrap(),
            bytes
        );
    }

    #[cfg(unix)]
    #[test]
    fn json_rejects_non_utf8_paths_losslessly() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        let invalid = PathBuf::from(OsString::from_vec(vec![b's', 0xff]));
        assert_eq!(
            json_document(&invalid, &[], None).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        let report = Report {
            skills: vec![Skill {
                name: "valid".into(),
                path: invalid,
                scope: Scope::Global,
                note: String::new(),
            }],
            warnings: vec![],
        };
        assert_eq!(
            json_document(
                Path::new("/workspace"),
                &[("Codex", CONFIRMED_LABEL, report)],
                None
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::InvalidData
        );
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;

    fn skill(name: &str, scope: Scope, path: &str) -> Skill {
        Skill {
            name: name.into(),
            scope,
            path: path.into(),
            note: String::new(),
        }
    }

    fn source_roots() -> Vec<Root> {
        [
            ("/home/.agents/skills", Scope::Global),
            ("/home/.codex/skills", Scope::Global),
            ("/project/pkg/.agents/skills", Scope::Project),
            ("/project/.claude/skills", Scope::Project),
            ("/home/xdg/agents/skills", Scope::Global),
            ("/custom/codex/skills", Scope::Global),
        ]
        .into_iter()
        .map(|(path, scope)| Root {
            path: path.into(),
            scope,
            depth: 1,
        })
        .collect()
    }

    fn strip_ansi(text: &str) -> String {
        let mut escape = false;
        text.chars()
            .filter(|&character| {
                if character == '\x1b' {
                    escape = true;
                    false
                } else if escape {
                    if character == 'm' {
                        escape = false;
                    }
                    false
                } else {
                    true
                }
            })
            .collect()
    }

    #[test]
    fn agent_hues_and_count_emphasis_survive_filtering_and_wrapping() {
        let results = vec![
            (
                "Codex",
                "本体で確認",
                Report {
                    skills: vec![
                        skill("日本語の長い名前", Scope::Project, "a"),
                        skill("日本語の長い名前", Scope::Project, "b"),
                    ],
                    warnings: vec![],
                },
            ),
            (
                "Pi",
                "本体で確認",
                Report {
                    skills: vec![skill("日本語の長い名前", Scope::Project, "c")],
                    warnings: vec![],
                },
            ),
        ];
        let render = |color, results: &[AgentResult<'_>]| {
            let mut output = Vec::new();
            render_overview(&mut output, results, RenderStyle { width: 32, color }).unwrap();
            String::from_utf8(output).unwrap()
        };
        let colored = render(true, &results);
        let plain = render(false, &results);
        assert_eq!(strip_ansi(&colored), plain);
        assert!(plain.lines().all(|line| line.width() <= 32));
        assert!(colored.contains("\x1b[36mCodex\x1b[0m\x1b[1;39m"));
        assert!(colored.contains("\x1b[35mPi\x1b[0m"));
        assert!(render(true, &results[1..]).contains("\x1b[35mPi\x1b[0m"));
        let mut detail = Vec::new();
        render_detail(
            &mut detail,
            "Pi",
            "本体で確認",
            &results[1].2,
            &source_roots(),
            Path::new("/project"),
            Path::new("/home"),
            RenderStyle {
                width: 32,
                color: true,
            },
        )
        .unwrap();
        assert!(
            String::from_utf8(detail)
                .unwrap()
                .contains("\x1b[1;35mPi\x1b[0m")
        );
        let mut spans = StyledText::default();
        spans.push("日本語Codex", agent_color("Codex"));
        spans.push(" · ", "");
        spans.push("Pi", agent_color("Pi"));
        let colored = spans.wrap(
            4,
            RenderStyle {
                width: 24,
                color: true,
            },
        );
        assert!(colored.len() > 2);
        assert!(colored.iter().all(|line| strip_ansi(line).width() <= 4));
        assert_eq!(strip_ansi(&colored.concat()), "日本語Codex · Pi");
        assert!(
            colored
                .iter()
                .take(3)
                .all(|line| line.starts_with("\x1b[36m"))
        );
    }

    #[test]
    fn source_badges_describe_layouts_without_claiming_agent_ownership() {
        let project = Path::new("/project");
        let home = Path::new("/home");
        let roots = source_roots();
        for (path, scope, expected) in [
            (
                "/home/.agents/skills/a/SKILL.md",
                Scope::Global,
                Some(("共通", "")),
            ),
            (
                "/home/.codex/skills/a/SKILL.md",
                Scope::Global,
                Some(("Codex", "36")),
            ),
            (
                "/project/pkg/.agents/skills/a/SKILL.md",
                Scope::Project,
                Some(("共通", "")),
            ),
            (
                "/project/.claude/skills/a/SKILL.md",
                Scope::Project,
                Some(("Claude Code", agent_color("Claude Code"))),
            ),
            (
                "/home/xdg/agents/skills/a/SKILL.md",
                Scope::Global,
                Some(("共通", "")),
            ),
            (
                "/custom/codex/skills/a/SKILL.md",
                Scope::Global,
                Some(("設定先", "36")),
            ),
            ("/home/custom/a/SKILL.md", Scope::Global, None),
            (
                "/home/.codex/plugins/plugin/.agents/skills/a/SKILL.md",
                Scope::Global,
                None,
            ),
            ("virtual://.codex/skills/a", Scope::Unspecified, None),
            ("/home/.codex/skills/a/SKILL.md", Scope::Unspecified, None),
            ("/other/.codex/skills/a", Scope::Global, None),
        ] {
            assert_eq!(
                source_badge(Path::new(path), scope, "Codex", &roots),
                expected,
                "{path}"
            );
        }
        let overlapping = vec![
            Root {
                path: "/custom".into(),
                scope: Scope::Global,
                depth: 64,
            },
            Root {
                path: "/custom/.claude/skills".into(),
                scope: Scope::Global,
                depth: 1,
            },
        ];
        assert_eq!(
            source_badge(
                Path::new("/custom/.claude/skills/a/SKILL.md"),
                Scope::Global,
                "Cursor",
                &overlapping
            ),
            Some(("Claude Code", agent_color("Claude Code")))
        );
        assert_eq!(
            source_badge(
                Path::new("/home/.codex/skills/a/SKILL.md"),
                Scope::Project,
                "Codex",
                &roots
            ),
            None
        );
        let report = Report {
            skills: vec![
                skill(
                    "review",
                    Scope::Global,
                    "/home/.agents/skills/review/SKILL.md",
                ),
                skill(
                    "review",
                    Scope::Global,
                    "/home/.codex/skills/review/SKILL.md",
                ),
                skill("custom", Scope::Global, "/home/custom/SKILL.md"),
            ],
            warnings: vec![],
        };
        let render = |color| {
            let mut output = Vec::new();
            render_detail(
                &mut output,
                "Codex",
                "本体で確認",
                &report,
                &source_roots(),
                project,
                home,
                RenderStyle { width: 100, color },
            )
            .unwrap();
            String::from_utf8(output).unwrap()
        };
        let colored = render(true);
        assert_eq!(strip_ansi(&colored), render(false));
        assert!(colored.contains("[共通] ~"));
        assert!(colored.contains("\x1b[36m[Codex]\x1b[0m ~/.codex/skills/review/SKILL.md"));
        assert!(!colored.contains("\x1b[2m"));
        assert!(colored.contains("~/custom/SKILL.md"));
    }

    #[test]
    fn unicode_modifiers_in_long_names_stay_within_the_column_budget() {
        let name = format!("{}{}", "☀️".repeat(30), "e\u{301}".repeat(30));
        let wrapped = wrap_cells(&name, 15);
        assert!(wrapped.len() > 1);
        assert!(wrapped.iter().all(|line| line.width() <= 15));
        assert_eq!(wrapped.concat(), name);
        assert!(
            wrapped
                .iter()
                .all(|line| !line.starts_with(['\u{fe0f}', '\u{301}']))
        );

        let mut output = Vec::new();
        render_styled_table(
            &mut output,
            &[(name, StyledText(vec![("Codex".into(), "")]))],
            "エージェント",
            15,
            "",
            RenderStyle {
                width: 32,
                color: false,
            },
        )
        .unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(text.lines().all(|line| line.width() <= 32));
    }

    #[test]
    fn overview_uses_one_name_column_width_across_sections() {
        let results = vec![
            (
                "Codex",
                "本体で確認",
                Report {
                    skills: vec![skill("a", Scope::Project, "a")],
                    warnings: vec![],
                },
            ),
            (
                "Cursor",
                "ローカル配置・有効状態未確認",
                Report {
                    skills: vec![skill("長い名前のスキル", Scope::Global, "b")],
                    warnings: vec![],
                },
            ),
        ];
        let mut output = Vec::new();
        render_overview(
            &mut output,
            &results,
            RenderStyle {
                width: 100,
                color: false,
            },
        )
        .unwrap();
        let text = String::from_utf8(output).unwrap();
        let columns: Vec<_> = [
            "利用できるエージェント",
            "候補のエージェント",
            "Codex",
            "Cursor",
        ]
        .iter()
        .map(|needle| {
            let line = text.lines().find(|line| line.contains(needle)).unwrap();
            line.split_once(needle).unwrap().0.width()
        })
        .collect();
        assert!(columns.iter().all(|column| *column == columns[0]));
        assert!(text.contains("プロジェクト (1スキル)"));
        assert!(text.contains("グローバル (1スキル)"));
    }

    #[test]
    fn overview_cross_tab_preserves_duplicates_and_separates_unconfirmed() {
        let results = vec![
            (
                "Codex",
                "本体で確認",
                Report {
                    skills: vec![
                        skill("review", Scope::Project, "a"),
                        skill("review", Scope::Project, "b"),
                        skill("review", Scope::Project, "a"),
                    ],
                    warnings: vec![],
                },
            ),
            (
                "Pi",
                "本体で確認",
                Report {
                    skills: vec![skill("review", Scope::Project, "c")],
                    warnings: vec![],
                },
            ),
            (
                "Cursor",
                "ローカル配置・有効状態未確認",
                Report {
                    skills: vec![skill("review", Scope::Project, "d")],
                    warnings: vec!["読めない配置あり".into()],
                },
            ),
        ];
        let mut output = Vec::new();
        render_overview(
            &mut output,
            &results,
            RenderStyle {
                width: 100,
                color: false,
            },
        )
        .unwrap();
        let text = String::from_utf8(output).unwrap();
        let (confirmed, local) = text.split_once("ローカルで検出（有効状態未確認）").unwrap();
        assert!(confirmed.contains("review") && confirmed.contains("Codex（2か所） · Pi"));
        assert!(!confirmed.contains("Cursor"));
        assert!(confirmed.contains("利用できるエージェント"));
        assert!(local.contains("候補のエージェント"));
        assert!(!local.contains("利用できるエージェント"));
        assert!(local.contains("review") && local.contains("Cursor"));
        assert!(local.contains("注意（Cursor）: 読めない配置あり"));
        assert_eq!(text.matches("プロジェクト (1スキル)").count(), 2);
    }

    #[test]
    fn narrow_table_aligns_wide_names_and_keeps_all_text() {
        let name = "日本語の非常に長いスキル名abcdefghijklmnop";
        let agents = "Codex（2か所） · GitHub Copilot CLI · Pi";
        let mut output = Vec::new();
        render_styled_table(
            &mut output,
            &[(name.into(), StyledText(vec![(agents.into(), "")]))],
            "利用できるエージェント",
            15,
            "",
            RenderStyle {
                width: 32,
                color: false,
            },
        )
        .unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(text.lines().all(|line| line.width() <= 32));
        let (_, data) = text.split_once("\n\n").unwrap();
        let data: Vec<_> = data.lines().collect();
        let left: String = data
            .iter()
            .map(|line| line.chars().take_while(|c| *c != ' ').collect::<String>())
            .collect();
        assert_eq!(left, name);
        let right: String = data
            .iter()
            .map(|line| {
                let mut cells = 0;
                line.chars()
                    .skip_while(|c| {
                        cells += c.width().unwrap_or(0);
                        cells <= 17
                    })
                    .collect::<String>()
            })
            .collect();
        assert_eq!(right, agents);
    }

    #[test]
    fn detail_keeps_every_definition_with_repeated_name_shown_once() {
        let report = Report {
            skills: vec![
                skill(
                    "review",
                    Scope::Global,
                    "/home/.codex/skills/review/SKILL.md",
                ),
                skill(
                    "review",
                    Scope::Global,
                    "/home/.agents/skills/review/SKILL.md",
                ),
            ],
            warnings: vec![],
        };
        let mut output = Vec::new();
        render_detail(
            &mut output,
            "Codex",
            "本体で確認",
            &report,
            &source_roots(),
            Path::new("/project"),
            Path::new("/home"),
            RenderStyle {
                width: 100,
                color: false,
            },
        )
        .unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("Codex  [本体で確認]"));
        assert!(text.contains("グローバル (2定義)"));
        assert!(text.contains("~/.agents/skills/review/SKILL.md"));
        assert!(text.contains("~/.codex/skills/review/SKILL.md"));
        assert_eq!(
            text.lines()
                .filter(|line| line.starts_with("review "))
                .count(),
            1
        );
    }

    #[test]
    fn overview_counts_distinct_paths_without_resolving_filesystem_aliases() {
        let results = vec![(
            "Codex",
            "本体で確認",
            Report {
                skills: vec![
                    skill("review", Scope::Project, "/a/b"),
                    skill("review", Scope::Project, "/a/b"),
                    skill("review", Scope::Project, "/a/./b"),
                    skill("review", Scope::Project, "/alias/b"),
                    skill("single", Scope::Project, "/c"),
                    skill("single", Scope::Project, "/c"),
                    skill("review", Scope::Global, "/d"),
                ],
                warnings: vec![],
            },
        )];
        let mut output = Vec::new();
        render_overview(
            &mut output,
            &results,
            RenderStyle {
                width: 100,
                color: false,
            },
        )
        .unwrap();
        let text = String::from_utf8(output).unwrap();
        assert_eq!(text.matches("Codex（2か所）").count(), 1);
        assert_eq!(text.matches("か所").count(), 1);
        assert!(text.contains("Codex\n\n\nグローバル"));
        assert!(!text.ends_with("\n\n"));
    }

    #[test]
    fn detail_styles_wrapped_names_and_paths_without_changing_text_or_scope_spacing() {
        let long_name = "日本語の非常に長いスキル名abcdefghijk";
        let mut first = skill(
            long_name,
            Scope::Project,
            "/project/長い読み込み元/日本語の説明/first/SKILL.md",
        );
        first.note = "日本語とEnglishの長い呼び出し条件を保持する".into();
        let report = Report {
            skills: vec![
                first,
                skill(
                    long_name,
                    Scope::Project,
                    "/project/別の読み込み元/second/SKILL.md",
                ),
                skill("global", Scope::Global, "/home/global/SKILL.md"),
            ],
            warnings: vec![],
        };
        let render = |color| {
            let mut output = Vec::new();
            render_detail(
                &mut output,
                "Codex",
                "本体で確認",
                &report,
                &source_roots(),
                Path::new("/project"),
                Path::new("/home"),
                RenderStyle { width: 52, color },
            )
            .unwrap();
            String::from_utf8(output).unwrap()
        };
        let plain = render(false);
        let colored = render(true);
        assert!(colored.contains("\x1b[1m日本語"));
        assert!(colored.contains("\x1b[2m"));
        let mut stripping = false;
        let stripped: String = colored
            .chars()
            .filter(|&character| {
                if character == '\x1b' {
                    stripping = true;
                    false
                } else if stripping {
                    if character == 'm' {
                        stripping = false;
                    }
                    false
                } else {
                    true
                }
            })
            .collect();
        assert_eq!(stripped, plain);
        assert!(!plain.contains('\x1b'));
        assert!(plain.lines().all(|line| line.width() <= 52));
        assert!(plain.contains("\n\n\nグローバル"));
        assert!(!plain.ends_with("\n\n"));
        let project = plain
            .split_once("プロジェクト (2定義)\n")
            .unwrap()
            .1
            .split_once("\n\n\nグローバル")
            .unwrap()
            .0;
        let (_, data) = project.split_once("\n\n").unwrap();
        let left_width = name_column_width(
            report.skills.iter().map(|skill| skill.name.as_str()),
            RenderStyle {
                width: 52,
                color: false,
            },
        );
        let mut left = String::new();
        let mut right = String::new();
        for line in data.lines() {
            let mut cells = 0;
            for character in line.chars() {
                cells += character.width().unwrap_or(0);
                if cells <= left_width {
                    if character != ' ' {
                        left.push(character);
                    }
                } else if cells > left_width + 2 {
                    right.push(character);
                }
            }
        }
        assert_eq!(left, long_name);
        assert!(right.contains("長い読み込み元/日本語の説明/first/SKILL.md"));
        assert!(right.contains("日本語とEnglishの長い呼び出し条件を保持する"));
        assert!(right.contains("別の読み込み元/second/SKILL.md"));
    }
}
