use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug)]
pub struct SourceIdentity {
    pub revision: Option<String>,
    pub state: &'static str,
    pub git_paths: Vec<PathBuf>,
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .current_dir(root)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn source_identity(root: &Path) -> SourceIdentity {
    let unknown = || SourceIdentity {
        revision: None,
        state: "unknown",
        git_paths: Vec::new(),
    };
    let Some(top) = git(root, &["rev-parse", "--show-toplevel"]) else {
        return unknown();
    };
    if root.canonicalize().ok() != Path::new(&top).canonicalize().ok() {
        return unknown();
    }
    let Some(revision) = git(root, &["rev-parse", "HEAD"]) else {
        return unknown();
    };
    if !matches!(revision.len(), 40 | 64) || !revision.bytes().all(|b| b.is_ascii_hexdigit()) {
        return unknown();
    }
    let Some(status) = git(root, &["status", "--porcelain", "--untracked-files=no"]) else {
        return unknown();
    };
    let mut git_paths = Vec::new();
    for name in ["HEAD", "index", "packed-refs", "refs"] {
        if let Some(path) = git(root, &["rev-parse", "--git-path", name]) {
            let path = PathBuf::from(path);
            git_paths.push(if path.is_absolute() {
                path
            } else {
                root.join(path)
            });
        }
    }
    if let Some(paths) = git(root, &["ls-files", "-z"]) {
        git_paths.extend(
            paths
                .split('\0')
                .filter(|path| !path.is_empty())
                .map(|path| root.join(path)),
        );
    }
    SourceIdentity {
        revision: Some(revision),
        state: if status.is_empty() { "clean" } else { "dirty" },
        git_paths,
    }
}
