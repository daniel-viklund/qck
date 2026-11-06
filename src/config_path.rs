use std::path::{Path, PathBuf};

/// Explicit path, then QCK_CONFIG, then config/config.lua above the executable.
/// Never creates or copies a config; the repository file is the source of truth.
pub fn resolve(explicit: Option<&str>) -> Result<PathBuf, String> {
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    select(
        explicit.map(PathBuf::from),
        std::env::var_os("QCK_CONFIG")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from),
        &executable,
        Path::is_file,
    )
}

fn select(
    explicit: Option<PathBuf>,
    environment: Option<PathBuf>,
    executable: &Path,
    is_file: impl Fn(&Path) -> bool,
) -> Result<PathBuf, String> {
    if let Some(path) = explicit.or(environment) {
        return Ok(path);
    }
    for directory in executable.parent().into_iter().flat_map(Path::ancestors) {
        let candidate = directory.join("config/config.lua");
        if is_file(&candidate) {
            return Ok(candidate);
        }
    }
    Err("Cannot find config/config.lua above the executable. Keep Qck inside the repository, set QCK_CONFIG to your repository config's full path, or pass a config path when launching.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_repo_config_from_build_and_artifact_layouts() {
        let root = std::env::temp_dir().join("qck-config-test");
        let expected = root.join("config/config.lua");
        for executable in ["target/release/qck.exe", "target/debug/qck.exe", "qck.exe"] {
            let actual = select(None, None, &root.join(executable), |p| p == expected).unwrap();
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn explicit_and_environment_paths_take_precedence_even_when_missing() {
        let executable = Path::new("qck.exe");
        assert_eq!(
            select(
                Some("explicit.lua".into()),
                Some("env.lua".into()),
                executable,
                |_| true
            )
            .unwrap(),
            Path::new("explicit.lua")
        );
        assert_eq!(
            select(None, Some("env.lua".into()), executable, |_| true).unwrap(),
            Path::new("env.lua")
        );
        assert!(
            select(None, None, executable, |_| false)
                .unwrap_err()
                .contains("QCK_CONFIG")
        );
    }
}
