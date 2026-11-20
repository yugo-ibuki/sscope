use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::agents::Agent;
use crate::scan::{self, Collision, Root};
use crate::{Report, Scope};

pub fn local(agent: &Agent, cwd: &Path, repo: Option<&Path>, home: &Path) -> Report {
    let project = repo
        .or_else(|| {
            cwd.ancestors()
                .take_while(|dir| *dir != home)
                .find(|dir| match agent.name {
                    "Claude Code" => dir.join(".claude").is_dir(),
                    "Gemini CLI" => dir.join(".gemini").is_dir(),
                    "Factory Droid" => dir.join(".factory").is_dir(),
                    _ => false,
                })
        })
        .unwrap_or(cwd);
    if agent.name == "Claude Code" {
        return claude(&agent.roots, project, home);
    }
    let mut report = scan::scan(&agent.roots, agent.collision);
    match agent.name {
        "Codex" => {
            let base = env::var_os("CODEX_HOME")
                .filter(|p| !p.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".codex"));
            let file = base.join("config.toml");
            if let Some(text) = read_config(&file, &mut report) {
                match toml::from_str::<toml::Value>(&text) {
                    Ok(settings) => {
                        if let Some(rules) = settings
                            .get("skills")
                            .and_then(|s| s.get("config"))
                            .and_then(toml::Value::as_array)
                        {
                            report.skills.retain(|skill| {
                                rules
                                    .iter()
                                    .rev()
                                    .find_map(|rule| {
                                        let matches =
                                            rule.get("name").and_then(toml::Value::as_str)
                                                == Some(skill.name.as_str())
                                                || rule
                                                    .get("path")
                                                    .and_then(toml::Value::as_str)
                                                    .is_some_and(|p| Path::new(p) == skill.path);
                                        if matches {
                                            rule.get("enabled").and_then(toml::Value::as_bool)
                                        } else {
                                            None
                                        }
                                    })
                                    .unwrap_or(true)
                            });
                        }
                    }
                    Err(_) => report
                        .warnings
                        .push(format!("TOML 設定を解析できません: {}", file.display())),
                }
            }
        }
        "Gemini CLI" => {
            let mut enabled = true;
            let mut disabled = HashSet::new();
            for config in config_pair(home, project, ".gemini/settings.json") {
                let settings = json_file(&config, &mut report);
                if let Some(value) = settings.pointer("/skills/enabled").and_then(Value::as_bool) {
                    enabled = value;
                }
                collect_names(settings.pointer("/skills/disabled"), &mut disabled);
            }
            report
                .skills
                .retain(|s| enabled && !disabled.contains(&s.name));
        }
        "Factory Droid" => {
            let mut disabled = HashSet::new();
            for config in config_pair(home, project, ".factory/settings.json") {
                let settings = json_file(&config, &mut report);
                collect_names(settings.get("disabledSkills"), &mut disabled);
            }
            report.skills.retain(|s| !disabled.contains(&s.name));
        }
        _ => {}
    }
    report
}

fn config_pair(home: &Path, project: &Path, relative: &str) -> Vec<PathBuf> {
    let mut paths = vec![home.join(relative)];
    if home != project {
        paths.push(project.join(relative));
    }
    paths
}

fn read_config(path: &Path, report: &mut Report) -> Option<String> {
    let result = (|| {
        if !fs::metadata(path)?.is_file() {
            return Err(io::Error::other("通常のファイルではありません"));
        }
        let mut text = String::new();
        const LIMIT: u64 = 16 * 1024 * 1024;
        fs::File::open(path)?
            .take(LIMIT + 1)
            .read_to_string(&mut text)?;
        if text.len() as u64 > LIMIT {
            return Err(io::Error::other("設定ファイルが 16 MiB を超えています"));
        }
        Ok(text)
    })();
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            if error.kind() != io::ErrorKind::NotFound {
                report.warnings.push(format!(
                    "設定を読み込めません ({}): {error}",
                    path.display()
                ));
            }
            None
        }
    }
}

fn json_file(path: &Path, report: &mut Report) -> Value {
    let Some(text) = read_config(path, report) else {
        return Value::Null;
    };
    serde_json::from_str(&text).unwrap_or_else(|error| {
        report.warnings.push(format!(
            "JSON 設定を解析できません ({}): {error}",
            path.display()
        ));
        Value::Null
    })
}

fn collect_names(value: Option<&Value>, names: &mut HashSet<String>) {
    if let Some(values) = value.and_then(Value::as_array) {
        names.extend(values.iter().filter_map(Value::as_str).map(str::to_owned));
    }
}

fn claude(roots: &[Root], project: &Path, home: &Path) -> Report {
    let config = env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".claude"));
    let mut ordered: Vec<_> = roots.iter().collect();
    // Claude reserves the unqualified command for the personal or repo-root copy.
    ordered.sort_by_key(|r| (r.scope == Scope::Project, r.path.components().count()));
    let mut report = Report::default();
    let mut used_names = HashSet::new();
    let mut files = HashSet::new();
    for root in ordered {
        let mut found = scan::scan_claude(std::slice::from_ref(root), Collision::Keep);
        report.warnings.append(&mut found.warnings);
        for mut skill in found.skills {
            let dir = root.path.parent().and_then(Path::parent);
            if matches!(skill.path.parent().and_then(Path::file_name).and_then(|p|p.to_str()),
                Some(name) if name.eq_ignore_ascii_case("synced"))
            {
                continue;
            }
            let target = skill
                .path
                .canonicalize()
                .unwrap_or_else(|_| skill.path.clone());
            if !files.insert(target) {
                continue;
            }
            if !used_names.insert(skill.name.clone()) {
                let Some(prefix) = dir
                    .and_then(|d| d.strip_prefix(project).ok())
                    .filter(|p| !p.as_os_str().is_empty())
                else {
                    continue;
                };
                if skill.scope != Scope::Project {
                    continue;
                }
                skill.name = format!("{}:{}", prefix.display(), skill.name);
            }
            report.skills.push(skill);
        }
    }

    let mut config_files = vec![config.join("settings.json")];
    if project != home {
        config_files.push(project.join(".claude/settings.json"));
        config_files.push(project.join(".claude/settings.local.json"));
    }
    let mut overrides = HashMap::new();
    let mut plugins = HashMap::new();
    for file in config_files {
        let settings = json_file(&file, &mut report);
        if let Some(entries) = settings.get("skillOverrides").and_then(Value::as_object) {
            for (key, value) in entries {
                if let Some(mode) = value.as_str() {
                    overrides.insert(key.clone(), mode.to_owned());
                }
            }
        }
        if let Some(entries) = settings.get("enabledPlugins").and_then(Value::as_object) {
            for (key, value) in entries {
                if let Some(enabled) = value.as_bool() {
                    plugins.insert(key.clone(), enabled);
                }
            }
        }
    }
    if plugins.values().any(|enabled| *enabled) {
        let installed = json_file(&config.join("plugins/installed_plugins.json"), &mut report);
        if let Some(entries) = installed.get("plugins").and_then(Value::as_object) {
            for (key, records) in entries {
                if plugins.get(key) != Some(&true) {
                    continue;
                }
                let Some(records) = records.as_array() else {
                    continue;
                };
                let selected = records
                    .iter()
                    .filter_map(|record| {
                        let (rank, scope) = match record.get("scope")?.as_str()? {
                            "local" | "project"
                                if Path::new(record.get("projectPath")?.as_str()?) == project =>
                            {
                                (0, Scope::Project)
                            }
                            "user" | "managed" => (1, Scope::Global),
                            _ => return None,
                        };
                        Some((rank, scope, Path::new(record.get("installPath")?.as_str()?)))
                    })
                    .min_by_key(|(rank, _, _)| *rank);
                if let Some((_, scope, install)) = selected {
                    let root = Root {
                        path: install.join("skills"),
                        scope,
                        depth: 1,
                    };
                    let mut found = scan::scan_claude(&[root], Collision::First);
                    let prefix = key.split('@').next().unwrap_or(key);
                    for s in &mut found.skills {
                        s.name = format!("{prefix}:{}", s.name);
                    }
                    report.skills.append(&mut found.skills);
                    report.warnings.append(&mut found.warnings);
                }
            }
        }
    }
    report.skills.retain_mut(
        |skill| match overrides.get(&skill.name).map(String::as_str) {
            Some("off") => false,
            Some("user-invocable-only") => {
                skill.note = "手動呼び出しのみ".into();
                true
            }
            _ => true,
        },
    );
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    #[test]
    fn claude_resolves_personal_and_nested_names_and_disabled_settings() {
        let temp = tempdir().unwrap();
        let home = temp.path().join("user");
        let project = temp.path().join("repo");
        let child = project.join("pkg");
        let mut roots = vec![];
        for (dir, scope) in [
            (&home, Scope::Global),
            (&project, Scope::Project),
            (&child, Scope::Project),
        ] {
            let skills = dir.join(".claude/skills");
            for name in ["review", "ignored"] {
                fs::create_dir_all(skills.join(name)).unwrap();
                fs::write(skills.join(name).join("SKILL.md"), "Perform a review").unwrap();
            }
            roots.push(Root {
                path: skills,
                scope,
                depth: 1,
            });
        }
        fs::write(
            project.join(".claude/settings.json"),
            r#"{"skillOverrides":{"ignored":"off","pkg:ignored":"off"}}"#,
        )
        .unwrap();
        let report = claude(&roots, &project, &home);
        assert!(report.warnings.is_empty());
        assert_eq!(report.skills.len(), 2);
        assert!(
            report
                .skills
                .iter()
                .any(|s| s.name == "pkg:review" && s.scope == Scope::Project)
        );
        assert!(
            report
                .skills
                .iter()
                .any(|s| s.name == "review" && s.scope == Scope::Global)
        );
    }
}
