//! Pictures from the web that show at once. This replaces egui's plain http loader, which opened a new connection for
//! every picture and forgot it all on exit. Here a picture is downloaded once, over connections that stay open and
//! several at a time, kept as a file on disk, and read back from there (a fraction of a millisecond) every time after.
use eframe::egui::{
    self,
    load::{Bytes, BytesLoadResult, BytesLoader, BytesPoll, LoadError},
};
use sha1::{Digest, Sha1};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    sync::{Arc, Condvar, Mutex, OnceLock},
    time::{Duration, Instant},
};

enum Entry {
    Pending,
    Ready(Arc<[u8]>),
    Failed(Instant, String),
}

pub struct DiskImages {
    dir: PathBuf,
    memory: Mutex<HashMap<String, Entry>>,
    queue: Mutex<VecDeque<(String, egui::Context)>>,
    wake: Condvar,
    asked: Mutex<HashSet<String>>,
}

static SHARED: OnceLock<Arc<DiskImages>> = OnceLock::new();

impl DiskImages {
    /// Starts the loader (and its download threads) and tidies the folder down to 300 MB.
    pub fn install(ctx: &egui::Context, dir: PathBuf) {
        let _ = std::fs::create_dir_all(&dir);
        let me = Arc::new(Self { dir, memory: Mutex::new(HashMap::new()), queue: Mutex::new(VecDeque::new()), wake: Condvar::new(), asked: Mutex::new(HashSet::new()) });
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(15))
            .pool_max_idle_per_host(8)
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new());
        for _ in 0..8 {
            let (worker, client) = (me.clone(), client.clone());
            std::thread::spawn(move || worker.run(client));
        }
        let tidy = me.clone();
        std::thread::spawn(move || tidy.trim(300 * 1024 * 1024));
        let _ = SHARED.set(me.clone());
        ctx.add_bytes_loader(me);
    }

    fn path(&self, uri: &str) -> PathBuf {
        let hash = Sha1::digest(uri.as_bytes());
        self.dir.join(hash.iter().map(|b| format!("{b:02x}")).collect::<String>())
    }

    fn trim(&self, limit: u64) {
        let Ok(read) = std::fs::read_dir(&self.dir) else { return };
        let mut files: Vec<_> = read.flatten().filter_map(|e| e.metadata().ok().map(|m| (e.path(), m.len(), m.modified().ok()))).collect();
        let mut total: u64 = files.iter().map(|f| f.1).sum();
        files.sort_by_key(|f| f.2);
        for (path, size, _) in files {
            if total <= limit {
                break;
            }
            if std::fs::remove_file(path).is_ok() {
                total = total.saturating_sub(size);
            }
        }
    }

    fn enqueue(&self, uri: &str, ctx: &egui::Context) {
        self.queue.lock().unwrap().push_back((uri.to_owned(), ctx.clone()));
        self.wake.notify_one();
    }

    fn run(&self, client: reqwest::blocking::Client) {
        loop {
            let job = {
                let mut queue = self.queue.lock().unwrap();
                loop {
                    // The newest request first: what you are looking at now matters more than what scrolled away.
                    if let Some(job) = queue.pop_back() {
                        break job;
                    }
                    queue = self.wake.wait(queue).unwrap();
                }
            };
            let (uri, ctx) = job;
            let result = client.get(&uri).send().and_then(|r| r.error_for_status()).and_then(|r| r.bytes());
            let entry = match result {
                Ok(bytes) if !bytes.is_empty() => {
                    let path = self.path(&uri);
                    let part = path.with_extension("part");
                    if std::fs::write(&part, &bytes).is_ok() {
                        let _ = std::fs::rename(&part, &path);
                    }
                    Entry::Ready(Arc::from(bytes.as_ref()))
                }
                Ok(_) => Entry::Failed(Instant::now(), "empty picture".into()),
                Err(error) => Entry::Failed(Instant::now(), error.to_string()),
            };
            self.memory.lock().unwrap().insert(uri, entry);
            ctx.request_repaint();
        }
    }
}

impl BytesLoader for DiskImages {
    fn id(&self) -> &str {
        egui::generate_loader_id!(DiskImages)
    }

    fn load(&self, ctx: &egui::Context, uri: &str) -> BytesLoadResult {
        if !(uri.starts_with("https://") || uri.starts_with("http://")) {
            return Err(LoadError::NotSupported);
        }
        let mut memory = self.memory.lock().unwrap();
        match memory.get(uri) {
            Some(Entry::Ready(bytes)) => return Ok(BytesPoll::Ready { size: None, bytes: Bytes::Shared(bytes.clone()), mime: None }),
            Some(Entry::Pending) => return Ok(BytesPoll::Pending { size: None }),
            Some(Entry::Failed(at, message)) if at.elapsed() < Duration::from_secs(20) => return Err(LoadError::Loading(message.clone())),
            _ => {}
        }
        if let Ok(bytes) = std::fs::read(self.path(uri))
            && !bytes.is_empty()
        {
            let shared: Arc<[u8]> = Arc::from(bytes);
            memory.insert(uri.to_owned(), Entry::Ready(shared.clone()));
            return Ok(BytesPoll::Ready { size: None, bytes: Bytes::Shared(shared), mime: None });
        }
        memory.insert(uri.to_owned(), Entry::Pending);
        drop(memory);
        self.enqueue(uri, ctx);
        Ok(BytesPoll::Pending { size: None })
    }

    fn forget(&self, uri: &str) {
        let mut memory = self.memory.lock().unwrap();
        if matches!(memory.get(uri), Some(Entry::Ready(_) | Entry::Failed(..))) {
            memory.remove(uri);
        }
    }

    fn forget_all(&self) {
        self.memory.lock().unwrap().retain(|_, e| matches!(e, Entry::Pending));
    }

    fn byte_size(&self) -> usize {
        self.memory.lock().unwrap().values().map(|e| if let Entry::Ready(b) = e { b.len() } else { 0 }).sum()
    }

    fn has_pending(&self) -> bool {
        self.memory.lock().unwrap().values().any(|e| matches!(e, Entry::Pending))
    }
}

/// Starts downloading pictures that are about to be needed, so they are on disk (and read back at once) when they are.
pub fn prefetch(ctx: &egui::Context, uris: impl IntoIterator<Item = String>) {
    let Some(images) = SHARED.get() else { return };
    for uri in uris {
        if !(uri.starts_with("https://") || uri.starts_with("http://")) || !images.asked.lock().unwrap().insert(uri.clone()) {
            continue;
        }
        if images.path(&uri).is_file() || images.memory.lock().unwrap().contains_key(&uri) {
            continue;
        }
        images.memory.lock().unwrap().insert(uri.clone(), Entry::Pending);
        images.enqueue(&uri, ctx);
    }
}
