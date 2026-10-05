use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MEMO_VERSION: u32 = 2;
const RACY_WINDOW: Duration = Duration::from_secs(2);
const MEMO_DIR: &str = "digests";
const SHARDS: u8 = 64;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Stamp {
    len: u64,
    mtime_ns: u128,
    inode: Option<u64>,
    device: Option<u64>,
    ctime_ns: Option<i128>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Entry {
    stamp: Stamp,
    digest: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    env: Vec<String>,
}

pub(crate) struct Scan {
    pub(crate) digest: String,
    pub(crate) env: Vec<String>,
}

#[derive(Default, Serialize, Deserialize)]
struct Shard {
    version: u32,
    entries: HashMap<PathBuf, Entry>,
    #[serde(skip)]
    fresh: HashMap<PathBuf, Entry>,
}

type Loaded = Mutex<HashMap<PathBuf, Shard>>;

fn loaded() -> &'static Loaded {
    static LOADED: OnceLock<Loaded> = OnceLock::new();
    LOADED.get_or_init(Loaded::default)
}

#[cfg(windows)]
fn change_time(path: &Path) -> Option<i128> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_BASIC_INFO, FileBasicInfo, GetFileInformationByHandleEx,
    };
    let file = fs::File::open(path).ok()?;
    let mut info = FILE_BASIC_INFO {
        CreationTime: 0,
        LastAccessTime: 0,
        LastWriteTime: 0,
        ChangeTime: 0,
        FileAttributes: 0,
    };
    let size = u32::try_from(std::mem::size_of::<FILE_BASIC_INFO>()).ok()?;
    // SAFETY: `file` stays open for the call and `info` is a FILE_BASIC_INFO of the size passed.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileBasicInfo,
            (&raw mut info).cast(),
            size,
        )
    };
    (ok != 0).then(|| i128::from(info.ChangeTime))
}

#[cfg_attr(
    not(windows),
    expect(
        unused_variables,
        reason = "only Windows reads the change time by path"
    )
)]
fn stamp(path: &Path, meta: &fs::Metadata) -> Option<Stamp> {
    let mtime_ns = meta
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos();
    #[cfg(unix)]
    let (inode, device, ctime_ns) = {
        use std::os::unix::fs::MetadataExt;
        (
            Some(meta.ino()),
            Some(meta.dev()),
            Some(i128::from(meta.ctime()) * 1_000_000_000 + i128::from(meta.ctime_nsec())),
        )
    };
    #[cfg(windows)]
    let (inode, device, ctime_ns) = (None, None, Some(change_time(path)?));
    #[cfg(not(any(unix, windows)))]
    let (inode, device, ctime_ns) = (None, None, None);
    Some(Stamp {
        len: meta.len(),
        mtime_ns,
        inode,
        device,
        ctime_ns,
    })
}

fn settled(stamp: &Stamp) -> bool {
    let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        return false;
    };
    now.as_nanos().saturating_sub(stamp.mtime_ns) >= RACY_WINDOW.as_nanos()
}

fn shard_path(home: &Path, canonical: &Path) -> PathBuf {
    let index = blake3::hash(canonical.as_os_str().as_encoded_bytes()).as_bytes()[0] % SHARDS;
    home.join(MEMO_DIR)
        .join(format!("v{MEMO_VERSION}"))
        .join(format!("{index:02x}.json"))
}

fn read_shard(path: &Path) -> Shard {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Shard>(&bytes).ok())
        .filter(|shard| shard.version == MEMO_VERSION)
        .unwrap_or_default()
}

fn scan(path: &Path) -> Result<Scan> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let mut env = Vec::new();
    if let Ok(text) = std::str::from_utf8(&bytes) {
        crate::key::scan_env(text, &mut env);
    }
    Ok(Scan {
        digest: blake3::hash(&bytes).to_hex().to_string(),
        env,
    })
}

fn lookup(shard: &Path, canonical: &Path, current: &Stamp) -> Option<Scan> {
    let found = |loaded: &Shard| {
        loaded
            .entries
            .get(canonical)
            .filter(|entry| entry.stamp == *current)
            .map(|entry| Scan {
                digest: entry.digest.clone(),
                env: entry.env.clone(),
            })
    };
    if let Some(loaded) = loaded()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(shard)
    {
        return found(loaded);
    }
    let read = read_shard(shard);
    let mut all = loaded().lock().unwrap_or_else(PoisonError::into_inner);
    found(all.entry(shard.to_path_buf()).or_insert(read))
}

pub(crate) fn file(home: Option<&Path>, path: &Path) -> Result<String> {
    Memo::new(home).file(path)
}

pub(crate) struct Memo<'a> {
    home: Option<&'a Path>,
    dirty: Vec<PathBuf>,
}

impl<'a> Memo<'a> {
    pub(crate) fn new(home: Option<&'a Path>) -> Self {
        Self {
            home,
            dirty: Vec::new(),
        }
    }

    pub(crate) fn file(&mut self, path: &Path) -> Result<String> {
        if self.home.is_none() {
            return Ok(scan(path)?.digest);
        }
        let Ok(canonical) = fs::canonicalize(path) else {
            return Ok(scan(path)?.digest);
        };
        let Ok(meta) = fs::metadata(&canonical) else {
            return Ok(scan(path)?.digest);
        };
        Ok(self.known(&canonical, &meta)?.digest)
    }

    pub(crate) fn known(&mut self, canonical: &Path, meta: &fs::Metadata) -> Result<Scan> {
        let (Some(home), Some(before)) = (self.home, stamp(canonical, meta)) else {
            return scan(canonical);
        };
        let shard = shard_path(home, canonical);
        if let Some(hit) = lookup(&shard, canonical, &before) {
            return Ok(hit);
        }
        let found = scan(canonical)?;
        let after = fs::metadata(canonical)
            .ok()
            .and_then(|meta| stamp(canonical, &meta));
        if after == Some(before) && settled(&before) {
            let entry = Entry {
                stamp: before,
                digest: found.digest.clone(),
                env: found.env.clone(),
            };
            let mut all = loaded().lock().unwrap_or_else(PoisonError::into_inner);
            let loaded = all.entry(shard.clone()).or_default();
            loaded
                .entries
                .insert(canonical.to_path_buf(), entry.clone());
            loaded.fresh.insert(canonical.to_path_buf(), entry);
            drop(all);
            if !self.dirty.contains(&shard) {
                self.dirty.push(shard);
            }
        }
        Ok(found)
    }

    pub(crate) fn flush(&mut self) {
        for shard in std::mem::take(&mut self.dirty) {
            let fresh = {
                let mut all = loaded().lock().unwrap_or_else(PoisonError::into_inner);
                all.get_mut(&shard)
                    .map(|loaded| std::mem::take(&mut loaded.fresh))
                    .unwrap_or_default()
            };
            if fresh.is_empty() {
                continue;
            }
            let mut disk = read_shard(&shard);
            disk.version = MEMO_VERSION;
            disk.entries.extend(fresh);
            let Some(dir) = shard.parent() else {
                continue;
            };
            let Ok(bytes) = serde_json::to_vec(&disk) else {
                continue;
            };
            if fs::create_dir_all(dir).is_ok() {
                drop(crate::platform::replace_atomic(&shard, |tmp| {
                    fs::write(tmp, &bytes)
                }));
            }
        }
    }
}

impl Drop for Memo<'_> {
    fn drop(&mut self) {
        self.flush();
    }
}

#[cfg(all(test, unix))]
#[path = "digest_tests.rs"]
mod tests;
