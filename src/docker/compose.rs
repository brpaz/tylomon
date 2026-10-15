use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Compose file names searched in the current directory, in Compose's own precedence order.
const DEFAULT_COMPOSE_FILES: [&str; 4] = [
    "compose.yaml",
    "compose.yml",
    "docker-compose.yaml",
    "docker-compose.yml",
];

/// A Compose project found on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposeProject {
    pub name: String,
    pub files: Vec<PathBuf>,
}

/// Environment inputs that influence Compose project resolution.
#[derive(Debug, Clone, Default)]
pub struct ComposeEnv {
    pub compose_file: Option<String>,
    pub path_separator: Option<String>,
    pub project_name: Option<String>,
}

impl ComposeEnv {
    pub fn from_process() -> Self {
        let get = |key| env::var(key).ok().filter(|v| !v.is_empty());
        Self {
            compose_file: get("COMPOSE_FILE"),
            path_separator: get("COMPOSE_PATH_SEPARATOR"),
            project_name: get("COMPOSE_PROJECT_NAME"),
        }
    }
}

/// Find the Compose project rooted at `dir`.
///
/// `COMPOSE_FILE` (a `:`-separated list, or `COMPOSE_PATH_SEPARATOR`) takes precedence over the
/// default file names. The project name is `COMPOSE_PROJECT_NAME`, else the top-level `name:` of
/// the last file declaring one, else the directory of the first file.
pub fn find_compose_project(dir: &Path, env: &ComposeEnv) -> Option<ComposeProject> {
    let files = compose_files(dir, env);
    if files.is_empty() {
        return None;
    }

    let name = env
        .project_name
        .as_deref()
        .map(normalize_project_name)
        .or_else(|| {
            files
                .iter()
                .rev()
                .filter_map(|f| fs::read_to_string(f).ok())
                .find_map(|content| declared_name(&content))
                .map(|n| normalize_project_name(&n))
        })
        .or_else(|| directory_name(&files[0]))
        .filter(|n| !n.is_empty())?;

    Some(ComposeProject { name, files })
}

fn compose_files(dir: &Path, env: &ComposeEnv) -> Vec<PathBuf> {
    if let Some(list) = &env.compose_file {
        let sep = env.path_separator.as_deref().unwrap_or(":");
        return list
            .split(sep)
            .filter(|p| !p.is_empty())
            .map(|p| dir.join(p))
            .filter(|p| p.is_file())
            .collect();
    }

    DEFAULT_COMPOSE_FILES
        .iter()
        .map(|name| dir.join(name))
        .find(|p| p.is_file())
        .into_iter()
        .collect()
}

/// Extract the top-level `name:` value.
fn declared_name(content: &str) -> Option<String> {
    let doc: serde_yaml_ng::Value = serde_yaml_ng::from_str(content).ok()?;
    let name = doc.get("name")?.as_str()?.trim();
    (!name.is_empty()).then(|| name.to_string())
}

fn directory_name(file: &Path) -> Option<String> {
    let abs = fs::canonicalize(file).ok()?;
    let name = abs.parent()?.file_name()?.to_str()?;
    Some(normalize_project_name(name))
}

/// Mirror Compose's project name normalisation: lowercase, keep `[a-z0-9_-]`.
fn normalize_project_name(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '-'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(tag: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("tylomon-compose-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("should create temp dir");
        dir
    }

    #[test]
    fn no_compose_file_yields_none() {
        let dir = tempdir("none");
        assert_eq!(find_compose_project(&dir, &ComposeEnv::default()), None);
    }

    #[test]
    fn name_defaults_to_directory() {
        let dir = tempdir("My.App");
        fs::write(dir.join("docker-compose.yml"), "services: {}\n").expect("should write");
        let project = find_compose_project(&dir, &ComposeEnv::default()).expect("should find");
        assert!(project.name.starts_with("tylomon-compose-myapp"));
        assert_eq!(project.files, vec![dir.join("docker-compose.yml")]);
    }

    #[test]
    fn declared_name_wins_over_directory() {
        let dir = tempdir("declared");
        fs::write(
            dir.join("compose.yml"),
            "name: \"Shop-Demo\"\nservices: {}\n",
        )
        .expect("should write");
        let project = find_compose_project(&dir, &ComposeEnv::default()).expect("should find");
        assert_eq!(project.name, "shop-demo");
    }

    #[test]
    fn compose_yaml_takes_precedence_over_docker_compose() {
        let dir = tempdir("precedence");
        fs::write(dir.join("compose.yaml"), "name: a\n").expect("should write");
        fs::write(dir.join("docker-compose.yml"), "name: b\n").expect("should write");
        let project = find_compose_project(&dir, &ComposeEnv::default()).expect("should find");
        assert_eq!(project.name, "a");
    }

    #[test]
    fn compose_file_env_overrides_defaults() {
        let dir = tempdir("envfile");
        fs::write(dir.join("compose.yml"), "name: default\n").expect("should write");
        fs::write(dir.join("base.yml"), "name: base\n").expect("should write");
        fs::write(dir.join("override.yml"), "services: {}\n").expect("should write");
        let env = ComposeEnv {
            compose_file: Some("base.yml:override.yml:missing.yml".into()),
            ..Default::default()
        };
        let project = find_compose_project(&dir, &env).expect("should find");
        assert_eq!(project.name, "base");
        assert_eq!(project.files.len(), 2);
    }

    #[test]
    fn custom_path_separator() {
        let dir = tempdir("sep");
        fs::write(dir.join("a.yml"), "name: first\n").expect("should write");
        fs::write(dir.join("b.yml"), "services: {}\n").expect("should write");
        let env = ComposeEnv {
            compose_file: Some("a.yml;b.yml".into()),
            path_separator: Some(";".into()),
            ..Default::default()
        };
        let project = find_compose_project(&dir, &env).expect("should find");
        assert_eq!(project.files.len(), 2);
    }

    #[test]
    fn project_name_env_wins() {
        let dir = tempdir("pname");
        fs::write(dir.join("compose.yml"), "name: file\n").expect("should write");
        let env = ComposeEnv {
            project_name: Some("Override".into()),
            ..Default::default()
        };
        let project = find_compose_project(&dir, &env).expect("should find");
        assert_eq!(project.name, "override");
    }

    #[test]
    fn declared_name_ignores_nested_and_comments() {
        assert_eq!(declared_name("services:\n  web:\n    name: x\n"), None);
        assert_eq!(declared_name("name: demo # note\n"), Some("demo".into()));
    }

    #[test]
    fn declared_name_handles_anchors_and_quotes() {
        let content = "x-common: &common\n  restart: always\nname: 'my-app'\nservices:\n  web:\n    <<: *common\n";
        assert_eq!(declared_name(content), Some("my-app".into()));
    }

    #[test]
    fn declared_name_invalid_yaml_is_none() {
        assert_eq!(declared_name("name: [unclosed"), None);
    }
}
