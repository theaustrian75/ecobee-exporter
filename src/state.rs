//! On-disk persistence for the bits we want to survive a restart:
//! the long-lived refresh token, and the most recent short-lived access
//! token plus its expiry.
//!
//! Writes are atomic (write-to-tmp + rename) so a power loss during
//! save can't leave a half-written file. The file mode is forced to
//! `0o600` on Unix so other users on the box can't read it.

use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StateError {
    #[error("io ({path}): {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("decoding {path}: {source}")]
    Decode {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("encoding state: {0}")]
    Encode(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PersistedState {
    /// Long-lived refresh token. Treat as a password equivalent.
    pub refresh_token: Option<String>,
    /// Most recent access token, if we have a non-expired one cached.
    pub access_token: Option<String>,
    /// Unix epoch seconds at which `access_token` becomes unusable. We
    /// proactively refresh ~30s before this.
    pub access_expires_at: Option<i64>,
}

impl PersistedState {
    pub fn load(path: &Path) -> Result<Self, StateError> {
        match fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|source| StateError::Decode { path: path.to_path_buf(), source }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(StateError::Io { path: path.to_path_buf(), source }),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), StateError> {
        let bytes = serde_json::to_vec_pretty(self)?;
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)
                .map_err(|source| StateError::Io { path: parent.to_path_buf(), source })?;
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, &bytes)
            .map_err(|source| StateError::Io { path: tmp.clone(), source })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            fs::set_permissions(&tmp, perms)
                .map_err(|source| StateError::Io { path: tmp.clone(), source })?;
        }
        fs::rename(&tmp, path)
            .map_err(|source| StateError::Io { path: path.to_path_buf(), source })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_yields_default() {
        let dir = tempdir();
        let path = dir.join("nope.json");
        let s = PersistedState::load(&path).unwrap();
        assert!(s.refresh_token.is_none());
        assert!(s.access_token.is_none());
    }

    #[test]
    fn round_trip_preserves_fields() {
        let dir = tempdir();
        let path = dir.join("state.json");
        let original = PersistedState {
            refresh_token: Some("rt-xyz".into()),
            access_token: Some("at-abc".into()),
            access_expires_at: Some(1_777_000_000),
        };
        original.save(&path).unwrap();
        let loaded = PersistedState::load(&path).unwrap();
        assert_eq!(loaded.refresh_token, original.refresh_token);
        assert_eq!(loaded.access_token, original.access_token);
        assert_eq!(loaded.access_expires_at, original.access_expires_at);
    }

    #[cfg(unix)]
    #[test]
    fn save_creates_0600_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir();
        let path = dir.join("state.json");
        PersistedState::default().save(&path).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "state file should be 0600, got {mode:o}");
    }

    fn tempdir() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "ecobee-exporter-test-{}-{}",
            std::process::id(),
            fastrand_like()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn fastrand_like() -> u64 {
        use std::time::SystemTime;
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0u128, |d| d.as_nanos());
        u64::try_from(nanos & u128::from(u64::MAX)).unwrap_or(0)
    }
}
