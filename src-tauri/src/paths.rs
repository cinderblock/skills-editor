use std::path::{Path, PathBuf};

#[cfg(test)]
thread_local! {
    /// Tests point "home" at a scratch dir. Thread-local, so parallel tests
    /// (and the real ~/.claude) are unaffected.
    pub static TEST_HOME: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// Home directory without relying on the (historically deprecated) std helper.
pub fn home_dir() -> Result<PathBuf, String> {
    #[cfg(test)]
    if let Some(home) = TEST_HOME.with(|h| h.borrow().clone()) {
        return Ok(home);
    }
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

/// A comparable form of a path: verbatim prefix stripped, separators
/// normalized, and case-folded on Windows (whose filesystem is
/// case-insensitive). Use for equality, never for display or I/O.
pub fn path_key(path: &Path) -> String {
    let s = strip_verbatim(path).to_string_lossy().to_string();
    if cfg!(windows) {
        s.replace('/', "\\").trim_end_matches('\\').to_lowercase()
    } else {
        s.trim_end_matches('/').to_string()
    }
}

/// Keep console programs we spawn (git, claude, hook scripts) from popping
/// up a console window — release builds are GUI-subsystem apps on Windows.
pub fn hide_console(cmd: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}
