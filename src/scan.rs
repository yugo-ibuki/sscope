use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

use serde_yaml_ng::Value;

use crate::{Report, Scope, Skill};

pub struct Root {
    pub path: PathBuf,
    pub scope: Scope,
    /// Maximum directory depth below this root; 1 includes direct skill children.
    pub depth: usize,
}

#[derive(Clone, Copy)]
pub enum Collision {
    First,
    Keep,
}

/// Inspect only the supplied skill roots, in descending precedence order.
pub fn scan(roots: &[Root], policy: Collision) -> Report {
    scan_with_format(roots, policy, false)
}

/// Claude allows omitted metadata and plain Markdown skill definitions.
pub fn scan_claude(roots: &[Root], policy: Collision) -> Report {
    scan_with_format(roots, policy, true)
}

fn scan_with_format(roots: &[Root], policy: Collision, claude: bool) -> Report {
    let mut scanner = Scanner {
        policy,
        claude,
        definitions: HashSet::new(),
        names: HashSet::new(),
        report: Report::default(),
    };
    for root in roots {
        // Alias roots may expose deeper definitions with a larger depth limit.
        let mut visited = HashMap::new();
        scanner.visit(&root.path, root.depth, root.scope, &mut visited, false);
    }
    scanner.report
}

struct Scanner {
    policy: Collision,
    claude: bool,
    definitions: HashSet<(PathBuf, String)>,
    names: HashSet<String>,
    report: Report,
}

impl Scanner {
    fn visit(
        &mut self,
        directory: &Path,
        remaining: usize,
        scope: Scope,
        visited: &mut HashMap<PathBuf, usize>,
        inspect: bool,
    ) {
        let physical = match fs::canonicalize(directory) {
            Ok(path) => path,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return,
            Err(error) => {
                diagnostic(&mut self.report, directory, error);
                return;
            }
        };
        if visited
            .get(&physical)
            .is_some_and(|depth| *depth >= remaining)
        {
            return;
        }
        visited.insert(physical, remaining);
        if inspect {
            let skill_path = directory.join("SKILL.md");
            match read_definition(&skill_path) {
                Ok(Some(text)) => {
                    let fallback = if self.claude {
                        directory.file_name().and_then(|name| name.to_str())
                    } else {
                        None
                    };
                    match metadata_with_fallback(&text, fallback) {
                        Ok((name, note)) => match fs::canonicalize(&skill_path) {
                            Ok(path) => {
                                if self.definitions.insert((path, name.clone()))
                                    && (matches!(self.policy, Collision::Keep)
                                        || !self.names.contains(&name))
                                {
                                    self.names.insert(name.clone());
                                    self.report.skills.push(Skill {
                                        name,
                                        path: skill_path,
                                        scope,
                                        note,
                                    });
                                }
                            }
                            Err(error) => diagnostic(&mut self.report, &skill_path, error),
                        },
                        Err(error) => diagnostic(&mut self.report, &skill_path, error),
                    }
                    // Never search a skill's scripts or reference resources.
                    return;
                }
                Ok(None) => {}
                Err(error) => {
                    diagnostic(&mut self.report, &skill_path, error);
                    return;
                }
            }
        }
        if remaining == 0 {
            return;
        }
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) => {
                diagnostic(&mut self.report, directory, error);
                return;
            }
        };
        let mut children = Vec::new();
        for entry in entries {
            match entry {
                Ok(entry) => {
                    if entry.file_name().to_string_lossy().starts_with('.') {
                        continue;
                    }
                    match fs::metadata(entry.path()) {
                        Ok(metadata) if metadata.is_dir() => children.push(entry.path()),
                        Ok(_) => {}
                        Err(error) => diagnostic(&mut self.report, &entry.path(), error),
                    }
                }
                Err(error) => diagnostic(&mut self.report, directory, error),
            }
        }
        children.sort();
        for child in children {
            self.visit(&child, remaining - 1, scope, visited, true);
        }
    }
}

const MAX_HEADER_BYTES: u64 = 1024 * 1024;

fn read_definition(path: &Path) -> io::Result<Option<String>> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "SKILL.md must be a regular file",
        ));
    }
    let file = fs::File::open(path)?;
    let mut reader = BufReader::new(file.take(MAX_HEADER_BYTES + 1));
    let mut text = String::new();
    reader.read_line(&mut text)?;
    let frontmatter = text
        .trim_start_matches('\u{feff}')
        .trim_end_matches(['\r', '\n'])
        == "---";
    loop {
        if text.len() as u64 > MAX_HEADER_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "skill header exceeds 1 MiB",
            ));
        }
        // For plain Markdown only the first nonempty line is needed.
        if !frontmatter && !text.trim().is_empty() {
            break;
        }
        let start = text.len();
        if reader.read_line(&mut text)? == 0 {
            break;
        }
        if frontmatter && text[start..].trim_end_matches(['\r', '\n']) == "---" {
            break;
        }
    }
    if text.len() as u64 > MAX_HEADER_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "skill header exceeds 1 MiB",
        ));
    }
    Ok(Some(text))
}

fn diagnostic(report: &mut Report, path: &Path, error: impl std::fmt::Display) {
    report.warnings.push(format!("{}: {error}", path.display()));
}

#[cfg(test)]
fn metadata(text: &str) -> Result<(String, String), String> {
    metadata_with_fallback(text, None)
}

fn metadata_with_fallback(
    text: &str,
    fallback_name: Option<&str>,
) -> Result<(String, String), String> {
    let mut lines = text.trim_start_matches('\u{feff}').lines();
    if lines.next() != Some("---") {
        if let Some(name) = fallback_name
            && !text.trim().is_empty()
        {
            return Ok((name.to_owned(), String::new()));
        }
        return Err("missing YAML frontmatter".into());
    }
    let mut yaml = String::new();
    let mut closed = false;
    for line in lines {
        if line == "---" {
            closed = true;
            break;
        }
        yaml.push_str(line);
        yaml.push('\n');
    }
    if !closed {
        return Err("unterminated YAML frontmatter".into());
    }
    let mut value: Value = serde_yaml_ng::from_str(&yaml)
        .map_err(|error| format!("invalid YAML frontmatter: {error}"))?;
    if value.is_null() && fallback_name.is_some() {
        value = Value::Mapping(Default::default());
    }
    let fields = value
        .as_mapping()
        .ok_or("YAML frontmatter must be a mapping")?;
    let field = |key: &str| fields.get(Value::String(key.into()));
    let required = |key: &str| -> Result<String, String> {
        field(key)
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned)
            .ok_or_else(|| format!("frontmatter {key} must be a nonempty string"))
    };
    let name = if field("name").is_none() {
        fallback_name
            .map(str::to_owned)
            .map(Ok)
            .unwrap_or_else(|| required("name"))?
    } else {
        required("name")?
    };
    if fallback_name.is_none() || field("description").is_some() {
        required("description")?;
    }
    let mut notes = Vec::new();
    // These fields are product-specific declarations, not universal enablement
    // controls. Report them without evaluating a different agent's semantics.
    for key in ["paths", "disable-model-invocation", "user-invocable"] {
        if let Some(value) = field(key) {
            let rendered = serde_yaml_ng::to_string(value)
                .map_err(|error| format!("cannot render {key}: {error}"))?;
            notes.push(format!("{key}={}", rendered.trim().replace('\n', " ")));
        }
    }
    let note = if notes.is_empty() {
        String::new()
    } else {
        format!("呼び出し条件: {}", notes.join("; "))
    };
    Ok((name, note))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn skill(parent: &Path, directory: &str, name: &str) -> PathBuf {
        let path = parent.join(directory);
        fs::create_dir_all(&path).unwrap();
        fs::write(
            path.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: Test skill\n---\nBody"),
        )
        .unwrap();
        path
    }

    fn root(path: &Path, scope: Scope, depth: usize) -> Root {
        Root {
            path: path.into(),
            scope,
            depth,
        }
    }

    #[test]
    fn root_order_controls_collisions_and_keep_preserves_distinct_definitions() {
        let temp = TempDir::new().unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        let chosen = skill(&first, "review", "review");
        skill(&second, "review", "review");
        let roots = [
            root(&first, Scope::Global, 1),
            root(&second, Scope::Project, 1),
        ];
        let selected = scan(&roots, Collision::First);
        assert_eq!(selected.skills.len(), 1);
        assert_eq!(selected.skills[0].path, chosen.join("SKILL.md"));
        assert!(matches!(selected.skills[0].scope, Scope::Global));
        assert_eq!(scan(&roots, Collision::Keep).skills.len(), 2);
    }

    #[test]
    fn recursion_is_bounded_and_ignores_hidden_folders_and_skill_resources() {
        let temp = TempDir::new().unwrap();
        let parent = skill(temp.path(), "parent", "parent");
        skill(&parent, "scripts/embedded", "embedded");
        skill(temp.path(), "category/child", "child");
        skill(temp.path(), ".hidden", "hidden");
        assert_eq!(
            scan(&[root(temp.path(), Scope::Project, 1)], Collision::Keep)
                .skills
                .len(),
            1
        );
        let report = scan(&[root(temp.path(), Scope::Project, 2)], Collision::Keep);
        assert_eq!(report.skills.len(), 2);
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn yaml_supports_crlf_multiline_and_reports_invalid_fields() {
        let (name, note) = metadata("---\r\nname: 'review'\r\ndescription: |\r\n  Multiple lines\r\n  are accepted\r\npaths: [src/**]\r\ndisable-model-invocation: true\r\nuser-invocable: false\r\nenabled: false\r\n---\r\nBody").unwrap();
        assert_eq!(name, "review");
        assert!(note.contains("呼び出し条件:"));
        assert!(note.contains("disable-model-invocation=true"));
        assert!(!note.contains("enabled"));
        for text in [
            "name: test",
            "---\nname: test\ndescription: test",
            "---\nname: test\ndescription: [bad]\n---",
            "---\nname: ''\ndescription: test\n---",
            "---\nname: test\ndescription: ' '\n---",
            "---\nname: [\ndescription: test\n---",
        ] {
            assert!(metadata(text).is_err(), "accepted {text:?}");
        }
    }

    #[test]
    fn missing_roots_are_benign_and_invalid_skills_are_diagnostic() {
        let temp = TempDir::new().unwrap();
        let bad = skill(temp.path(), "bad", "bad");
        fs::write(bad.join("SKILL.md"), "---\nname: bad\n---").unwrap();
        let roots = [
            root(temp.path(), Scope::Project, 1),
            root(&temp.path().join("absent"), Scope::Global, 1),
        ];
        let report = scan(&roots, Collision::Keep);
        assert!(report.skills.is_empty());
        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].contains("description"));
    }

    #[test]
    fn claude_accepts_optional_frontmatter_without_weakening_standard_format() {
        let temp = TempDir::new().unwrap();
        let plain = skill(temp.path(), "plain", "plain");
        fs::write(
            plain.join("SKILL.md"),
            "Explain this code.\nMore instructions.",
        )
        .unwrap();
        let optional = skill(temp.path(), "optional", "optional");
        fs::write(
            optional.join("SKILL.md"),
            "---\nuser-invocable: false\n---\nExplain code.",
        )
        .unwrap();
        let roots = [root(temp.path(), Scope::Project, 1)];
        let claude = scan_claude(&roots, Collision::First);
        assert_eq!(claude.skills.len(), 2);
        assert_eq!(claude.skills[0].name, "optional");
        assert_eq!(claude.skills[1].name, "plain");
        assert!(claude.warnings.is_empty());
        let standard = scan(&roots, Collision::First);
        assert!(standard.skills.is_empty());
        assert_eq!(standard.warnings.len(), 2);
    }

    #[test]
    fn explicitly_supplied_hidden_roots_are_scanned() {
        let temp = TempDir::new().unwrap();
        let hidden = temp.path().join(".skills");
        skill(&hidden, "review", "review");
        let report = scan(&[root(&hidden, Scope::Global, 1)], Collision::First);
        assert_eq!(report.skills.len(), 1);
    }

    #[test]
    fn storage_root_is_not_itself_a_skill_and_body_is_not_read() {
        let temp = TempDir::new().unwrap();
        fs::write(
            temp.path().join("SKILL.md"),
            "---\nname: storage\ndescription: ignored\n---",
        )
        .unwrap();
        let child = skill(temp.path(), "review", "review");
        let mut definition = b"---\nname: review\ndescription: review\n---\n".to_vec();
        definition.extend_from_slice(&[0xff; 16]);
        fs::write(child.join("SKILL.md"), definition).unwrap();
        let report = scan(&[root(temp.path(), Scope::Project, 1)], Collision::Keep);
        assert_eq!(report.skills.len(), 1);
        assert_eq!(report.skills[0].name, "review");
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn oversized_headers_are_rejected() {
        let temp = TempDir::new().unwrap();
        let child = skill(temp.path(), "review", "review");
        fs::write(
            child.join("SKILL.md"),
            format!("---\n{}", "x".repeat(MAX_HEADER_BYTES as usize)),
        )
        .unwrap();
        let report = scan(&[root(temp.path(), Scope::Project, 1)], Collision::Keep);
        assert!(report.skills.is_empty());
        assert!(report.warnings[0].contains("exceeds 1 MiB"));
    }

    #[cfg(unix)]
    #[test]
    fn non_regular_skill_files_are_rejected_without_opening() {
        use std::os::unix::net::UnixListener;
        let temp = TempDir::new().unwrap();
        let child = temp.path().join("socket");
        fs::create_dir(&child).unwrap();
        let _socket = UnixListener::bind(child.join("SKILL.md")).unwrap();
        let report = scan(&[root(temp.path(), Scope::Project, 1)], Collision::Keep);
        assert!(report.skills.is_empty());
        assert!(report.warnings[0].contains("regular file"));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_aliases_and_cycles_do_not_duplicate_or_loop() {
        use std::os::unix::fs::symlink;
        let temp = TempDir::new().unwrap();
        let shared = temp.path().join("shared");
        skill(&shared, "review", "review");
        let alias = temp.path().join("alias");
        symlink(&shared, &alias).unwrap();
        symlink(&shared, shared.join("cycle")).unwrap();
        let roots = [
            root(&alias, Scope::Project, 5),
            root(&shared, Scope::Global, 5),
        ];
        let report = scan(&roots, Collision::Keep);
        assert_eq!(report.skills.len(), 1);
        assert!(report.skills[0].path.starts_with(&alias));
    }

    #[cfg(unix)]
    #[test]
    fn deeper_alias_first_does_not_hide_a_shallower_path() {
        use std::os::unix::fs::symlink;
        let temp = TempDir::new().unwrap();
        let shared = temp.path().join("z-shared");
        skill(&shared, "nested/child", "child");
        let category = temp.path().join("a-category");
        fs::create_dir(&category).unwrap();
        symlink(&shared, category.join("alias")).unwrap();
        let report = scan(&[root(temp.path(), Scope::Project, 3)], Collision::Keep);
        assert_eq!(report.skills.len(), 1);
    }
}
