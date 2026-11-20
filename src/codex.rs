use std::path::Path;

use serde_json::{Value, json};

use crate::{Report, Scope, Skill, rpc::Server};

/// Ask Codex itself which skills are enabled for this directory.
pub fn list(cwd: &Path) -> Result<Report, String> {
    let cwd = cwd
        .canonicalize()
        .map_err(|e| format!("Codex の対象ディレクトリを確認できません: {e}"))?;
    let cwd_text = cwd
        .to_str()
        .ok_or("Codex に渡すディレクトリを UTF-8 で表現できません")?;
    let mut server = Server::spawn("codex", &["app-server"], &cwd)?;
    server.send(json!({
        "id": 1, "method": "initialize",
        "params": {"clientInfo": {"name": "sscope", "version": env!("CARGO_PKG_VERSION")}}
    }))?;
    response(&server, 1)?;
    server.send(json!({"method": "initialized"}))?;
    server.send(json!({
        "id": 2, "method": "skills/list",
        "params": {"cwds": [cwd_text], "forceReload": true}
    }))?;
    parse_result(&response(&server, 2)?, &cwd)
}

fn response(server: &Server, id: u64) -> Result<Value, String> {
    loop {
        let message = server.receive()?;
        if message.get("id").and_then(Value::as_u64) != Some(id) {
            continue;
        }
        if let Some(error) = message.get("error") {
            return Err(format!("Codex の要求が失敗しました: {error}"));
        }
        return message
            .get("result")
            .cloned()
            .ok_or_else(|| "Codex の応答に result がありません".to_owned());
    }
}

fn parse_result(result: &Value, cwd: &Path) -> Result<Report, String> {
    let data = result
        .get("data")
        .and_then(Value::as_array)
        .ok_or("Codex の応答に data 配列がありません")?;
    let matching: Vec<_> = data
        .iter()
        .filter(|entry| {
            entry
                .get("cwd")
                .and_then(Value::as_str)
                .is_some_and(|value| Path::new(value) == cwd)
        })
        .collect();
    if matching.len() != 1 {
        return Err("Codex の応答に対象ディレクトリの結果が一意に含まれていません".to_owned());
    }
    let entry = matching[0];
    let skills = entry
        .get("skills")
        .and_then(Value::as_array)
        .ok_or("Codex の応答に skills 配列がありません")?;
    let errors = entry
        .get("errors")
        .and_then(Value::as_array)
        .ok_or("Codex の応答に errors 配列がありません")?;
    let mut report = Report::default();
    for error in errors {
        report
            .warnings
            .push(format!("Codex のスキル読み込みエラー: {error}"));
    }
    for skill in skills {
        match skill.get("enabled").and_then(Value::as_bool) {
            Some(false) => continue,
            Some(true) => {}
            None => {
                report
                    .warnings
                    .push("Codex のスキルの有効状態が不明です".to_owned());
                continue;
            }
        }
        let Some(name) = skill
            .get("name")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        else {
            report
                .warnings
                .push("Codex のスキル名が不明です".to_owned());
            continue;
        };
        let mut scope_note = String::new();
        let scope = match skill.get("scope").and_then(Value::as_str) {
            Some("repo") => Scope::Project,
            Some("user" | "system" | "admin") => Scope::Global,
            _ => {
                scope_note = format!(
                    "範囲未特定。scope={}",
                    skill.get("scope").unwrap_or(&Value::Null)
                );
                Scope::Unspecified
            }
        };
        let Some(path) = skill
            .get("path")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        else {
            report
                .warnings
                .push(format!("Codex のスキル {name} の path が不明です"));
            continue;
        };
        // Codex can report virtual paths. Preserve them without resolving them against CWD.
        let mut note = if Path::new(path).is_absolute() {
            String::new()
        } else {
            "Codex skills/list で有効。返却パスは仮想または相対パスで、実ファイルの位置は未確認"
                .to_owned()
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

    fn fixture(skills: Value, errors: Value) -> Value {
        json!({"data": [{"cwd": "/workspace", "skills": skills, "errors": errors}]})
    }

    #[test]
    fn keeps_duplicate_names_and_only_enabled_entries() {
        let value = fixture(
            json!([
                {"name":"review","path":"/repo/SKILL.md","scope":"repo","enabled":true},
                {"name":"review","path":"/home/SKILL.md","scope":"user","enabled":true},
                {"name":"disabled","path":"/other/SKILL.md","scope":"repo","enabled":false}
            ]),
            json!([]),
        );
        let report = parse_result(&value, Path::new("/workspace")).unwrap();
        assert_eq!(report.skills.len(), 2);
        assert!(matches!(report.skills[0].scope, Scope::Project));
        assert!(matches!(report.skills[1].scope, Scope::Global));
        assert_eq!(report.skills[0].name, report.skills[1].name);
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn reports_unknown_fields_and_load_errors_without_inference() {
        let value = fixture(
            json!([
                {"name":"future","path":"/future","scope":"future","enabled":true},
                {"name":"missing-path","scope":"repo","enabled":true},
                {"name":"missing-enabled","path":"/missing","scope":"user"}
            ]),
            json!([{"path":"/broken/SKILL.md","message":"invalid YAML"}]),
        );
        let report = parse_result(&value, Path::new("/workspace")).unwrap();
        assert_eq!(report.skills.len(), 1);
        assert!(matches!(report.skills[0].scope, Scope::Unspecified));
        assert!(report.skills[0].note.contains("future"));
        assert_eq!(report.warnings.len(), 3);
        assert!(report.warnings[0].contains("invalid YAML"));
    }

    #[test]
    fn preserves_virtual_paths_and_classifies_system_and_admin() {
        let value = fixture(
            json!([
                {"name":"virtual","path":"skill://bundled/review","scope":"system","enabled":true},
                {"name":"admin","path":"/etc/codex/skills/review/SKILL.md","scope":"admin","enabled":true}
            ]),
            json!([]),
        );
        let report = parse_result(&value, Path::new("/workspace")).unwrap();
        assert_eq!(report.skills[0].path, Path::new("skill://bundled/review"));
        assert!(report.skills[0].note.contains("仮想"));
        assert!(
            report
                .skills
                .iter()
                .all(|s| matches!(s.scope, Scope::Global))
        );
    }

    #[test]
    fn rejects_missing_directory_and_malformed_envelope() {
        assert!(parse_result(&json!({"data": []}), Path::new("/workspace")).is_err());
        assert!(
            parse_result(
                &json!({"data": [{"cwd":"/workspace", "skills":[]}]}),
                Path::new("/workspace")
            )
            .is_err()
        );
    }
}
