use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use notify::{Event, EventKind, RecursiveMode, Watcher, Config};
use notify::event::{ModifyKind, CreateKind, RemoveKind};
use tokio::sync::mpsc;
use parking_lot::RwLock;

/// Debounce duration: coalesce rapid events on the same file
const DEBOUNCE_MS: u64 = 500;

pub enum WatchEvent {
    FileCreated(PathBuf),
    FileModified(PathBuf),
    FileDeleted(PathBuf),
    FileRenamed(PathBuf, PathBuf), // old, new
}

/// Cross-platform file watcher wrapping notify's RecommendedWatcher
pub struct FileWatcher {
    event_tx: mpsc::UnboundedSender<WatchEvent>,
    _watcher: notify::RecommendedWatcher,
    watched_path: PathBuf,
}

impl FileWatcher {
    /// Create a native file watcher for local filesystems
    pub fn new(
        path: &Path,
        event_tx: mpsc::UnboundedSender<WatchEvent>,
    ) -> Result<Self, String> {
        let tx = event_tx.clone();
        let watched_path = path.to_path_buf();

        let mut watcher = notify::RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    match event.kind {
                        EventKind::Create(CreateKind::File) => {
                            for path in event.paths {
                                let _ = tx.send(WatchEvent::FileCreated(path));
                            }
                        }
                        EventKind::Modify(ModifyKind::Data(_)) => {
                            for path in event.paths {
                                let _ = tx.send(WatchEvent::FileModified(path));
                            }
                        }
                        EventKind::Remove(RemoveKind::File) => {
                            for path in event.paths {
                                let _ = tx.send(WatchEvent::FileDeleted(path));
                            }
                        }
                        EventKind::Modify(ModifyKind::Name(_)) => {
                            if event.paths.len() >= 2 {
                                let _ = tx.send(WatchEvent::FileRenamed(
                                    event.paths[0].clone(),
                                    event.paths[1].clone(),
                                ));
                            }
                        }
                        _ => {}
                    }
                }
            },
            Config::default(),
        )
        .map_err(|e| format!("Failed to create watcher: {}", e))?;

        watcher
            .watch(path, RecursiveMode::Recursive)
            .map_err(|e| format!("Failed to watch path: {}", e))?;

        Ok(FileWatcher {
            event_tx,
            _watcher: watcher,
            watched_path,
        })
    }

    pub fn watched_path(&self) -> &Path {
        &self.watched_path
    }
}

/// Network file watcher: polls at intervals since SMB/NFS doesn't support inotify
pub struct NetworkWatcher {
    event_tx: mpsc::UnboundedSender<WatchEvent>,
    watched_path: PathBuf,
    poll_interval: Duration,
    last_snapshot: Arc<RwLock<Vec<(PathBuf, std::time::SystemTime, u64)>>>,
}

impl NetworkWatcher {
    pub fn new(
        path: &Path,
        event_tx: mpsc::UnboundedSender<WatchEvent>,
        poll_interval: Duration,
    ) -> Self {
        NetworkWatcher {
            event_tx,
            watched_path: path.to_path_buf(),
            poll_interval,
            last_snapshot: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn start(&self) {
        let tx = self.event_tx.clone();
        let path = self.watched_path.clone();
        let interval = self.poll_interval;
        let snapshot = self.last_snapshot.clone();

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(interval).await;
                Self::poll_once(&path, &tx, &snapshot);
            }
        });
    }

    fn poll_once(
        path: &Path,
        tx: &mpsc::UnboundedSender<WatchEvent>,
        snapshot: &Arc<RwLock<Vec<(PathBuf, std::time::SystemTime, u64)>>>,
    ) {
        let mut current: Vec<(PathBuf, std::time::SystemTime, u64)> = Vec::new();

        let walker = walkdir::WalkDir::new(path)
            .max_depth(20)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file());

        for entry in walker {
            if let Ok(meta) = entry.metadata() {
                current.push((
                    entry.path().to_path_buf(),
                    meta.modified().unwrap_or(std::time::UNIX_EPOCH),
                    meta.len(),
                ));
            }
        }

        let mut old = snapshot.write();
        
        for (cur_path, cur_mtime, cur_size) in &current {
            let found = old.iter().find(|(p, m, s)| {
                p == cur_path && m == cur_mtime && s == cur_size
            });
            if found.is_none() {
                let existed = old.iter().any(|(p, _, _)| p == cur_path);
                if existed {
                    let _ = tx.send(WatchEvent::FileModified(cur_path.clone()));
                } else {
                    let _ = tx.send(WatchEvent::FileCreated(cur_path.clone()));
                }
            }
        }

        for (old_path, _, _) in old.iter() {
            if !current.iter().any(|(p, _, _)| p == old_path) {
                let _ = tx.send(WatchEvent::FileDeleted(old_path.clone()));
            }
        }

        *old = current;
    }
}

/// Debouncer: groups rapid events on the same file
pub struct Debouncer {
    events: std::collections::HashMap<PathBuf, tokio::time::Instant>,
    delay: Duration,
}

impl Debouncer {
    pub fn new(delay_ms: u64) -> Self {
        Debouncer {
            events: std::collections::HashMap::new(),
            delay: Duration::from_millis(delay_ms),
        }
    }

    pub fn should_process(&mut self, path: &Path) -> bool {
        let now = tokio::time::Instant::now();
        match self.events.get(path) {
            Some(last) if now.duration_since(*last) < self.delay => {
                self.events.insert(path.to_path_buf(), now);
                false
            }
            _ => {
                self.events.insert(path.to_path_buf(), now);
                true
            }
        }
    }
}