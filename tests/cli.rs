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
        let home = temp.path().join("home");
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
    assert!(text.contains("グローバル (1件)") && text.contains("~/.kiro/skills/review/SKILL.md"));
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
        text.contains("Kiro CLI") && text.contains("プロジェクト (1件)") && text.contains("review")
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
    assert!(text.contains("グローバル (1件)") && text.contains("プロジェクト (1件)"));
    assert!(!text.contains("hidden"));
}
