//! Process ownership for one persistent YAS server instance.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Held for the server lifetime so two endpoints cannot split ownership of
/// one instance's process-exclusive persistent stores.
pub(crate) struct InstanceLock {
    _file: File,
}

impl InstanceLock {
    pub(crate) fn acquire(name: &crate::ServerName) -> Result<Option<Self>, String> {
        let Some(database) = crate::kv::configured_db_path() else {
            // With no persistent KV path there is no shared instance state to
            // serialize. The server is already explicitly memory-only.
            return Ok(None);
        };
        let path = lock_path(&database);
        acquire_at(&path, name).map(Some)
    }
}

fn lock_path(database: &Path) -> PathBuf {
    let mut path = database.as_os_str().to_os_string();
    path.push(".server.lock");
    PathBuf::from(path)
}

fn acquire_at(path: &Path, name: &crate::ServerName) -> Result<InstanceLock, String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("instance lock has no parent: {}", path.display()))?;
    std::fs::create_dir_all(parent).map_err(|error| {
        format!(
            "cannot create instance lock directory {}: {error}",
            parent.display()
        )
    })?;

    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|error| format!("cannot open instance lock {}: {error}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("cannot secure instance lock {}: {error}", path.display()))?;
    }

    file.try_lock().map_err(|error| {
        let owner = std::fs::read_to_string(path)
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
            .map_or_else(String::new, |pid| format!(" (held by pid {pid})"));
        format!(
            "server instance '{name}' is already running{owner}; cannot acquire {}: {error}",
            path.display()
        )
    })?;

    file.set_len(0)
        .and_then(|()| file.write_all(std::process::id().to_string().as_bytes()))
        .map_err(|error| format!("cannot record instance lock {}: {error}", path.display()))?;
    Ok(InstanceLock { _file: file })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_process_owns_an_instance_until_drop() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("kv.redb.server.lock");
        let name: crate::ServerName = "work".parse().unwrap();

        let first = acquire_at(&path, &name).unwrap();
        let error = match acquire_at(&path, &name) {
            Ok(_) => panic!("second instance lock was acquired"),
            Err(error) => error,
        };
        assert!(error.contains("server instance 'work' is already running"));
        assert!(error.contains(&format!("held by pid {}", std::process::id())));

        drop(first);
        acquire_at(&path, &name).unwrap();
    }

    #[test]
    fn lock_path_keeps_the_database_identity() {
        assert_eq!(
            lock_path(Path::new("/state/work.db")),
            Path::new("/state/work.db.server.lock")
        );
    }
}
