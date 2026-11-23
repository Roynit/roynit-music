use crate::model::{Library, Track};
use anyhow::{Context, Result, bail};
use fs2::FileExt;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub struct Store {
    pub root: PathBuf,
    pub data: Library,
    _lock: File,
    committed: Library,
}
impl Store {
    pub fn open(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(&root)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join("library.lock"))?;
        lock.try_lock_exclusive()
            .context("Another Roy Music process is using this data directory")?;
        for dir in ["cache", "downloads", "lyrics", "themes", "plugins"] {
            fs::create_dir_all(root.join(dir))?;
        }
        let path = root.join("library.json");
        let data: Library = if path.exists() {
            serde_json::from_reader(File::open(&path)?)
                .context("Library is invalid; original file has been preserved")?
        } else {
            Library::default()
        };
        anyhow::ensure!(
            data.schema_version == 1,
            "Unsupported library schema {}",
            data.schema_version
        );
        data.effects.validate()?;
        Ok(Self {
            root,
            committed: data.clone(),
            data,
            _lock: lock,
        })
    }
    pub fn save(&mut self) -> Result<()> {
        match write_json(&self.root.join("library.json"), &self.data) {
            Ok(()) => {
                self.committed = self.data.clone();
                Ok(())
            }
            Err(error) => {
                self.data = self.committed.clone();
                Err(error)
            }
        }
    }
    pub fn track(&self, id: &str) -> Result<&Track> {
        self.data.tracks.get(id).context("Unknown track ID")
    }
    pub fn remove(&mut self, id: &str) -> Result<()> {
        if self.data.tracks.remove(id).is_none() {
            bail!("Unknown track ID");
        }
        self.data.favorites.retain(|x| x != id);
        for playlist in self.data.playlists.values_mut() {
            playlist.tracks.retain(|x| x != id);
        }
        let old_index = self.data.queue.index;
        let before = old_index
            .map(|i| {
                self.data
                    .queue
                    .tracks
                    .iter()
                    .take(i)
                    .filter(|x| x.as_str() == id)
                    .count()
            })
            .unwrap_or(0);
        let removed_current = self.data.queue.current() == Some(id);
        self.data.queue.tracks.retain(|x| x != id);
        self.data.queue.index = if removed_current {
            None
        } else {
            old_index.map(|i| i - before)
        };
        self.data.queue.back_stack.clear();
        self.save()
    }
    pub fn export(&self, path: &Path) -> Result<()> {
        write_json(path, &self.data)
    }
    pub fn import(&mut self, path: &Path) -> Result<()> {
        let data: Library = serde_json::from_reader(File::open(path)?)?;
        anyhow::ensure!(data.schema_version == 1, "Unsupported import schema");
        data.effects.validate()?;
        anyhow::ensure!(
            data.tracks
                .iter()
                .all(|(id, t)| id == &t.id && t.duration.is_finite() && t.duration >= 0.),
            "Imported track identity or duration is invalid"
        );
        anyhow::ensure!(
            data.playlists.iter().all(|(id, p)| id == &p.id),
            "Imported playlist identity is invalid"
        );
        anyhow::ensure!(
            data.queue
                .back_stack
                .iter()
                .all(|i| *i < data.queue.tracks.len()),
            "Invalid queue history index"
        );
        for p in data.playlists.values() {
            anyhow::ensure!(
                p.tracks.iter().all(|x| data.tracks.contains_key(x)),
                "Playlist references missing tracks"
            );
        }
        anyhow::ensure!(
            data.queue
                .tracks
                .iter()
                .all(|x| data.tracks.contains_key(x)),
            "Queue references missing tracks"
        );
        anyhow::ensure!(
            data.queue.index.is_none_or(|i| i < data.queue.tracks.len()),
            "Invalid queue index"
        );
        self.export(&self.root.join("before-import.json"))?;
        self.data = data;
        self.save()
    }
}
pub fn write_json(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    // Pretty JSON emits many tiny writes. Buffer them before durable atomic replacement.
    {
        let mut writer = std::io::BufWriter::with_capacity(64 * 1024, file.as_file_mut());
        serde_json::to_writer_pretty(&mut writer, value)?;
        writer.write_all(b"\n")?;
        writer.flush()?;
    }
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| e.error)?;
    Ok(())
}
pub fn file_hash(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buf = [0; 65536];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
pub fn key(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
