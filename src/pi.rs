use std::path::Path;

use serde_json::{Value, json};

use crate::{Report, Scope, Skill, rpc::Server};

pub fn list(cwd: &Path) -> Result<Report, String> {
    let mut server = Server::spawn(
        "pi",
        &[
            "--mode",
            "rpc",
            "--offline",
            "--no-session",
            "--no-extensions",
            "--no-mcp",
            "--no-tools",
        ],
        cwd,
    )?;
    server.send(json!({"id":"sscope-list", "type":"get_commands"}))?;
    loop {
        let message = server.receive()?;
        if message.get("id").and_then(Value::as_str) == Some("sscope-list") {
            return parse_response(&message);
        }
    }
}

fn parse_response(message: &Value) -> Result<Report, String> {
    if message.get("type").and_then(Value::as_str) != Some("response")
        || message.get("command").and_then(Value::as_str) != Some("get_commands")
    {
        return Err("Pi の応答形式を確認できません".to_owned());
    }
    if message.get("success").and_then(Value::as_bool) != Some(true) {
        return Err(format!(
            "Pi の一覧取得が失敗しました: {}",
            message.get("error").unwrap_or(&Value::Null)
        ));
    }
    let commands = message
        .pointer("/data/commands")
        .and_then(Value::as_array)
        .ok_or("Pi の応答に commands 配列がありません")?;
    let mut report = Report::default();
    for command in commands {
        if command.get("source").and_then(Value::as_str) != Some("skill") {
            continue;
        }
        let Some(name) = command
            .get("name")
            .and_then(Value::as_str)
            .and_then(|s| s.strip_prefix("skill:"))
            .filter(|s| !s.is_empty())
        else {
            report
                .warnings
                .push("Pi のスキルコマンド名が不明です".to_owned());
            continue;
        };
        let mut scope_note = String::new();
        let scope = match command.pointer("/sourceInfo/scope").and_then(Value::as_str) {
            Some("project") => Scope::Project,
            Some("user") => Scope::Global,
            _ => {
                scope_note = format!(
                    "範囲未特定。scope={}",
                    command.pointer("/sourceInfo/scope").unwrap_or(&Value::Null)
                );
                Scope::Unspecified
            }
        };
        let Some(path) = command
            .pointer("/sourceInfo/path")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        else {
            report
                .warnings
                .push(format!("Pi のスキル {name} の path が不明です"));
            continue;
        };
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

    fn response(commands: Value) -> Value {
        json!({"type":"response", "command":"get_commands", "success":true, "data":{"commands":commands}})
    }

    #[test]
    fn parses_skill_commands_and_preserves_scopes() {
        let report = parse_response(&response(json!([
            {"name":"skill:review", "source":"skill", "sourceInfo":{"scope":"project", "path":"/repo/SKILL.md"}},
            {"name":"skill:review", "source":"skill", "sourceInfo":{"scope":"user", "path":"/home/SKILL.md"}},
            {"name":"prompt", "source":"prompt", "sourceInfo":{"scope":"user", "path":"/prompt.md"}}
        ]))).unwrap();
        assert_eq!(report.skills.len(), 2);
        assert_eq!(report.skills[0].name, "review");
        assert!(matches!(report.skills[0].scope, Scope::Project));
        assert!(matches!(report.skills[1].scope, Scope::Global));
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn reports_unknown_metadata_without_guessing() {
        let report = parse_response(&response(json!([
            {"name":"skill:temporary", "source":"skill", "sourceInfo":{"scope":"temporary", "path":"/temporary"}},
            {"name":"skill:missing", "source":"skill", "sourceInfo":{"scope":"user"}}
        ]))).unwrap();
        assert_eq!(report.skills.len(), 1);
        assert!(matches!(report.skills[0].scope, Scope::Unspecified));
        assert!(report.skills[0].note.contains("temporary"));
        assert_eq!(report.warnings.len(), 1);
    }

    #[test]
    fn rejects_failed_or_incomplete_responses() {
        assert!(parse_response(
            &json!({"type":"response", "command":"get_commands", "success":false, "error":"failed"})
        )
        .is_err());
        assert!(
            parse_response(&json!({"type":"response", "command":"get_commands", "success":true}))
                .is_err()
        );
    }
}
