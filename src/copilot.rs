use std::path::Path;

use serde_json::Value;

use crate::{Report, Scope, Skill, rpc::Server};

/// List discovered, enabled skills without opening a Copilot conversation.
pub fn list(cwd: &Path) -> Result<Report, String> {
    let result = Server::spawn("copilot", &["skill", "list", "--json"], cwd)?.output()?;
    parse_result(&result)
}

fn parse_result(result: &Value) -> Result<Report, String> {
    let rows = result
        .as_array()
        .ok_or("Copilot の一覧が JSON 配列ではありません")?;
    let mut report = Report::default();
    for row in rows {
        match row.get("enabled").and_then(Value::as_bool) {
            Some(false) => continue,
            Some(true) => {}
            None => {
                report
                    .warnings
                    .push("Copilot のスキルの有効状態が不明です".to_owned());
                continue;
            }
        }
        let Some(name) = row
            .get("name")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        else {
            report
                .warnings
                .push("Copilot のスキル名が不明です".to_owned());
            continue;
        };
        // Source labels verified with the official @github/copilot 1.0.114 distribution.
        // Custom/plugin/remote sources keep their source label without guessing their scope.
        let mut scope_note = String::new();
        let scope = match row.get("source").and_then(Value::as_str) {
            Some("project" | "inherited") => Scope::Project,
            Some("personal-copilot" | "personal-agents" | "builtin") => Scope::Global,
            _ => {
                scope_note = format!(
                    "範囲未特定。source={}",
                    row.get("source").unwrap_or(&Value::Null)
                );
                Scope::Unspecified
            }
        };
        let Some(path) = row
            .get("path")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        else {
            report
                .warnings
                .push(format!("Copilot のスキル {name} の path が不明です"));
            continue;
        };
        // Copilot returns the skill directory. Keep the returned value rather than
        // appending SKILL.md or assuming an unverified filesystem location.
        let mut note = if Path::new(path).is_absolute() {
            String::new()
        } else {
            "返却パスは仮想または相対パスで、実ファイルの位置は未確認".to_owned()
        };
        if !scope_note.is_empty() {
            if !note.is_empty() {
                note.push('。');
            }
            note.push_str(&scope_note);
        }
        report.skills.push(Skill {
            name: name.to_owned(),
            path: path.into(),
            scope,
            note,
        });
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn classifies_verified_sources_and_preserves_directory_paths() {
        let report = parse_result(&json!([
            {"name":"project","source":"project","path":"/repo/skills/project","enabled":true},
            {"name":"inherited","source":"inherited","path":"/parent/skills/inherited","enabled":true},
            {"name":"personal","source":"personal-copilot","path":"/home/.copilot/skills/personal","enabled":true},
            {"name":"shared","source":"personal-agents","path":"/home/.agents/skills/shared","enabled":true},
            {"name":"builtin","source":"builtin","path":"/cache/builtin","enabled":true},
            {"name":"disabled","source":"project","path":"/disabled","enabled":false}
        ])).unwrap();
        assert_eq!(report.skills.len(), 5);
        assert!(
            report.skills[..2]
                .iter()
                .all(|s| matches!(s.scope, Scope::Project))
        );
        assert!(
            report.skills[2..]
                .iter()
                .all(|s| matches!(s.scope, Scope::Global))
        );
        assert_eq!(report.skills[0].path, Path::new("/repo/skills/project"));
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn warns_on_missing_or_ambiguous_metadata() {
        let report = parse_result(&json!([
            {"name":"custom","source":"custom","path":"/custom","enabled":true},
            {"name":"plugin:review","source":"plugin","path":"/plugin","enabled":true},
            {"name":"unknown","source":"future","path":"/future","enabled":true},
            {"name":"missing-path","source":"project","enabled":true},
            {"name":"missing-enabled","source":"project","path":"/missing"}
        ]))
        .unwrap();
        assert_eq!(report.skills.len(), 3);
        assert!(
            report
                .skills
                .iter()
                .all(|s| matches!(s.scope, Scope::Unspecified))
        );
        assert!(report.skills[0].note.contains("custom"));
        assert_eq!(report.warnings.len(), 2);
    }

    #[test]
    fn rejects_invalid_envelope() {
        assert!(parse_result(&json!({"skills":[]})).is_err());
    }
}
