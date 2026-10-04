use super::*;

pub fn must_link(meta: &Metadata, order: &[String]) -> std::collections::HashSet<String> {
    let mut set = std::collections::HashSet::new();
    for id in order {
        if package(meta, id).is_ok_and(Package::is_proc_macro) {
            set.insert(id.clone());
        }
        let Ok(n) = node(meta, id) else {
            continue;
        };
        for d in &n.deps {
            if d.usable_for_script() {
                set.insert(d.pkg.clone());
            }
        }
    }
    loop {
        let snap: Vec<_> = set.iter().cloned().collect();
        let mut grew = false;
        for id in snap {
            let Ok(n) = node(meta, &id) else {
                continue;
            };
            for d in &n.deps {
                if d.usable_for_lib() && set.insert(d.pkg.clone()) {
                    grew = true;
                }
            }
        }
        if !grew {
            break;
        }
    }
    set
}

pub fn build_only(
    meta: &Metadata,
    roots: &[String],
    order: &[String],
    dev: bool,
) -> std::collections::HashSet<String> {
    let mut runtime: std::collections::HashSet<String> = roots.iter().cloned().collect();
    let mut stack = roots.to_vec();
    while let Some(id) = stack.pop() {
        if package(meta, &id).is_ok_and(Package::is_proc_macro) {
            continue;
        }
        let Ok(n) = node(meta, &id) else {
            continue;
        };
        let tested = dev && roots.contains(&id);
        for d in &n.deps {
            let runs = d.usable_for_lib() || (tested && d.usable_for_dev());
            if runs && runtime.insert(d.pkg.clone()) {
                stack.push(d.pkg.clone());
            }
        }
    }
    must_link(meta, order)
        .into_iter()
        .filter(|id| !runtime.contains(id))
        .collect()
}

impl Metadata {
    fn pkg_ix(&self) -> &HashMap<String, usize> {
        self.pkg_ix.get_or_init(|| {
            self.packages
                .iter()
                .enumerate()
                .map(|(i, p)| (p.id.clone(), i))
                .collect()
        })
    }

    fn node_ix(&self) -> &HashMap<String, usize> {
        self.node_ix.get_or_init(|| {
            self.resolve
                .as_ref()
                .map(|r| {
                    r.nodes
                        .iter()
                        .enumerate()
                        .map(|(i, n)| (n.id.clone(), i))
                        .collect()
                })
                .unwrap_or_default()
        })
    }
}

pub fn package<'a>(meta: &'a Metadata, id: &str) -> Result<&'a Package> {
    meta.pkg_ix()
        .get(id)
        .map(|&i| &meta.packages[i])
        .with_context(|| format!("package {id} missing from metadata"))
}

pub fn node<'a>(meta: &'a Metadata, id: &str) -> Result<&'a Node> {
    let resolve = meta
        .resolve
        .as_ref()
        .context("cargo metadata missing resolve")?;
    meta.node_ix()
        .get(id)
        .map(|&i| &resolve.nodes[i])
        .with_context(|| format!("resolve node {id} missing"))
}

pub fn find_manifest(start: &Path) -> Result<PathBuf> {
    let start = if start.as_os_str().is_empty() {
        Path::new(".")
    } else {
        start
    };
    let mut dir = start
        .canonicalize()
        .with_context(|| format!("no such directory: {}", start.display()))?;
    loop {
        let candidate = dir.join("Cargo.toml");
        if candidate.is_file() {
            return Ok(candidate);
        }
        if !dir.pop() {
            bail!("no Cargo.toml above {}", start.display());
        }
    }
}

fn id_path(id: &str) -> Option<(PathBuf, &str)> {
    let (url, fragment) = id.strip_prefix("path+file://")?.split_once('#')?;
    let mut bytes = Vec::with_capacity(url.len());
    let mut rest = url.as_bytes();
    while let [first, tail @ ..] = rest {
        if *first == b'%'
            && let [hi, lo, after @ ..] = tail
            && let Ok(byte) = u8::from_str_radix(std::str::from_utf8(&[*hi, *lo]).ok()?, 16)
        {
            bytes.push(byte);
            rest = after;
            continue;
        }
        bytes.push(*first);
        rest = tail;
    }
    let path = String::from_utf8(bytes).ok()?;
    let path = match path.as_bytes() {
        [b'/', drive, b':', ..] if drive.is_ascii_alphabetic() => path[1..].to_string(),
        _ => path,
    };
    Some((PathBuf::from(path), fragment))
}

pub(crate) fn same_package_id(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    match (id_path(a), id_path(b)) {
        (Some((pa, fa)), Some((pb, fb))) => {
            fa == fb && crate::resolve_path(&pa) == crate::resolve_path(&pb)
        }
        _ => false,
    }
}

const HOST_ID_PREFIX: &str = "host+";

pub fn host_id(id: &str) -> String {
    format!("{HOST_ID_PREFIX}{id}")
}

pub(crate) fn is_host_clone(id: &str) -> bool {
    id.starts_with(HOST_ID_PREFIX)
}

pub fn id_by_name(meta: &Metadata, name: &str) -> Result<String> {
    let hits: Vec<_> = meta
        .packages
        .iter()
        .filter(|p| !is_host_clone(&p.id))
        .filter(|p| p.name == name || same_package_id(&p.id, name))
        .collect();
    if hits.is_empty() {
        return Err(Unmodeled(format!(
            "package spec {name} is not a workspace package name"
        ))
        .into());
    }
    if let Some(p) = hits.iter().find(|p| p.source.is_none()) {
        return Ok(p.id.clone());
    }
    Ok(hits[0].id.clone())
}

pub fn root_id(meta: &Metadata, dir: &Path) -> Result<String> {
    let manifest = dir.join("Cargo.toml");
    if let Some(p) = meta
        .packages
        .iter()
        .find(|p| !is_host_clone(&p.id) && (p.manifest_path == manifest || p.root() == dir))
    {
        return Ok(p.id.clone());
    }
    meta.resolve
        .as_ref()
        .and_then(|r| r.root.clone())
        .context("cannot find package for directory")
}

pub fn closure(meta: &Metadata, root: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    fn walk(
        meta: &Metadata,
        id: &str,
        seen: &mut std::collections::HashSet<String>,
        out: &mut Vec<String>,
    ) -> Result<()> {
        if !seen.insert(id.to_string()) {
            return Ok(());
        }
        let n = node(meta, id)?;
        for d in &n.deps {
            if d.is_dev() {
                continue;
            }
            if d.usable_for_lib() || d.usable_for_script() {
                walk(meta, &d.pkg, seen, out)?;
            }
        }
        out.push(id.to_string());
        Ok(())
    }
    walk(meta, root, &mut seen, &mut out)?;
    Ok(out)
}

pub fn closure_many(meta: &Metadata, roots: &[String]) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for root in roots {
        for id in closure(meta, root)? {
            if seen.insert(id.clone()) {
                out.push(id);
            }
        }
    }
    Ok(out)
}

pub fn compile_deps(meta: &Metadata, id: &str) -> Result<Vec<String>> {
    let n = node(meta, id)?;
    Ok(n.deps
        .iter()
        .filter(|d| !d.is_dev() && (d.usable_for_lib() || d.usable_for_script()))
        .map(|d| d.pkg.clone())
        .collect())
}

pub fn test_compile_deps(meta: &Metadata, id: &str) -> Result<Vec<String>> {
    let mut deps = compile_deps(meta, id)?;
    for d in &node(meta, id)?.deps {
        if d.usable_for_dev() && !deps.contains(&d.pkg) {
            deps.push(d.pkg.clone());
        }
    }
    Ok(deps)
}

pub fn test_closure_many(meta: &Metadata, roots: &[String]) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for root in roots {
        for id in test_closure(meta, root)? {
            if seen.insert(id.clone()) {
                out.push(id);
            }
        }
    }
    Ok(out)
}

pub fn test_closure(meta: &Metadata, root: &str) -> Result<Vec<String>> {
    let mut out = closure(meta, root)?;
    let n = node(meta, root)?;
    for d in &n.deps {
        if !d.usable_for_dev() {
            continue;
        }
        for id in closure(meta, &d.pkg)? {
            if !out.contains(&id) {
                let root_i = out.iter().position(|x| x == root).unwrap_or(out.len());
                out.insert(root_i, id);
            }
        }
    }
    Ok(out)
}
