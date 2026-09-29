use super::*;

fn directive<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    line.strip_prefix("cargo::")
        .or_else(|| line.strip_prefix("cargo:"))?
        .strip_prefix(name)
}

#[must_use]
pub fn metadata(output: &str) -> Vec<(String, String)> {
    const RESERVED: [&str; 12] = [
        "rustc-link-lib",
        "rustc-link-search",
        "rustc-link-arg",
        "rustc-cdylib-link-arg",
        "rustc-flags",
        "rustc-cfg",
        "rustc-check-cfg",
        "rustc-env",
        "rerun-if-changed",
        "rerun-if-env-changed",
        "warning",
        "error",
    ];
    output
        .lines()
        .filter_map(|line| {
            let rest = line
                .strip_prefix("cargo::")
                .or_else(|| line.strip_prefix("cargo:"))?;
            let rest = rest.strip_prefix("metadata=").unwrap_or(rest);
            let (key, value) = rest.split_once('=')?;
            (!RESERVED.contains(&key)).then(|| (key.to_string(), value.to_string()))
        })
        .collect()
}

pub(super) fn input_stamp(
    pkg: &Package,
    output: &str,
    out_dir: &Path,
    workspace_root: &Path,
) -> String {
    let mut key = Key::new();
    let roots = [pkg.root(), workspace_root];
    let spellings: Vec<Vec<String>> = roots
        .iter()
        .map(|root| {
            let plain = root.display().to_string();
            let shown = crate::platform::env_path(root).display().to_string();
            [plain, shown]
                .into_iter()
                .flat_map(|text| {
                    let escaped = text.replace('\\', "\\\\");
                    [text, escaped]
                })
                .collect()
        })
        .collect();
    let mut found: Vec<bool> = spellings
        .iter()
        .map(|texts| texts.iter().any(|text| output.contains(text.as_str())))
        .collect();
    let pending: Vec<(usize, memchr::memmem::Finder<'_>)> = spellings
        .iter()
        .enumerate()
        .filter(|(index, _)| !found[*index])
        .flat_map(|(index, texts)| {
            texts
                .iter()
                .map(move |text| (index, memchr::memmem::Finder::new(text.as_bytes())))
        })
        .collect();
    if !pending.is_empty() {
        let mut hits = vec![false; pending.len()];
        mentions(out_dir, &pending, &mut hits);
        for ((index, _), hit) in pending.iter().zip(hits) {
            found[*index] |= hit;
        }
    }
    for (root, hit) in roots.iter().zip(found) {
        if hit {
            key.feed_str(&root.display().to_string());
        }
    }
    for line in output.lines() {
        if let Some(path) = directive(line, "rerun-if-changed=") {
            key.feed_str(path);
            watch(&mut key, &pkg.root().join(path));
        }
        if let Some(name) = directive(line, "rerun-if-env-changed=") {
            key.feed_str(name);
            let value = std::env::var_os(name);
            key.feed(&[u8::from(value.is_some())]);
            if let Some(value) = value {
                key.feed(value.as_encoded_bytes());
            }
        }
    }
    key.full_digest()
}

fn mentions(dir: &Path, needles: &[(usize, memchr::memmem::Finder<'_>)], hits: &mut [bool]) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if hits.iter().all(|hit| *hit) {
            return;
        }
        let path = entry.path();
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => mentions(&path, needles, hits),
            Ok(kind) if kind.is_file() => {
                if let Ok(bytes) = fs::read(&path) {
                    for ((_, finder), hit) in needles.iter().zip(hits.iter_mut()) {
                        *hit = *hit || finder.find(&bytes).is_some();
                    }
                }
            }
            _ => {}
        }
    }
}

pub(super) fn watch(key: &mut Key, path: &Path) {
    match fs::metadata(path) {
        Err(_) => {
            key.feed(b"absent");
        }
        Ok(m) if m.is_dir() => {
            key.feed(b"dir");
            let Ok(dir) = fs::read_dir(path) else {
                return;
            };
            let mut kids: Vec<PathBuf> = dir.flatten().map(|e| e.path()).collect();
            kids.sort();
            for kid in kids {
                if let Some(name) = kid.file_name() {
                    key.feed(name.as_encoded_bytes());
                }
                watch(key, &kid);
            }
        }
        Ok(_) => {
            key.feed(b"file");
            key.feed(&fs::read(path).unwrap_or_default());
        }
    }
}

pub fn rustc_cfgs(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| directive(line, "rustc-cfg="))
        .map(str::to_string)
        .collect()
}

pub fn rustc_envs(output: &str) -> Vec<(String, String)> {
    output
        .lines()
        .filter_map(|line| directive(line, "rustc-env="))
        .filter_map(|v| v.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

pub fn link_libs(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| directive(line, "rustc-link-lib="))
        .map(str::to_string)
        .collect()
}

pub fn link_args(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| {
            directive(line, "rustc-link-arg=").or_else(|| directive(line, "rustc-cdylib-link-arg="))
        })
        .map(str::to_string)
        .collect()
}

pub fn check_cfgs(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| directive(line, "rustc-check-cfg="))
        .map(str::to_string)
        .collect()
}

pub fn rustc_flags(output: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in output.lines() {
        let Some(rest) = directive(line, "rustc-flags=") else {
            continue;
        };
        let mut parts = rest.split_whitespace();
        while let Some(part) = parts.next() {
            match part {
                "-l" | "-L" => {
                    if let Some(value) = parts.next() {
                        out.push(part.to_string());
                        out.push(value.to_string());
                    }
                }
                other if other.starts_with("-l") || other.starts_with("-L") => {
                    out.push(other[..2].to_string());
                    out.push(other[2..].to_string());
                }
                _ => {}
            }
        }
    }
    out
}

pub fn link_search(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| directive(line, "rustc-link-search="))
        .map(str::to_string)
        .collect()
}

type CfgRows = Vec<(String, String)>;

pub(super) fn parse_print_cfg(stdout: &str) -> Result<CfgRows> {
    let mut rows = CfgRows::new();
    let mut push = |key: String, value: &str| match rows.iter_mut().find(|(k, _)| *k == key) {
        Some((_, joined)) => {
            joined.push(',');
            joined.push_str(value);
        }
        None => rows.push((key, value.to_string())),
    };
    for line in stdout.lines() {
        if let Some((k, v)) = line.split_once('=') {
            let v = v.trim_matches('"');
            let key = k.to_uppercase().replace('-', "_");
            push(key, v);
        } else if !line.is_empty() {
            push(line.to_uppercase().replace('-', "_"), "true");
        }
    }
    Ok(rows)
}

#[cfg(test)]
#[path = "../script_tests.rs"]
mod tests;
