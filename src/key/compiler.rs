use super::*;

pub fn rustc_version(home: &Path) -> Result<String> {
    rustc_version_in(home, &std::env::current_dir().context("cwd")?)
}

pub(crate) fn toolchain_key(dir: &Path) -> String {
    let dir_key = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let stamp = rustup_toolchains()
        .and_then(|dir| fs::metadata(dir).ok())
        .and_then(|meta| meta.modified().ok())
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    let toolchain = toolchain_file(dir)
        .map(|file| {
            let body = fs::read_to_string(&file).unwrap_or_default();
            format!("{}:{body}", file.display())
        })
        .unwrap_or_default();
    let settings = rustup_toolchains()
        .and_then(|dir| dir.parent().map(|root| root.join("settings.toml")))
        .and_then(|file| fs::read_to_string(file).ok())
        .unwrap_or_default();
    format!(
        "{}|{}|{toolchain}|{settings}|{}|{}|{stamp}",
        dir_key.display(),
        std::env::var("RUSTUP_TOOLCHAIN").unwrap_or_default(),
        std::env::var("PATH").unwrap_or_default(),
        compiler_file_identity(),
    )
}

pub(crate) fn explicit_rustc_identity() -> Option<String> {
    let Some(rustc) = std::env::var("RUSTC")
        .ok()
        .filter(|rustc| !rustc.trim().is_empty())
    else {
        return Some(String::new());
    };
    let path = Path::new(&rustc);
    let resolved = if path.is_absolute() {
        Some(path.to_path_buf())
    } else if path.components().count() == 1 {
        let relative_entry = std::env::var_os("PATH")
            .is_some_and(|dirs| std::env::split_paths(&dirs).any(|dir| dir.is_relative()));
        if relative_entry {
            return None;
        }
        crate::platform::on_path(&rustc)
    } else {
        None
    };
    resolved
        .filter(|path| path.is_file())
        .map(|path| file_identity(&path.display().to_string()))
}

fn compiler_file_identity() -> String {
    file_identity(&rustc_bin())
}

pub(crate) fn file_identity(named: &str) -> String {
    let named = named.to_string();
    let path = if Path::new(&named).components().count() > 1 {
        Some(PathBuf::from(&named))
    } else {
        crate::platform::on_path(&named)
    };
    let Some(path) = path.and_then(|p| p.canonicalize().ok()) else {
        return named;
    };
    let Ok(meta) = fs::metadata(&path) else {
        return path.display().to_string();
    };
    let modified = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    format!("{}:{}:{modified}", path.display(), meta.len())
}

fn rustup_toolchains() -> Option<PathBuf> {
    let root = std::env::var_os("RUSTUP_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::home::dirs_home().join(".rustup"));
    Some(root.join("toolchains"))
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

pub(crate) fn probe_memo(
    home: &Path,
    name: &str,
    key: &str,
    run: impl FnOnce() -> Result<String>,
) -> Result<String> {
    const FRESH: std::time::Duration = std::time::Duration::from_secs(60);
    let dir = home.join("versions");
    let digest = blake3::hash(key.as_bytes()).to_hex();
    let path = dir.join(format!("{name}-{}", &digest[..32]));
    if let Ok(text) = fs::read_to_string(&path)
        && let Some((stamp, value)) = text.split_once('\n')
        && let Ok(stamp) = stamp.parse::<u64>()
        && unix_now().saturating_sub(stamp) < FRESH.as_secs()
    {
        return Ok(value.to_string());
    }
    let value = run()?;
    if fs::create_dir_all(&dir).is_ok() {
        drop(crate::platform::replace_atomic(&path, |tmp| {
            fs::write(tmp, format!("{}\n{value}", unix_now()))
        }));
    }
    Ok(value)
}

#[must_use]
pub fn rustc_bin() -> String {
    std::env::var("RUSTC")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "rustc".to_string())
}

pub fn rustc_version_in(home: &Path, dir: &Path) -> Result<String> {
    static CACHE: std::sync::LazyLock<
        std::sync::Mutex<std::collections::HashMap<String, (std::time::Instant, String)>>,
    > = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    const FRESH: std::time::Duration = std::time::Duration::from_secs(60);
    let key = format!("{}|{}", toolchain_key(dir), rustc_bin());
    if let Ok(cache) = CACHE.lock()
        && let Some((at, v)) = cache.get(&key)
        && at.elapsed() < FRESH
    {
        return Ok(v.clone());
    }
    let v = probe_memo(home, "rustc", &key, || {
        let mut cmd = Command::new(rustc_bin());
        cmd.arg("-vV").current_dir(dir);
        crate::jobs::isolate(&mut cmd);
        let out = cmd.output().context("rustc -vV")?;
        if !out.status.success() {
            anyhow::bail!("rustc -vV failed");
        }
        let mut v = String::from_utf8(out.stdout)?;
        if let Some(file) = toolchain_file(dir) {
            v.push('\n');
            v.push_str("toolchain-file\n");
            if let Ok(body) = fs::read_to_string(&file) {
                v.push_str(&body);
            }
        }
        Ok(v)
    })?;
    if let Ok(mut cache) = CACHE.lock() {
        cache.insert(key, (std::time::Instant::now(), v.clone()));
    }
    Ok(v)
}

fn toolchain_file(dir: &Path) -> Option<PathBuf> {
    let mut cur = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    loop {
        let toml = cur.join("rust-toolchain.toml");
        if toml.is_file() {
            return Some(toml);
        }
        let legacy = cur.join("rust-toolchain");
        if legacy.is_file() {
            return Some(legacy);
        }
        if !cur.pop() {
            return None;
        }
    }
}

pub fn rustc_host(version: &str) -> Result<String> {
    for line in version.lines() {
        if let Some(host) = line.strip_prefix("host: ") {
            return Ok(host.to_string());
        }
    }
    anyhow::bail!("no host: in rustc -vV")
}

pub fn rustc_print_cfg(home: &Path, dir: &Path) -> Result<Vec<String>> {
    rustc_print_cfg_with_flags(home, dir, &[])
}

pub(crate) fn rustc_print_cfg_with_flags(
    home: &Path,
    dir: &Path,
    flags: &[String],
) -> Result<Vec<String>> {
    static CACHE: std::sync::LazyLock<
        std::sync::Mutex<std::collections::HashMap<String, Vec<String>>>,
    > = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    let key = format!("{}|{}|{flags:?}", toolchain_key(dir), rustc_bin());
    if let Some(hit) = CACHE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&key)
    {
        return Ok(hit.clone());
    }
    let text = probe_memo(home, "rustc-cfg", &key, || {
        let mut cmd = Command::new(rustc_bin());
        cmd.arg("--print").arg("cfg").args(flags).current_dir(dir);
        crate::jobs::isolate(&mut cmd);
        let out = cmd.output().context("rustc --print cfg")?;
        anyhow::ensure!(out.status.success(), "rustc --print cfg failed");
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    })?;
    let lines: Vec<String> = text.lines().map(str::to_string).collect();
    CACHE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(key, lines.clone());
    Ok(lines)
}
