use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum JailError {
    #[error("Path traversal detected: {0} escapes repository root {1}")]
    EscapedRoot(String, String),
    #[error("Target path contains illegal components ('..' or root navigation): {0}")]
    IllegalComponent(String),
    #[error("IO error canonicalizing path {0}: {1}")]
    IoError(String, std::io::Error),
    #[error("Path does not exist: {0}")]
    NotFound(String),
}

/// Canonicalizes a path and verifies that it strictly resides beneath `repo_root`.
/// Mitigates directory traversal (`../../etc/passwd`) and malicious alias redirection.
pub fn jail_path(repo_root: &Path, target: &Path) -> Result<PathBuf, JailError> {
    // 1. Lexical validation
    let target_str = target.to_string_lossy();
    if target_str.contains("..") {
        return Err(JailError::IllegalComponent(target_str.to_string()));
    }

    // 2. Canonicalize repo root
    let canonical_root = dunce::canonicalize(repo_root)
        .map_err(|e| JailError::IoError(repo_root.display().to_string(), e))?;

    // 3. Resolve target relative to repo root if not absolute
    let candidate = if target.is_absolute() {
        target.to_path_buf()
    } else {
        canonical_root.join(target)
    };

    // 4. Canonicalize target
    let canonical_target = dunce::canonicalize(&candidate)
        .map_err(|e| JailError::IoError(candidate.display().to_string(), e))?;

    // 5. Strict prefix check
    if !canonical_target.starts_with(&canonical_root) {
        return Err(JailError::EscapedRoot(
            canonical_target.display().to_string(),
            canonical_root.display().to_string(),
        ));
    }

    Ok(canonical_target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_jail_path_valid() {
        let temp_dir = std::env::temp_dir().join("cg_test_jail_valid");
        fs::create_dir_all(&temp_dir).unwrap();
        let sub_file = temp_dir.join("test.txt");
        fs::write(&sub_file, "hello").unwrap();

        let result = jail_path(&temp_dir, Path::new("test.txt"));
        assert!(result.is_ok());

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_jail_path_traversal_blocked() {
        let temp_dir = std::env::temp_dir().join("cg_test_jail_traversal");
        fs::create_dir_all(&temp_dir).unwrap();

        let result = jail_path(&temp_dir, Path::new("../escaped.txt"));
        assert!(matches!(result, Err(JailError::IllegalComponent(_))));

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
