use std::path::{Path, PathBuf};

/// Home directory without relying on the (historically deprecated) std helper.
pub fn home_dir() -> Result<PathBuf, String> {
    #[cfg(windows)]
    let var = "USERPROFILE";
    #[cfg(not(windows))]
    let var = "HOME";
    std::env::var(var)
        .map(PathBuf::from)
        .map_err(|_| format!("could not resolve home directory ({var} unset)"))
}

pub fn claude_dir() -> Result<PathBuf, String> {
    Ok(home_dir()?.join(".claude"))
}

/// Strip Windows' `\\?\` verbatim prefix, which confuses external tools
/// (git) and looks terrible in the UI.
pub fn strip_verbatim(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    match s.strip_prefix(r"\\?\UNC\") {
        Some(rest) => PathBuf::from(format!(r"\\{rest}")),
        None => match s.strip_prefix(r"\\?\") {
            Some(rest) => PathBuf::from(rest),
            None => path.to_path_buf(),
        },
    }
}

/// True when `path` (canonicalized) sits inside `root` (canonicalized).
pub fn is_within(path: &Path, root: &Path) -> bool {
    match (path.canonicalize(), root.canonicalize()) {
        (Ok(p), Ok(r)) => p.starts_with(&r),
        _ => false,
    }
}
