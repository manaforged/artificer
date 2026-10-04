use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub struct InstallReport {
    pub binary: PathBuf,
    pub shim: PathBuf,
    pub env: PathBuf,
}

fn exe(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

pub(crate) fn launcher(cargo_home: &Path) -> PathBuf {
    cargo_home.join("bin").join(exe("artificer"))
}

pub(crate) fn self_launcher() -> anyhow::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    if exe
        .file_stem()
        .is_some_and(|name| name.eq_ignore_ascii_case("cargo"))
    {
        return Ok(launcher(&crate::cargo::cargo_home()));
    }
    Ok(exe)
}

pub(crate) fn env_file(control: &Path) -> PathBuf {
    #[cfg(unix)]
    {
        control.join("env")
    }
    #[cfg(windows)]
    {
        control.join("env.ps1")
    }
}

pub fn check_real_cargo(real_cargo: &Path, control: &Path) -> Result<()> {
    if !real_cargo.is_file() {
        bail!(
            "real Cargo not found at {}; set ARTIFICER_REAL_CARGO",
            real_cargo.display()
        );
    }
    if crate::resolve_path(real_cargo)
        == crate::resolve_path(&control.join("bin").join(exe("cargo")))
    {
        bail!("the real Cargo must not be the Artificer shim");
    }
    Ok(())
}

pub fn install(
    binary: &Path,
    real_cargo: &Path,
    cargo_home: &Path,
    control: &Path,
) -> Result<InstallReport> {
    let shim_dir = control.join("bin");
    let binary_dst = launcher(cargo_home);
    let shim_dst = shim_dir.join(exe("cargo"));
    check_real_cargo(real_cargo, control)?;
    fs::create_dir_all(cargo_home.join("bin"))
        .with_context(|| format!("create {}", cargo_home.join("bin").display()))?;
    fs::create_dir_all(&shim_dir).with_context(|| format!("create {}", shim_dir.display()))?;
    copy_launcher(binary, &binary_dst)?;
    copy_launcher(binary, &shim_dst)?;
    let stamp = control.join("real-cargo");
    fs::write(&stamp, format!("{}\n", real_cargo.display()))
        .with_context(|| format!("write {}", stamp.display()))?;
    let env = env_file(control);
    fs::write(&env, crate::home::env_script_for(&shim_dir))
        .with_context(|| format!("write {}", env.display()))?;
    Ok(InstallReport {
        binary: binary_dst,
        shim: shim_dst,
        env,
    })
}

pub fn uninstall(cargo_home: &Path, control: &Path) -> Result<Vec<PathBuf>> {
    let mut remaining = Vec::new();
    let binary = cargo_home.join("bin").join(exe("artificer"));
    let owned_by_cargo = cargo_package(cargo_home).is_some();
    let paths = [
        (!owned_by_cargo).then_some(binary),
        Some(control.join("bin").join(exe("cargo"))),
        Some(control.join("real-cargo")),
        Some(env_file(control)),
        Some(control.join("store")),
        Some(control.join(CREATED)),
    ];
    for path in paths.into_iter().flatten() {
        if !path.exists() {
            continue;
        }
        if fs::remove_file(&path).is_err() {
            remaining.push(path);
        }
    }
    for dir in [control.join("bin"), control.to_path_buf()] {
        drop(fs::remove_dir(dir));
    }
    Ok(remaining)
}

#[derive(Deserialize)]
struct CargoInstalls {
    installs: BTreeMap<String, CargoInstall>,
}

#[derive(Deserialize)]
struct CargoInstall {
    bins: Vec<String>,
}

pub fn cargo_package(cargo_home: &Path) -> Option<String> {
    let bytes = fs::read(cargo_home.join(".crates2.json")).ok()?;
    let record: CargoInstalls = serde_json::from_slice(&bytes).ok()?;
    let name = exe("artificer");
    record
        .installs
        .into_iter()
        .find(|(_, install)| install.bins.contains(&name))
        .and_then(|(id, _)| id.split(' ').next().map(str::to_string))
}

#[cfg(unix)]
pub fn profile_line(home: &Path, control: &Path) -> String {
    let file = env_file(control);
    let env = file
        .strip_prefix(home)
        .ok()
        .and_then(Path::to_str)
        .filter(|rel| !rel.contains(['"', '$', '`', '\\']))
        .map_or_else(
            || crate::home::sh_quote(&file.display().to_string()),
            |rel| format!("\"$HOME/{rel}\""),
        );
    format!("[ ! -f {env} ] || . {env}")
}

#[cfg(unix)]
pub fn profiles(home: &Path, shell: Option<&Path>, zdotdir: Option<&Path>) -> Vec<PathBuf> {
    let mut files = vec![home.join(".profile")];
    files.extend(
        [".bashrc", ".bash_profile"]
            .map(|name| home.join(name))
            .into_iter()
            .filter(|path| path.is_file()),
    );
    let zshenv = zdotdir.unwrap_or(home).join(".zshenv");
    let zsh = shell
        .and_then(Path::file_name)
        .is_some_and(|name| name == "zsh");
    if zsh || zshenv.is_file() {
        files.push(zshenv);
    }
    files
}

#[cfg(unix)]
pub fn installed_profiles(home: &Path, zdotdir: Option<&Path>) -> Vec<PathBuf> {
    let mut files = profiles(home, Some(Path::new("zsh")), zdotdir);
    let home_zshenv = home.join(".zshenv");
    if !files.contains(&home_zshenv) {
        files.push(home_zshenv);
    }
    files
}

const CREATED: &str = "created";

#[cfg(unix)]
fn created(control: &Path) -> Vec<PathBuf> {
    fs::read_to_string(control.join(CREATED))
        .unwrap_or_default()
        .lines()
        .map(PathBuf::from)
        .collect()
}

#[cfg(unix)]
pub fn add_to_profiles(files: &[PathBuf], line: &str, control: &Path) -> Result<Vec<PathBuf>> {
    let mut changed = Vec::new();
    let mut made = created(control);
    for file in files {
        let body = match fs::read_to_string(file) {
            Ok(body) => body,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                made.push(file.clone());
                String::new()
            }
            Err(e) => return Err(e).with_context(|| format!("read {}", file.display())),
        };
        if body.lines().any(|l| l == line) {
            continue;
        }
        let gap = if body.is_empty() || body.ends_with('\n') {
            ""
        } else {
            "\n"
        };
        fs::write(file, format!("{body}{gap}{line}\n"))
            .with_context(|| format!("write {}", file.display()))?;
        changed.push(file.clone());
    }
    record_created(control, made)?;
    Ok(changed)
}

#[cfg(unix)]
fn record_created(control: &Path, mut made: Vec<PathBuf>) -> Result<()> {
    made.sort();
    made.dedup();
    if made.is_empty() {
        return Ok(());
    }
    let list: String = made.iter().map(|p| format!("{}\n", p.display())).collect();
    let path = control.join(CREATED);
    fs::write(&path, list).with_context(|| format!("write {}", path.display()))
}

#[cfg(unix)]
pub fn remove_from_profiles(files: &[PathBuf], line: &str, control: &Path) -> Result<Vec<PathBuf>> {
    let made = created(control);
    let mut changed = Vec::new();
    for file in files {
        let Ok(body) = fs::read_to_string(file) else {
            continue;
        };
        if !body.lines().any(|l| l == line) {
            continue;
        }
        let kept: String = body
            .split_inclusive('\n')
            .filter(|l| l.trim_end_matches(['\n', '\r']) != line)
            .collect();
        if kept.trim().is_empty() && made.contains(file) {
            fs::remove_file(file).with_context(|| format!("remove {}", file.display()))?;
        } else {
            fs::write(file, kept).with_context(|| format!("write {}", file.display()))?;
        }
        changed.push(file.clone());
    }
    Ok(changed)
}

fn expand_env(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(len) = after.find('%') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let name = &after[..len];
        match std::env::var(name) {
            Ok(value) if !name.is_empty() => {
                out.push_str(&value);
                rest = &after[len + 1..];
            }
            _ => {
                out.push('%');
                out.push_str(name);
                rest = &after[len..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn dir_key(entry: &str) -> String {
    expand_env(entry)
        .trim_end_matches(['\\', '/'])
        .to_lowercase()
}

fn same_dir(a: &str, b: &str) -> bool {
    dir_key(a) == dir_key(b)
}

pub fn path_prepend(path: &str, dir: &str) -> Option<String> {
    let entries: Vec<&str> = path.split(';').filter(|e| !e.is_empty()).collect();
    if entries.first().is_some_and(|first| same_dir(first, dir)) {
        return None;
    }
    let rest = entries.into_iter().filter(|e| !same_dir(e, dir));
    Some(
        std::iter::once(dir)
            .chain(rest)
            .collect::<Vec<_>>()
            .join(";"),
    )
}

pub fn path_remove(path: &str, dir: &str) -> Option<String> {
    let entries: Vec<&str> = path.split(';').filter(|e| !e.is_empty()).collect();
    if !entries.iter().any(|e| same_dir(e, dir)) {
        return None;
    }
    Some(
        entries
            .into_iter()
            .filter(|e| !same_dir(e, dir))
            .collect::<Vec<_>>()
            .join(";"),
    )
}

#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub fn add_to_user_path(dir: &Path) -> Result<bool> {
    let current = windows::read()?;
    let Some(next) = path_prepend(&current.value, &dir.display().to_string()) else {
        return Ok(false);
    };
    windows::write(&current, &next)?;
    Ok(true)
}

#[cfg(windows)]
pub fn remove_from_user_path(dir: &Path) -> Result<bool> {
    let current = windows::read()?;
    let Some(next) = path_remove(&current.value, &dir.display().to_string()) else {
        return Ok(false);
    };
    windows::write(&current, &next)?;
    Ok(true)
}

pub fn refresh_shim(running: &Path, control: &Path, cargo_home: &Path) -> Result<bool> {
    let shim = control.join("bin").join(exe("cargo"));
    if crate::resolve_path(running) != crate::resolve_path(&shim) {
        return Ok(false);
    }
    let shim = shim.as_path();
    let installed = cargo_home.join("bin").join(exe("artificer"));
    let (Ok(newer), Ok(current)) = (fs::metadata(&installed), fs::metadata(shim)) else {
        return Ok(false);
    };
    let stale = match (newer.modified(), current.modified()) {
        (Ok(newer), Ok(current)) => newer > current,
        _ => false,
    };
    if !stale || crate::resolve_path(&installed) == crate::resolve_path(shim) {
        return Ok(false);
    }
    copy_launcher(&installed, shim)?;
    Ok(true)
}

fn copy_launcher(src: &Path, dst: &Path) -> Result<()> {
    if crate::resolve_path(src) == crate::resolve_path(dst) {
        return Ok(());
    }
    let staged = dst.with_extension("new");
    fs::copy(src, &staged)
        .with_context(|| format!("copy {} -> {}", src.display(), staged.display()))?;
    fs::rename(&staged, dst)
        .with_context(|| format!("rename {} -> {}", staged.display(), dst.display()))?;
    Ok(())
}

#[cfg(test)]
#[path = "install_tests.rs"]
mod tests;
