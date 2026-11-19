use rand::RngCore;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AuthError {
    #[error("Failed to verify peer credentials: {0}")]
    PeerCheckFailed(String),
    #[error("Permission denied: Caller UID {0} does not match daemon UID {1}")]
    UidMismatch(u32, u32),
    #[error("Invalid or missing handshake token")]
    InvalidToken,
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Generates a cryptographically secure 256-bit hex token for daemon authentication
pub fn generate_handshake_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let mut s = String::with_capacity(64);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

/// Computes an isolated, per-user secure runtime directory for Unix domain sockets (mode 0700)
pub fn get_secure_runtime_dir() -> Result<PathBuf, std::io::Error> {
    let dir = if let Ok(xdg) = std::env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(xdg).join("codegraph")
    } else if cfg!(target_os = "macos") {
        if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home)
                .join("Library")
                .join("Caches")
                .join("com.codegraph")
                .join("run")
        } else {
            std::env::temp_dir().join(format!("codegraph-{}", unsafe { libc::getuid() }))
        }
    } else {
        if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home)
                .join(".local")
                .join("state")
                .join("codegraph")
                .join("run")
        } else {
            std::env::temp_dir().join(format!("codegraph-{}", unsafe { libc::getuid() }))
        }
    };

    fs::create_dir_all(&dir)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&dir)?.permissions();
        perms.set_mode(0o700);
        fs::set_permissions(&dir, perms)?;
    }

    Ok(dir)
}

/// Computes a unique socket path for a given repository root
pub fn get_socket_path_for_repo(repo_root: &Path) -> Result<PathBuf, std::io::Error> {
    let runtime_dir = get_secure_runtime_dir()?;
    let canonical = dunce::canonicalize(repo_root).unwrap_or_else(|_| repo_root.to_path_buf());
    
    // Hash the repo path
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    canonical.to_string_lossy().hash(&mut hasher);
    let hash = hasher.finish();

    Ok(runtime_dir.join(format!("cg-{:016x}.sock", hash)))
}

/// Validates peer credentials on Unix domain sockets
#[cfg(unix)]
pub fn verify_peer_uid(stream: &std::os::unix::net::UnixStream) -> Result<(), AuthError> {
    use std::os::unix::io::AsRawFd;

    #[cfg(target_os = "macos")]
    {
        let mut uid = 0u32;
        let mut gid = 0u32;
        let fd = stream.as_raw_fd();
        let ret = unsafe { libc::getpeereid(fd, &mut uid, &mut gid) };
        if ret != 0 {
            return Err(AuthError::PeerCheckFailed(
                std::io::Error::last_os_error().to_string(),
            ));
        }
        let my_uid = unsafe { libc::getuid() };
        if uid != my_uid {
            return Err(AuthError::UidMismatch(uid, my_uid));
        }
        Ok(())
    }

    #[cfg(target_os = "linux")]
    {
        let fd = stream.as_raw_fd();
        let mut ucred = libc::ucred {
            pid: 0,
            uid: 0,
            gid: 0,
        };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        let ret = unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                &mut ucred as *mut _ as *mut libc::c_void,
                &mut len,
            )
        };
        if ret != 0 {
            return Err(AuthError::PeerCheckFailed(
                std::io::Error::last_os_error().to_string(),
            ));
        }
        let my_uid = unsafe { libc::getuid() };
        if ucred.uid != my_uid {
            return Err(AuthError::UidMismatch(ucred.uid, my_uid));
        }
        Ok(())
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Ok(())
    }
}
