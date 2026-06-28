//! Small cross-platform helpers.

use std::path::Path;

/// Whether an executable named `name` can be found on `PATH`. Used to detect
/// which agent CLIs are installed without spawning them.
pub fn binary_available(name: &str) -> bool {
    // Absolute / relative path provided directly.
    if name.contains('/') || name.contains('\\') {
        return Path::new(name).is_file();
    }
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".into())
            .split(';')
            .map(|s| s.to_string())
            .collect()
    } else {
        vec![String::new()]
    };
    for dir in std::env::split_paths(&paths) {
        for ext in &exts {
            let candidate = dir.join(format!("{name}{ext}"));
            if candidate.is_file() {
                return true;
            }
        }
    }
    false
}
