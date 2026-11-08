use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

#[cfg(windows)]
mod discovery;

/// Shared with the startup discovery thread; hotkeys only read the index.
#[derive(Clone, Default)]
pub struct AppResolver {
    paths: Arc<RwLock<HashMap<String, Vec<PathBuf>>>>,
    searching: Arc<AtomicBool>,
}

impl AppResolver {
    #[cfg(windows)]
    pub fn start() -> Self {
        let resolver = Self::default();
        resolver.searching.store(true, Ordering::Release);
        let worker = resolver.clone();
        if let Err(error) = std::thread::Builder::new()
            .name("qck-app-discovery".into())
            .spawn(move || {
                eprintln!("qck: discovering application paths in the background");
                discovery::scan(|path| worker.record(path));
                worker.searching.store(false, Ordering::Release);
                eprintln!(
                    "qck: app discovery complete ({} executable names)",
                    worker.paths.read().unwrap().len()
                );
            })
        {
            resolver.searching.store(false, Ordering::Release);
            eprintln!("qck: cannot start app discovery: {error}");
        }
        resolver
    }

    fn record(&self, path: PathBuf) {
        if !path.is_absolute() || !path.is_file() {
            return;
        }
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            return;
        };
        if !is_executable_name(name) {
            return;
        }
        let mut index = self.paths.write().unwrap();
        let paths = index.entry(name.to_lowercase()).or_default();
        if !paths.contains(&path) {
            paths.push(path);
        }
    }

    pub fn launch(
        &self,
        target: &str,
        mut shell_execute: impl FnMut(&Path) -> isize,
    ) -> Result<(), String> {
        // Preserve Shell resolution (including PATH and registered App Paths).
        let result = shell_execute(Path::new(target));
        if result > 32 {
            return Ok(());
        }
        // Explicit paths select a particular install. Never replace them, and
        // never turn access denied or an invalid executable into another launch.
        if !matches!(result, 2 | 3) || !is_executable_name(target) {
            return Err(launch_error(Path::new(target), result));
        }
        let candidates = self
            .paths
            .read()
            .unwrap()
            .get(&target.to_lowercase())
            .cloned()
            .unwrap_or_default();
        for path in candidates {
            if !path.is_file() {
                continue;
            }
            let result = shell_execute(&path);
            if result > 32 {
                eprintln!("qck: launched {target} from {}", path.display());
                return Ok(());
            }
            if !matches!(result, 2 | 3) {
                return Err(launch_error(&path, result));
            }
        }
        let mut error = launch_error(Path::new(target), result);
        if self.searching.load(Ordering::Acquire) {
            error.push_str(". App discovery is still running; retry shortly");
        }
        Err(error)
    }
}

fn is_executable_name(name: &str) -> bool {
    !name.contains(['\\', '/', ':'])
        && name.len() > 4
        && name.to_ascii_lowercase().ends_with(".exe")
}

fn launch_error(target: &Path, result: isize) -> String {
    format!(
        "Cannot launch {} (ShellExecute {result}); if discovery cannot find it, use its full executable path in config/config.lua",
        target.display()
    )
}

/// Scan once per startup, without following symlinks or Windows junctions.
/// A shared visited set avoids rescanning install folders during the drive pass.
fn walk(
    roots: impl IntoIterator<Item = PathBuf>,
    visited: &mut HashSet<PathBuf>,
    mut visit_file: impl FnMut(PathBuf),
) {
    for root in roots {
        if root.ancestors().any(|p| visited.contains(&path_key(p))) {
            continue;
        }
        let mut pending = vec![root.clone()];
        while let Some(directory) = pending.pop() {
            let Ok(metadata) = std::fs::symlink_metadata(&directory) else {
                continue;
            };
            if !metadata.is_dir() || is_link(&metadata) {
                continue;
            }
            if visited.contains(&path_key(&directory)) {
                continue;
            }
            let Ok(entries) = std::fs::read_dir(&directory) else {
                continue;
            };
            let mut children = Vec::new();
            for entry in entries.flatten() {
                let Ok(metadata) = entry.metadata() else {
                    continue;
                };
                if is_link(&metadata) {
                    continue;
                }
                if metadata.is_dir() {
                    children.push(entry.path());
                } else if metadata.is_file() {
                    visit_file(entry.path());
                }
            }
            // Stable order makes multiple installs predictable within a source.
            children.sort();
            pending.extend(children.into_iter().rev());
        }
        // Remember completed roots, rather than every directory on the computer.
        // Junctions are skipped, so there are no directory cycles within a root.
        visited.insert(path_key(&root));
    }
}

fn path_key(path: &Path) -> PathBuf {
    PathBuf::from(path.to_string_lossy().to_lowercase())
}

fn is_link(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return true; // FILE_ATTRIBUTE_REPARSE_POINT, including junctions.
        }
    }
    metadata.file_type().is_symlink()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    struct Installation(PathBuf);
    impl Installation {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "qck-launch-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&root).unwrap();
            Self(root)
        }

        fn executable(&self, relative: &str) -> PathBuf {
            let path = self.0.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, []).unwrap();
            path
        }
    }
    impl Drop for Installation {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn startup_search_finds_nested_custom_install_for_case_insensitive_cold_launch() {
        let installation = Installation::new();
        let executable = installation.executable("Custom Apps/Passwords/v1/Bitwarden.exe");
        installation.executable("Other/notes.txt");
        let resolver = AppResolver::default();
        let mut visited = HashSet::new();
        walk(
            [installation.0.join("missing"), installation.0.clone()],
            &mut visited,
            |path| resolver.record(path),
        );
        let mut attempts = Vec::new();
        resolver
            .launch("bitwarden.exe", |path| {
                attempts.push(path.to_path_buf());
                if path == executable { 33 } else { 2 }
            })
            .unwrap();
        assert_eq!(attempts, [PathBuf::from("bitwarden.exe"), executable]);
        assert_eq!(resolver.paths.read().unwrap().len(), 1);
    }

    #[test]
    fn search_preserves_priority_deduplicates_and_skips_deleted_installs() {
        let installation = Installation::new();
        let registered = installation.executable("Preferred/Bitwarden.exe");
        let portable = installation.executable("Portable/Bitwarden.exe");
        let resolver = AppResolver::default();
        resolver.record(registered.clone());
        let mut visited = HashSet::new();
        for _ in 0..2 {
            walk([installation.0.clone()], &mut visited, |p| {
                resolver.record(p)
            });
        }
        assert_eq!(resolver.paths.read().unwrap()["bitwarden.exe"].len(), 2);
        let mut selected = None;
        resolver
            .launch("Bitwarden.exe", |path| {
                if path.is_absolute() {
                    selected = Some(path.to_path_buf());
                    33
                } else {
                    2
                }
            })
            .unwrap();
        assert_eq!(selected, Some(registered.clone()));
        std::fs::remove_file(registered).unwrap();
        resolver
            .launch(
                "Bitwarden.exe",
                |path| if path == portable { 33 } else { 2 },
            )
            .unwrap();
    }

    #[test]
    fn successful_shell_resolution_errors_and_explicit_paths_do_not_use_index() {
        let installation = Installation::new();
        let resolver = AppResolver::default();
        resolver.record(installation.executable("Bitwarden.exe"));
        for (target, result) in [
            ("Bitwarden.exe", 33),
            ("Bitwarden.exe", 5),
            (r"C:\Custom\Bitwarden.exe", 2),
            ("C:Bitwarden.exe", 2),
            ("Other.exe", 2),
        ] {
            let mut attempts = 0;
            let outcome = resolver.launch(target, |_| {
                attempts += 1;
                result
            });
            assert_eq!(outcome.is_ok(), result > 32);
            assert_eq!(attempts, 1);
        }
    }

    #[test]
    fn reports_discovery_progress_and_installed_executable_failure() {
        let resolver = AppResolver::default();
        resolver.searching.store(true, Ordering::Release);
        assert!(
            resolver
                .launch("Bitwarden.exe", |_| 2)
                .unwrap_err()
                .contains("still running")
        );
        resolver.searching.store(false, Ordering::Release);
        assert!(
            !resolver
                .launch("Bitwarden.exe", |_| 2)
                .unwrap_err()
                .contains("still running")
        );
        let installation = Installation::new();
        resolver.record(installation.executable("Bitwarden.exe"));
        let error = resolver
            .launch(
                "Bitwarden.exe",
                |path| if path.is_absolute() { 5 } else { 2 },
            )
            .unwrap_err();
        assert!(error.contains("ShellExecute 5"));
        assert!(error.contains(&installation.0.display().to_string()));
    }

    #[test]
    fn discovery_can_publish_paths_while_launches_read_the_index() {
        let installation = Installation::new();
        let executable = installation.executable("Bitwarden.exe");
        let resolver = AppResolver::default();
        let publisher = resolver.clone();
        std::thread::spawn(move || publisher.record(executable))
            .join()
            .unwrap();
        resolver
            .launch("Bitwarden.exe", |p| if p.is_absolute() { 33 } else { 2 })
            .unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn search_does_not_follow_directory_symlinks_or_revisit_roots() {
        let installation = Installation::new();
        installation.executable("App/Bitwarden.exe");
        std::os::unix::fs::symlink(&installation.0, installation.0.join("App/loop")).unwrap();
        let mut visited = HashSet::new();
        let mut files = Vec::new();
        walk(
            [installation.0.clone(), installation.0.join("App")],
            &mut visited,
            |p| files.push(p),
        );
        assert_eq!(files.len(), 1);
        assert_eq!(visited.len(), 1);
    }

    #[test]
    fn installed_subfolder_is_not_rescanned_during_parent_drive_scan() {
        let installation = Installation::new();
        let installed = installation.executable("Apps/Bitwarden.exe");
        let portable = installation.executable("Portable/Other.exe");
        let mut files = Vec::new();
        walk(
            [
                installation.0.join("Apps"),
                installation.0.clone(),
                installation.0.join("Apps"),
            ],
            &mut HashSet::new(),
            |p| files.push(p),
        );
        assert_eq!(files, [installed, portable]);
    }
}
