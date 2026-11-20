//! Local discovery candidates, not a claim that a running product enabled them.
//! Standard locations are embedded so the research notes are not runtime inputs.
use std::env;
use std::path::{Path, PathBuf};

use crate::Scope;
use crate::scan::{Collision, Root};

pub struct Agent {
    pub name: &'static str,
    pub roots: Vec<Root>,
    pub collision: Collision,
}

struct Spec {
    name: &'static str,
    marker: &'static str,
    executable: &'static str,
    global: &'static str,
    project: &'static str,
}

// These are installation-layout candidates. Keep all names because product
// precedence is not established for these entries. Shared paths never detect a
// product; aliases sharing a marker are represented once.
#[rustfmt::skip]
const CATALOG: &[Spec] = &[
    Spec { name: "AiderDesk", marker: ".aider-desk", executable: "", global: "~/.aider-desk/skills", project: ".aider-desk/skills" },
    Spec { name: "AstrBot", marker: ".astrbot", executable: "", global: "~/.astrbot/data/skills", project: "data/skills" },
    Spec { name: "Autohand Code", marker: ".autohand", executable: "autohand", global: "@AUTOHAND_HOME|~/.autohand/skills", project: ".autohand/skills" },
    Spec { name: "IBM Bob", marker: ".bob", executable: "", global: "~/.bob/skills", project: ".bob/skills" },
    Spec { name: "CodeArts Agent", marker: ".codeartsdoer", executable: "", global: "~/.codeartsdoer/skills", project: ".codeartsdoer/skills" },
    Spec { name: "CodeBuddy", marker: ".codebuddy", executable: "codebuddy", global: "~/.codebuddy/skills", project: ".codebuddy/skills" },
    Spec { name: "Codemaker", marker: ".codemaker", executable: "", global: "~/.codemaker/skills", project: ".codemaker/skills" },
    Spec { name: "Code Studio", marker: ".codestudio", executable: "", global: "~/.codestudio/skills", project: ".codestudio/skills" },
    Spec { name: "Command Code", marker: ".commandcode", executable: "", global: "~/.commandcode/skills", project: ".commandcode/skills" },
    Spec { name: "ForgeCode", marker: ".forge", executable: "forgecode", global: "~/.forge/skills", project: ".forge/skills" },
    Spec { name: "Grok Build", marker: ".grok", executable: "", global: "@GROK_HOME|~/.grok/skills", project: ".grok/skills" },
    Spec { name: "inference.sh", marker: ".inferencesh", executable: "", global: "~/.inferencesh/skills", project: ".inferencesh/skills" },
    Spec { name: "Jazz", marker: ".jazz", executable: "", global: "~/.jazz/skills", project: ".jazz/skills" },
    Spec { name: "iFlow CLI", marker: ".iflow", executable: "iflow", global: "~/.iflow/skills", project: ".iflow/skills" },
    Spec { name: "Kimchi", marker: ".kimchi", executable: "", global: "~/.config/kimchi/harness/skills", project: ".kimchi/skills" },
    Spec { name: "Kode", marker: ".kode", executable: "", global: "~/.kode/skills", project: ".kode/skills" },
    Spec { name: "Lingma", marker: ".lingma", executable: "", global: "~/.lingma/skills", project: ".lingma/skills" },
    Spec { name: "MCPJam", marker: ".mcpjam", executable: "mcpjam", global: "~/.mcpjam/skills", project: ".mcpjam/skills" },
    Spec { name: "MiniMax Code", marker: ".minimax", executable: "", global: "~/.minimax/skills", project: ".minimax/skills" },
    Spec { name: "Moxby", marker: ".moxby", executable: "", global: "~/.moxby/skills", project: ".moxby/skills" },
    Spec { name: "Mux", marker: ".mux", executable: "", global: "~/.mux/skills", project: ".mux/skills" },
    Spec { name: "Ona", marker: ".ona", executable: "", global: "~/.ona/skills", project: ".ona/skills" },
    Spec { name: "Posit Assistant", marker: ".posit/assistant", executable: "", global: "~/.posit/assistant/skills", project: ".posit/assistant/skills" },
    Spec { name: "Qoder", marker: ".qoder", executable: "qoder", global: "~/.qoder/skills;~/.agents/skills", project: ".qoder/skills" },
    Spec { name: "Qoder CN", marker: ".qoder-cn", executable: "", global: "~/.qoder-cn/skills;~/.agents/skills", project: ".qoder/skills" },
    Spec { name: "Reasonix", marker: ".reasonix", executable: "", global: "@REASONIX_HOME|~/.reasonix/skills", project: ".reasonix/skills" },
    Spec { name: "Tabnine CLI", marker: ".tabnine", executable: "tabnine", global: "~/.tabnine/agent/skills", project: ".tabnine/agent/skills" },
    Spec { name: "Terramind", marker: ".terramind", executable: "", global: "~/.terramind/skills", project: ".terramind/skills" },
    Spec { name: "Tinycloud", marker: ".tinycloud", executable: "", global: "~/.tinycloud/skills", project: ".tinycloud/skills" },
    Spec { name: "Trae", marker: ".trae", executable: "", global: "~/.trae/skills", project: ".trae/skills" },
    Spec { name: "Trae CN", marker: ".trae-cn", executable: "", global: "~/.trae-cn/skills", project: ".trae/skills" },
    Spec { name: "ZCode", marker: ".zcode", executable: "", global: "~/.zcode/skills", project: ".zcode/skills" },
    Spec { name: "Zencoder / Zenflow", marker: ".zencoder", executable: "", global: "~/.zencoder/skills", project: ".zencoder/skills" },
    Spec { name: "Neovate", marker: ".neovate", executable: "", global: "~/.neovate/skills", project: ".neovate/skills" },
    Spec { name: "Pochi", marker: ".pochi", executable: "pochi", global: "~/.pochi/skills", project: ".pochi/skills" },
    Spec { name: "AdaL", marker: ".adal", executable: "", global: "~/.adal/skills", project: ".adal/skills" },
    Spec { name: "Dexto", marker: ".dexto", executable: "dexto", global: "~/.agents/skills", project: ".agents/skills" },
    Spec { name: "Firebender", marker: ".firebender", executable: "", global: "~/.firebender/skills;~/.agents/skills", project: ".agents/skills" },
    Spec { name: "fx", marker: ".fx", executable: "", global: "~/.agents/skills", project: ".agents/skills" },
    Spec { name: "Loaf", marker: ".loaf", executable: "", global: "~/.agents/skills", project: ".agents/skills" },
    Spec { name: "Sarvam Code", marker: ".sarvam", executable: "sarvam", global: "~/.agents/skills", project: ".agents/skills" },
    Spec { name: "Replit Agent", marker: ".replit", executable: "", global: "", project: ".agents/skills" },
];

fn env_path(name: &str) -> Option<PathBuf> {
    env::var_os(name).and_then(|value| {
        let text = value.to_string_lossy();
        let trimmed = text.trim();
        (!trimmed.is_empty()).then(|| PathBuf::from(trimmed))
    })
}

fn resolve(value: &str, home: &Path) -> PathBuf {
    if let Some(value) = value.strip_prefix('@') {
        let (variable, fallback) = value.split_once('|').unwrap();
        // Overrides name the product's configuration home, not its skills folder.
        return env_path(variable)
            .map(|p| p.join("skills"))
            .unwrap_or_else(|| resolve(fallback, home));
    }
    if let Some(value) = value.strip_prefix("~config/") {
        return env_path("XDG_CONFIG_HOME")
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(".config"))
            .join(value);
    }
    value
        .strip_prefix("~/")
        .map_or_else(|| PathBuf::from(value), |p| home.join(p))
}

fn executable(name: &str) -> bool {
    !name.is_empty() && crate::on_path(name)
}

fn eve_project(base: &Path) -> bool {
    if !base.join("agent").is_dir() {
        return false;
    }
    let Ok(contents) = std::fs::read(base.join("package.json")) else {
        return false;
    };
    let Ok(package) = serde_json::from_slice::<serde_json::Value>(&contents) else {
        return false;
    };
    ["dependencies", "devDependencies"]
        .into_iter()
        .any(|section| {
            package
                .get(section)
                .and_then(|deps| deps.get("eve"))
                .and_then(serde_json::Value::as_str)
                .is_some_and(|version| !version.is_empty())
        })
}

fn ancestors(cwd: &Path, repo: Option<&Path>, home: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    for path in cwd.ancestors() {
        // A user's home configuration must not become a project configuration.
        if path == home {
            break;
        }
        result.push(path.to_path_buf());
        if Some(path) == repo {
            break;
        }
    }
    result
}

struct Discovery<'a> {
    cwd: &'a Path,
    repo: Option<&'a Path>,
    home: &'a Path,
    result: Vec<Agent>,
}
impl Discovery<'_> {
    // Keeping the registry row declarative makes product rules easy to compare.
    #[allow(clippy::too_many_arguments)]
    fn add(
        &mut self,
        name: &'static str,
        marker: &str,
        exe: &str,
        global: &str,
        project: &str,
        ancestors_search: bool,
        user_first: bool,
        depth: usize,
        collision: Collision,
    ) {
        let ancestors = ancestors(self.cwd, self.repo, self.home);
        let global_paths: Vec<_> = global
            .split(';')
            .filter(|p| !p.is_empty())
            .map(|p| resolve(p, self.home))
            .collect();
        let marker_global = self.home.join(marker);
        let config_signal = global_paths.iter().any(|p| {
            // Compatibility directories are search sources, never evidence that
            // the consuming product itself is installed.
            let parent = p.parent().unwrap_or(p);
            let own = match name {
                "Cursor" => parent == self.home.join(".cursor"),
                "Gemini CLI" => *p == self.home.join(".gemini/skills") && p.is_dir(),
                "Antigravity App" => {
                    parent == self.home.join(".gemini/antigravity")
                        || parent == self.home.join(".gemini/config")
                }
                "Antigravity CLI" => parent == self.home.join(".gemini/antigravity-cli"),
                "OpenClaw" => parent == marker_global,
                _ => {
                    let compatible = [
                        "~/.agents/skills",
                        "~config/agents/skills",
                        "~/.claude/skills",
                        "~/.codex/skills",
                        "~/.agent/skills",
                    ];
                    !compatible.iter().any(|s| *p == resolve(s, self.home))
                        || (name == "Claude Code"
                            && *p == resolve("@CLAUDE_CONFIG_DIR|~/.claude/skills", self.home))
                }
            };
            own && parent.is_dir()
        });
        let has_marker = |base: &Path| match name {
            "Gemini CLI" => {
                base.join(".gemini/settings.json").exists() || base.join(".gemini/skills").is_dir()
            }
            "GitHub Copilot CLI" => {
                base.join(marker).exists() || base.join(".github/skills").is_dir()
            }
            "Cline" => base.join(marker).exists() || base.join(".clinerules/skills").is_dir(),
            "Eve" => eve_project(base),
            "PromptScript" => {
                base.join(".promptscript").exists() || base.join("promptscript.yaml").is_file()
            }
            _ => base.join(marker).exists(),
        };
        let marker_project = ancestors.iter().find(|p| has_marker(p));
        let home_signal = !global.is_empty() && has_marker(self.home);
        let executable_signal = executable(exe) || (name == "Cursor" && executable("cursor-agent"));
        if !home_signal && !config_signal && marker_project.is_none() && !executable_signal {
            return;
        }
        let projects = if matches!(name, "Goose" | "Continue CLI" | "Cline") {
            (self.cwd != self.home)
                .then(|| self.cwd.to_path_buf())
                .into_iter()
                .collect()
        } else if ancestors_search {
            ancestors
        } else {
            self.repo
                .filter(|p| *p != self.home)
                .map(Path::to_path_buf)
                .or_else(|| marker_project.cloned())
                .or_else(|| (self.cwd != self.home).then(|| self.cwd.to_path_buf()))
                .into_iter()
                .collect()
        };
        let mut roots = Vec::new();
        let mut push = |path: PathBuf, scope: Scope| {
            if !roots.iter().any(|r: &Root| r.path == path) {
                roots.push(Root { path, scope, depth });
            }
        };
        if user_first {
            for p in &global_paths {
                push(p.clone(), Scope::Global);
            }
        }
        for base in &projects {
            for relative in project.split(';').filter(|p| !p.is_empty()) {
                push(base.join(relative), Scope::Project);
            }
        }
        if !user_first {
            for p in global_paths {
                push(p, Scope::Global);
            }
        }
        self.result.push(Agent {
            name,
            roots,
            collision,
        });
    }
}

#[rustfmt::skip]
pub fn detected(cwd: &Path, repo: Option<&Path>, home: &Path) -> Vec<Agent> {
    let mut d = Discovery { cwd, repo, home, result: Vec::new() };
    // Known standard search rules. Settings, plugins, trust, and session caches
    // remain unverified; these are disk candidates even where precedence is known.
    d.add("Claude Code", ".claude", "claude", "@CLAUDE_CONFIG_DIR|~/.claude/skills", ".claude/skills", true, true, 1, Collision::First);
    d.add("Cursor", ".cursor", "cursor", "~/.agents/skills;~/.cursor/skills;~/.claude/skills;~/.codex/skills", ".agents/skills;.cursor/skills;.claude/skills;.codex/skills", false, false, 64, Collision::Keep);
    d.add("Pi", ".pi", "pi", "@PI_CODING_AGENT_DIR|~/.pi/agent/skills;~/.agents/skills", ".pi/skills;.agents/skills", true, false, 64, Collision::First);
    d.add("Gemini CLI", ".gemini", "gemini", "~/.agents/skills;~/.gemini/skills", ".agents/skills;.gemini/skills", false, false, 1, Collision::First);
    d.add("GitHub Copilot CLI", ".copilot", "copilot", "~/.copilot/skills;~/.agents/skills", ".github/skills;.claude/skills;.agents/skills", false, false, 1, Collision::Keep);
    d.add("Kiro CLI", ".kiro", "kiro-cli", "~/.kiro/skills", ".kiro/skills", false, false, 1, Collision::First);
    d.add("Goose", ".goose", "goose", "~/.agents/skills;~config/goose/skills;~/.claude/skills;~config/agents/skills", ".agents/skills;.goose/skills;.claude/skills", false, false, 1, Collision::First);
    d.add("Zed", ".zed", "zed", "~/.agents/skills;~config/zed/skills", ".agents/skills", false, false, 1, Collision::First);
    // Zed's config directory detects installation; it is not a documented skills source.
    if let Some(agent) = d.result.iter_mut().find(|a| a.name == "Zed") { agent.roots.retain(|r| r.path != resolve("~config/zed/skills", home)); }
    d.add("Cline", ".cline", "cline", "~/.agents/skills;~/.cline/skills", ".agents/skills;.claude/skills;.cline/skills;.clinerules/skills", false, true, 1, Collision::First);
    d.add("OpenCode", ".opencode", "opencode", "~config/opencode/skills;~/.agents/skills;~/.claude/skills", ".opencode/skills;.agents/skills;.claude/skills", true, false, 6, Collision::Keep);
    d.add("Amp", ".amp", "amp", "~config/agents/skills;~/.agents/skills;~config/amp/skills;~/.claude/skills", ".agents/skills;.claude/skills", true, true, 5, Collision::First);
    if let Some(agent) = d.result.iter_mut().find(|a| a.name == "Amp") {
        // Amp's personal Claude fallback follows every project source.
        agent.roots.sort_by_key(|root| {
            if root.path == resolve("~config/agents/skills", home) { 0 }
            else if root.path == home.join(".agents/skills") { 1 }
            else if root.path == resolve("~config/amp/skills", home) { 2 }
            else if root.scope == Scope::Project && root.path.ends_with(".agents/skills") { 3 }
            else if root.scope == Scope::Project { 4 }
            else { 5 }
        });
    }
    d.add("Devin Local / Windsurf", ".devin", "devin", "~config/devin/skills;~/.agents/skills;~/.codeium/windsurf/skills", ".devin/skills;.agents/skills;.windsurf/skills", false, false, 1, Collision::Keep);
    d.add("Windsurf (legacy)", ".windsurf", "windsurf", "~/.codeium/windsurf/skills", ".windsurf/skills", false, false, 1, Collision::Keep);
    // A legacy install must not produce two labels from the same global signal.
    if d.result.iter().any(|a| a.name == "Windsurf (legacy)") && !home.join(".devin").exists() && !resolve("~config/devin", home).exists() && !ancestors(cwd, repo, home).iter().any(|p| p.join(".devin").exists()) && !executable("devin") { d.result.retain(|a| a.name != "Devin Local / Windsurf"); }
    d.add("Roo Code", ".roo", "", "~/.roo/skills;~/.agents/skills", ".roo/skills;.agents/skills", false, false, 1, Collision::First);
    d.add("Kilo Code", ".kilo", "kilo", "~/.kilo/skills;~/.agents/skills", ".kilo/skills;.agents/skills", false, false, 1, Collision::First);
    d.add("Junie CLI", ".junie", "junie", "~/.junie/skills;~/.agents/skills", ".junie/skills;.agents/skills", false, false, 1, Collision::Keep);
    d.add("Augment CLI", ".augment", "auggie", "~/.augment/skills", ".augment/skills;.claude/skills;.agents/skills", true, true, 1, Collision::First);
    d.add("Continue CLI", ".continue", "", "@CONTINUE_GLOBAL_DIR|~/.continue/skills", ".continue/skills;.claude/skills", false, false, 1, Collision::Keep);
    d.add("Cortex Code", ".cortex", "cortex", "~/.snowflake/cortex/skills;~/.claude/skills", ".cortex/skills;.claude/skills", false, false, 1, Collision::First);
    d.add("Crush", ".crush", "crush", "~config/agents/skills;~config/crush/skills;~/.agents/skills;~/.claude/skills", ".agents/skills;.crush/skills;.claude/skills;.cursor/skills", false, false, 1, Collision::Keep);
    d.add("Deep Agents", ".deepagents", "deepagents", "~/.agents/skills;~/.deepagents/agent/skills", ".agents/skills;.deepagents/skills", false, false, 1, Collision::First);
    d.add("Factory Droid", ".factory", "droid", "~/.factory/skills;~/.agents/skills;~/.agent/skills", ".factory/skills;.agents/skills;.agent/skills", false, false, 64, Collision::Keep);
    d.add("Qwen Code", ".qwen", "qwen", "~/.qwen/skills;~/.agents/skills", ".qwen/skills;.agents/skills", false, false, 1, Collision::First);
    d.add("Mistral Vibe", ".vibe", "vibe", "@VIBE_HOME|~/.vibe/skills;~/.agents/skills", ".vibe/skills;.agents/skills", false, false, 1, Collision::Keep);
    d.add("Letta Code", ".letta", "letta", "~/.letta/skills", ".agents/skills", false, false, 1, Collision::Keep);
    d.add("Hermes Agent", ".hermes", "hermes", "@HERMES_HOME|~/.hermes/skills", ".hermes/skills;.agents/skills", false, false, 1, Collision::First);
    d.add("Kimi Code CLI", ".kimi-code", "kimi", "@KIMI_CODE_HOME|~/.kimi-code/skills;~/.agents/skills", ".kimi-code/skills;.kimi/skills;.agents/skills", false, false, 1, Collision::Keep);
    d.add("OpenHands", ".openhands", "openhands", "~/.agents/skills;~/.openhands/skills;~/.openhands/microagents", ".agents/skills;.openhands/skills;.openhands/microagents", false, false, 1, Collision::First);
    d.add("Rovo Dev", ".rovodev", "rovodev", "~/.rovodev/skills;~/.agents/skills", ".rovodev/skills;.agents/skills", false, true, 1, Collision::First);
    d.add("Warp", ".warp", "", "~/.agents/skills;~/.warp/skills;~/.claude/skills", ".agents/skills;.warp/skills;.claude/skills", true, false, 1, Collision::Keep);
    d.add("Antigravity App", ".antigravity", "", "~/.gemini/config/skills;~/.gemini/antigravity/skills", ".agents/skills", true, false, 1, Collision::Keep);
    d.add("Antigravity CLI", ".antigravity-cli", "agy", "~/.gemini/antigravity-cli/skills;~/.gemini/skills;~/.gemini/config/skills", ".agents/skills", true, true, 1, Collision::Keep);
    d.add("GitLab Duo", ".gitlab/duo", "", "~/.gitlab/duo/skills", ".agents/skills;skills", false, false, 1, Collision::Keep);
    d.add("Eve", "agent", "", "", "agent/skills", false, false, 1, Collision::Keep);
    d.add("PromptScript", ".promptscript", "", "", ".agents/skills", false, false, 1, Collision::Keep);
    // OpenClaw's actual workspace can be configured elsewhere; cwd is only a candidate.
    let claw_home = [".openclaw", ".clawdbot", ".moltbot"].into_iter().find(|p| home.join(p).is_dir()).unwrap_or(".openclaw");
    let claw_global = format!("~/.agents/skills;~/{claw_home}/skills");
    d.add("OpenClaw", claw_home, "openclaw", &claw_global, "skills;.agents/skills", false, false, 1, Collision::First);
    for s in CATALOG { d.add(s.name, s.marker, s.executable, s.global, s.project, false, false, 1, Collision::Keep); }
    d.result.sort_by_key(|a| a.name);
    d.result
}

pub fn codex_roots(cwd: &Path, repo: Option<&Path>, home: &Path) -> Vec<Root> {
    let mut roots: Vec<_> = ancestors(cwd, repo, home)
        .into_iter()
        .map(|p| Root {
            path: p.join(".agents/skills"),
            scope: Scope::Project,
            depth: 1,
        })
        .collect();
    roots.push(Root {
        path: home.join(".agents/skills"),
        scope: Scope::Global,
        depth: 1,
    });
    roots.push(Root {
        path: env_path("CODEX_HOME")
            .unwrap_or_else(|| home.join(".codex"))
            .join("skills"),
        scope: Scope::Global,
        depth: 1,
    });
    roots.push(Root {
        path: PathBuf::from("/etc/codex/skills"),
        scope: Scope::Global,
        depth: 1,
    });
    roots
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn shared_roots_do_not_detect_products() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let cwd = temp.path().join("project");
        fs::create_dir_all(home.join(".agents/skills")).unwrap();
        fs::create_dir_all(home.join(".claude/skills")).unwrap();
        fs::create_dir_all(cwd.join(".agents/skills")).unwrap();
        let mut d = Discovery {
            cwd: &cwd,
            repo: None,
            home: &home,
            result: vec![],
        };
        d.add(
            "Example",
            ".example",
            "",
            "~/.agents/skills;~/.claude/skills",
            ".agents/skills",
            false,
            false,
            1,
            Collision::Keep,
        );
        assert!(d.result.is_empty());
    }

    #[test]
    fn non_git_project_uses_nearest_specific_marker() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let project = home.join("workspace");
        let cwd = project.join("src/deep");
        fs::create_dir_all(project.join(".example/skills")).unwrap();
        fs::create_dir_all(&cwd).unwrap();
        let mut d = Discovery {
            cwd: &cwd,
            repo: None,
            home: &home,
            result: vec![],
        };
        d.add(
            "Example",
            ".example",
            "",
            "~/.example/skills",
            ".example/skills",
            false,
            false,
            1,
            Collision::Keep,
        );
        assert_eq!(d.result.len(), 1);
        assert_eq!(d.result[0].roots[0].path, project.join(".example/skills"));
        assert_eq!(d.result[0].roots[0].scope, Scope::Project);
    }

    #[test]
    fn home_marker_is_never_a_project_marker() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        fs::create_dir_all(home.join(".example/skills")).unwrap();
        let mut d = Discovery {
            cwd: home,
            repo: None,
            home,
            result: vec![],
        };
        d.add(
            "Example",
            ".example",
            "",
            "~/.example/skills",
            ".example/skills",
            true,
            false,
            1,
            Collision::Keep,
        );
        assert_eq!(d.result.len(), 1);
        assert!(d.result[0].roots.iter().all(|r| r.scope == Scope::Global));
    }

    #[test]
    fn amp_uses_first_name_and_delays_personal_claude_fallback() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let project = home.join("workspace");
        fs::create_dir_all(home.join(".amp")).unwrap();
        fs::create_dir_all(project.join(".agents/skills")).unwrap();
        let amp = detected(&project, None, &home)
            .into_iter()
            .find(|a| a.name == "Amp")
            .unwrap();
        assert!(matches!(amp.collision, Collision::First));
        let position = |path: &Path| amp.roots.iter().position(|r| r.path == path).unwrap();
        assert!(
            position(&resolve("~config/amp/skills", &home))
                < position(&project.join(".agents/skills"))
        );
        assert!(position(&project.join(".claude/skills")) < position(&home.join(".claude/skills")));
    }

    #[test]
    fn eve_requires_directory_and_valid_package_dependency() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let cwd = temp.path().join("project");
        fs::create_dir_all(cwd.join("agent/skills")).unwrap();
        for invalid in [
            "not JSON",
            r#"{"dependencies":{"other":"1"}}"#,
            r#"{"dependencies":{"eve":null}}"#,
        ] {
            fs::write(cwd.join("package.json"), invalid).unwrap();
            assert!(!detected(&cwd, None, &home).iter().any(|a| a.name == "Eve"));
        }
        for section in ["dependencies", "devDependencies"] {
            fs::write(
                cwd.join("package.json"),
                format!(r#"{{"{section}":{{"eve":"1"}}}}"#),
            )
            .unwrap();
            let eve = detected(&cwd, None, &home)
                .into_iter()
                .find(|a| a.name == "Eve")
                .unwrap();
            assert_eq!(eve.roots.len(), 1);
            assert_eq!(eve.roots[0].path, cwd.join("agent/skills"));
            assert_eq!(eve.roots[0].scope, Scope::Project);
        }
        fs::remove_dir_all(cwd.join("agent")).unwrap();
        assert!(!eve_project(&cwd));
    }

    #[test]
    fn promptscript_markers_have_project_roots_only() {
        for marker in [".promptscript", "promptscript.yaml"] {
            let temp = tempfile::tempdir().unwrap();
            let home = temp.path().join("home");
            let cwd = temp.path().join("project");
            fs::create_dir_all(&cwd).unwrap();
            fs::write(cwd.join(marker), "").unwrap();
            let agent = detected(&cwd, None, &home)
                .into_iter()
                .find(|a| a.name == "PromptScript")
                .unwrap();
            assert_eq!(agent.roots.len(), 1);
            assert_eq!(agent.roots[0].path, cwd.join(".agents/skills"));
            assert_eq!(agent.roots[0].scope, Scope::Project);
        }
    }

    #[test]
    fn cwd_only_products_do_not_use_repository_root_or_home_as_project() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let repo = home.join("workspace");
        let cwd = repo.join("nested");
        fs::create_dir_all(&cwd).unwrap();
        for marker in [".cline", ".continue", ".goose"] {
            fs::create_dir_all(home.join(marker)).unwrap();
        }
        let agents = detected(&cwd, Some(&repo), &home);
        for name in ["Cline", "Continue CLI", "Goose"] {
            let agent = agents.iter().find(|a| a.name == name).unwrap();
            assert!(
                agent
                    .roots
                    .iter()
                    .filter(|r| r.scope == Scope::Project)
                    .all(|r| r.path.starts_with(&cwd))
            );
        }
        let cline = agents.iter().find(|a| a.name == "Cline").unwrap();
        let expected = [
            home.join(".agents/skills"),
            home.join(".cline/skills"),
            cwd.join(".agents/skills"),
            cwd.join(".claude/skills"),
            cwd.join(".cline/skills"),
            cwd.join(".clinerules/skills"),
        ];
        assert_eq!(
            cline.roots.iter().map(|r| &r.path).collect::<Vec<_>>(),
            expected.iter().collect::<Vec<_>>()
        );
        let home_agents = detected(&home, None, &home);
        for name in ["Cline", "Continue CLI", "Goose"] {
            let agent = home_agents.iter().find(|a| a.name == name).unwrap();
            assert!(agent.roots.iter().all(|r| r.scope == Scope::Global));
        }
    }

    #[test]
    fn ancestor_walk_stops_at_git_root() {
        let home = Path::new("/users/example");
        let repo = home.join("work/repo");
        let cwd = repo.join("packages/app");
        assert_eq!(
            ancestors(&cwd, Some(&repo), home),
            vec![cwd, repo.join("packages"), repo]
        );
    }
}
