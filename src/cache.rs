use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

fn files(root: &Path) -> Result<Vec<(PathBuf, u64, SystemTime)>> {
    let directory = root.join("cache");
    std::fs::create_dir_all(&directory)?;
    let base = directory.canonicalize()?;
    ensure!(
        base.starts_with(root.canonicalize()?),
        "Cache directory escaped application data"
    );
    let mut files = Vec::new();
    for entry in std::fs::read_dir(&directory)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let path = entry.path();
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if stem.len() != 64 || !stem.bytes().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        let metadata = entry.metadata()?;
        files.push((
            path,
            metadata.len(),
            metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
        ));
    }
    Ok(files)
}
pub fn status(root: &Path, limit: u64) -> Result<Value> {
    let files = files(root)?;
    Ok(
        json!({"bytes":files.iter().map(|(_,size,_)|*size).sum::<u64>(),"files":files.len(),"limit_bytes":limit}),
    )
}
/// Only app-owned hashed cache files are eligible; never follows symlinks or deletes user downloads.
pub fn prune(root: &Path, limit: u64, protected: &[PathBuf]) -> Result<Value> {
    let mut files = files(root)?;
    files.sort_by_key(|(_, _, time)| *time);
    let mut bytes = files.iter().map(|(_, size, _)| *size).sum::<u64>();
    let mut removed = 0;
    let protected: Vec<_> = protected
        .iter()
        .filter_map(|p| p.canonicalize().ok())
        .collect();
    for (path, size, _) in files {
        if bytes <= limit {
            break;
        }
        if protected
            .iter()
            .any(|p| path.canonicalize().is_ok_and(|file| file == *p))
        {
            continue;
        }
        if let Err(error) = std::fs::remove_file(path) {
            if error.kind() == std::io::ErrorKind::PermissionDenied {
                continue;
            }
            return Err(error.into());
        }
        bytes = bytes.saturating_sub(size);
        removed += 1;
    }
    Ok(json!({"bytes":bytes,"removed_files":removed,"limit_bytes":limit,"over_limit":bytes>limit}))
}
pub fn touch(path: &Path) {
    if let Ok(file) = std::fs::OpenOptions::new().write(true).open(path) {
        let _ = file.set_times(std::fs::FileTimes::new().set_modified(SystemTime::now()));
    }
}
