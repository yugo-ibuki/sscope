use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

struct Workspace {
    temp: TempDir,
    home: PathBuf,
}

impl Workspace {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let home = temp.path().canonicalize().unwrap().join("home");
        fs::create_dir(&home).unwrap();
        Self { temp, home }
    }

    fn cli(&self, cwd: &Path, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_sscope"))
            .env_clear()
            .env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .env("PATH", self.temp.path().join("no-binaries"))
            .current_dir(cwd)
            .args(args)
            .output()
            .unwrap()
    }
}

fn skill(root: &Path, name: &str) {
    let dir = root.join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: A test skill\n---\n"),
    )
    .unwrap();
}

#[test]
fn global_outside_repo_and_project_override_from_nested_cwd() {
    let w = Workspace::new();
    skill(&w.home.join(".kiro/skills"), "review");
    let outside = w.temp.path().join("outside");
    fs::create_dir(&outside).unwrap();
    let output = w.cli(&outside, &["--agent", "kiro-cli"]);
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("グローバル (1定義)") && text.contains("~/.kiro/skills/review/SKILL.md"));
    assert!(!text.contains("プロジェクト ("));

    let repo = w.temp.path().join("repo");
    skill(&repo.join(".kiro/skills"), "review");
    fs::create_dir(repo.join(".git")).unwrap();
    let child = repo.join("pkg");
    fs::create_dir(&child).unwrap();
    let output = w.cli(&child, &[]);
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.contains("Kiro CLI")
            && text.contains("プロジェクト (1スキル)")
            && text.contains("review")
    );
    assert!(!text.contains("グローバル ("));
}

#[test]
fn shared_directory_does_not_claim_every_agent() {
    let w = Workspace::new();
    skill(&w.home.join(".agents/skills"), "personal");
    let output = w.cli(w.temp.path(), &[]);
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("利用エージェント未特定") && text.contains("personal"));
    assert!(!text.contains("Codex") && !text.contains("Cursor"));
}

#[test]
fn gemini_disabled_setting_applies_to_listing() {
    let w = Workspace::new();
    skill(&w.home.join(".gemini/skills"), "hidden");
    fs::write(
        w.home.join(".gemini/settings.json"),
        r#"{"skills":{"disabled":["hidden"]}}"#,
    )
    .unwrap();
    let output = w.cli(w.temp.path(), &["--agent", "Gemini CLI"]);
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains("hidden"));
    assert!(text.contains("Gemini CLI"));
}

#[test]
fn codex_fallback_keeps_same_names_and_honors_personal_disable_rules() {
    let w = Workspace::new();
    let repo = w.temp.path().join("repo");
    skill(&repo.join(".agents/skills"), "review");
    skill(&w.home.join(".codex/skills"), "review");
    skill(&w.home.join(".codex/skills"), "hidden");
    fs::create_dir(repo.join(".git")).unwrap();
    let child = repo.join("sub");
    fs::create_dir(&child).unwrap();
    fs::write(
        w.home.join(".codex/config.toml"),
        "[[skills.config]]\nname='hidden'\nenabled=false\n",
    )
    .unwrap();
    let output = w.cli(&child, &["--agent", "codex"]);
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("ローカル配置") && !text.contains("[本体で確認]"));
    assert!(text.contains("グローバル (1定義)") && text.contains("プロジェクト (1定義)"));
    assert!(!text.contains("hidden"));
}

fn json_output(output: &Output) -> serde_json::Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output.stdout.contains(&0x1b));
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn json_empty_and_detected_agent_without_skills() {
    let w = Workspace::new();
    let output = w.cli(w.temp.path(), &["--json"]);
    let document = json_output(&output);
    assert_eq!(
        document["cwd"],
        w.temp.path().canonicalize().unwrap().to_str().unwrap()
    );
    assert_eq!(document["agents"], serde_json::json!([]));
    assert!(output.stderr.is_empty());

    fs::create_dir(w.home.join(".codex")).unwrap();
    let output = w.cli(w.temp.path(), &["--json"]);
    assert_eq!(
        json_output(&output)["agents"],
        serde_json::json!([
            {"name":"Codex", "evidence":"candidate", "skills":[]}
        ])
    );
}

#[test]
fn json_shared_fallback_has_no_invented_agent() {
    let w = Workspace::new();
    skill(&w.home.join(".agents/skills"), "personal");
    let output = w.cli(w.temp.path(), &["--json"]);
    assert_eq!(
        json_output(&output)["agents"],
        serde_json::json!([{
            "name":null,
            "evidence":"candidate",
            "skills":[{
                "name":"personal", "scope":"global",
                "sources":[{"path":w.home.join(".agents/skills/personal/SKILL.md"), "note":null}]
            }]
        }])
    );
}

#[test]
fn json_preserves_sources_and_filters_agents_in_either_argument_order() {
    let w = Workspace::new();
    skill(&w.home.join(".agents/skills"), "review");
    skill(&w.home.join(".codex/skills"), "review");
    skill(&w.home.join(".kiro/skills"), "other");
    let first = w.cli(w.temp.path(), &["--agent", "codex", "--json"]);
    let second = w.cli(w.temp.path(), &["--json", "--agent", "Codex"]);
    assert_eq!(first.stdout, second.stdout);
    let document = json_output(&first);
    assert_eq!(document["agents"].as_array().unwrap().len(), 1);
    let agent = &document["agents"][0];
    assert_eq!(agent["name"], "Codex");
    assert_eq!(agent["skills"].as_array().unwrap().len(), 1);
    assert_eq!(
        agent["skills"][0]["sources"],
        serde_json::json!([
            {"path":w.home.join(".agents/skills/review/SKILL.md"), "note":null},
            {"path":w.home.join(".codex/skills/review/SKILL.md"), "note":null}
        ])
    );
    let all = w.cli(w.temp.path(), &["--json"]);
    assert_eq!(json_output(&all)["agents"].as_array().unwrap().len(), 2);
}

#[test]
fn json_diagnostics_are_only_on_stderr() {
    let w = Workspace::new();
    skill(&w.home.join(".codex/skills"), "review");
    fs::write(w.home.join(".codex/config.toml"), "[invalid").unwrap();
    let output = w.cli(w.temp.path(), &["--json"]);
    assert_eq!(json_output(&output)["agents"][0]["name"], "Codex");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("TOML 設定を解析できません"));
    assert!(!stderr.contains('\x1b'));
    assert!(!String::from_utf8(output.stdout).unwrap().contains("TOML"));
}

#[test]
fn invalid_json_arguments_fail_without_stdout() {
    let w = Workspace::new();
    for args in [
        vec!["--jsno"],
        vec!["--json", "--json"],
        vec!["--agent"],
        vec!["--json", "--agent"],
        vec!["--agent", "--json"],
        vec!["--agent", ""],
        vec!["--agent", "!!!"],
        vec!["--agent", "codex", "--agent", "codex"],
        vec!["--json=true"],
    ] {
        let output = w.cli(w.temp.path(), &args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
        assert!(!output.stderr.is_empty(), "{args:?}");
    }
    let output = w.cli(w.temp.path(), &["--json", "--agent", "unknown"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("指定されたエージェントを検出できません")
    );
}

#[cfg(target_os = "linux")]
#[test]
fn json_non_utf8_source_fails_before_output() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    let w = Workspace::new();
    let root = w
        .home
        .join(".codex/skills")
        .join(OsString::from_vec(vec![b's', 0xff]));
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("SKILL.md"),
        "---\nname: valid\ndescription: Valid metadata\n---\n",
    )
    .unwrap();
    let output = w.cli(w.temp.path(), &["--json"]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8(output.stderr).unwrap().contains("UTF-8"));
}
