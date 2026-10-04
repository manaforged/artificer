use crate::build::check_roots;
use crate::serve::Request as ServeRequest;
use crate::{cargo, config, features, key, manifest};
use anyhow::Result;
use std::path::{Path, PathBuf};

pub fn passthrough_reason(req: &ServeRequest, dev: bool, home: &Path) -> Result<Option<String>> {
    crate::profile::span(crate::profile::SetupPhase::Gate, || {
        passthrough_reason_inner(req, dev, home)
    })
}

fn passthrough_reason_inner(req: &ServeRequest, dev: bool, home: &Path) -> Result<Option<String>> {
    let manifest = match preflight(req, home)? {
        Ok(manifest) => manifest,
        Err(reason) => return Ok(Some(reason)),
    };
    let pkg_dir = manifest.parent().unwrap_or(&req.dir);
    let mut extra = features::feature_args(req.all_features, &req.features, req.no_default);
    extra.extend(req.meta_flags.iter().cloned());
    let extra: Vec<&str> = extra.iter().map(String::as_str).collect();
    let meta = match modeled(cargo::metadata_extra(&manifest, &extra, home))? {
        Ok(meta) => meta,
        Err(reason) => return Ok(Some(reason)),
    };
    if let Some(reason) = manifest_reason(req, &meta, pkg_dir) {
        return Ok(Some(reason));
    }
    if let Some(reason) = config_reason(req.op == "test", pkg_dir, home)? {
        return Ok(Some(reason));
    }
    let roots = match modeled(check_roots(&meta, pkg_dir, &req.packages, req.workspace))? {
        Ok(roots) => roots,
        Err(reason) => return Ok(Some(reason)),
    };
    if features::selected(&manifest, &roots, &meta, &extra, dev, home).is_none() {
        return Ok(Some(
            "per-invocation feature resolution is unavailable".into(),
        ));
    }
    Ok(None)
}

fn preflight(req: &ServeRequest, home: &Path) -> Result<std::result::Result<PathBuf, String>> {
    if let Some(reason) = environment_reason(req.op == "test") {
        return Ok(Err(reason));
    }
    let manifest = cargo::find_manifest(&req.dir)?;
    if let Some(reason) = below_min_cargo(home)? {
        return Ok(Err(reason));
    }
    let pkg_dir = manifest.parent().unwrap_or(&req.dir);
    match config_location_reason(pkg_dir)? {
        Some(reason) => Ok(Err(reason)),
        None => Ok(Ok(manifest)),
    }
}

fn environment_reason(doctest: bool) -> Option<String> {
    if let Some(name) = gated_env(doctest) {
        return Some(format!("{name} is set"));
    }
    key::explicit_rustc_identity()
        .is_none()
        .then(|| "RUSTC must be an absolute path or a program name on PATH".into())
}

fn config_location_reason(pkg_dir: &Path) -> Result<Option<String>> {
    let cargo_home = cargo::cargo_home();
    let found = |dir: &Path| -> Vec<std::path::PathBuf> {
        config::config_files(dir, &cargo_home)
            .iter()
            .map(|file| crate::resolve_path(file))
            .collect()
    };
    Ok(
        (found(&std::env::current_dir()?) != found(pkg_dir)).then(|| {
            "Cargo config from the current directory differs from the package directory's".into()
        }),
    )
}

fn manifest_reason(req: &ServeRequest, meta: &cargo::Metadata, pkg_dir: &Path) -> Option<String> {
    let ws = cargo::root(meta, pkg_dir);
    let profile = if req.release {
        "release"
    } else if req.op == "test" {
        "test"
    } else {
        "dev"
    };
    if let Err(reason) = manifest::overrides(ws, profile) {
        return Some(reason);
    }
    if let Err(reason) = manifest::profile_gate(ws, profile) {
        return Some(reason);
    }
    meta.packages
        .iter()
        .filter(|p| p.source.is_none())
        .find_map(|pkg| {
            let root = manifest::package_root(&pkg.manifest_path);
            manifest::lints(&pkg.manifest_path, &root).err()
        })
}

fn config_reason(doctest: bool, pkg_dir: &Path, home: &Path) -> Result<Option<String>> {
    let cfg = config::config(pkg_dir);
    if let Some(reason) = cfg.unmodeled.first() {
        return Ok(Some(format!("{reason} is not modeled")));
    }
    if doctest && let Some(reason) = cfg.unmodeled_doctest.first() {
        return Ok(Some(format!("{reason} is not modeled")));
    }
    if cfg.target_rustflags.is_empty() && cfg.target_tools.is_empty() {
        return Ok(None);
    }
    let print = key::rustc_print_cfg(home, pkg_dir)?;
    let version = key::rustc_version_in(home, pkg_dir)?;
    let host = key::rustc_host(&version)?;
    if let Err(reason) = config::resolve_target_flags(&cfg, &host, &print) {
        return Ok(Some(reason));
    }
    Ok(config::resolve_host_tools(&cfg.target_tools, &host, &print).err())
}

fn modeled<T>(result: Result<T>) -> Result<std::result::Result<T, String>> {
    match result {
        Ok(value) => Ok(Ok(value)),
        Err(error) => match model_fallback(&error) {
            Some(reason) => Ok(Err(reason)),
            None => Err(error),
        },
    }
}

fn model_fallback(error: &anyhow::Error) -> Option<String> {
    error
        .downcast_ref::<cargo::Unmodeled>()
        .map(|unmodeled| unmodeled.0.clone())
}

fn below_min_cargo(home: &Path) -> Result<Option<String>> {
    let version = crate::profile::span(crate::profile::SetupPhase::CargoVersion, || {
        cargo::cargo_version(home)
    })?;
    Ok(older_than(&version))
}

fn older_than(version: &str) -> Option<String> {
    const MIN: (u32, u32) = (1, 98);
    let unreadable = || format!("cargo --version is not modeled: {version}");
    let Some(number) = version.split_whitespace().nth(1) else {
        return Some(unreadable());
    };
    let mut parts = number.split('.');
    let (Some(major), Some(minor)) = (parts.next(), parts.next()) else {
        return Some(unreadable());
    };
    let (Ok(major), Ok(minor)) = (major.parse::<u32>(), minor.parse::<u32>()) else {
        return Some(unreadable());
    };
    ((major, minor) < MIN).then(|| format!("cargo {number} is older than 1.98"))
}

pub(crate) fn gated_name(names: impl Iterator<Item = String>, doctest: bool) -> Option<String> {
    const ALWAYS: [&str; 5] = [
        "CARGO_BUILD_RUSTC",
        "CARGO_BUILD_RUSTFLAGS",
        "CARGO_BUILD_TARGET",
        "CARGO_BUILD_TARGET_DIR",
        "CARGO_BUILD_BUILD_DIR",
    ];
    const DOCTEST: [&str; 3] = [
        "CARGO_BUILD_RUSTDOC",
        "CARGO_BUILD_RUSTDOCFLAGS",
        "CARGO_ENCODED_RUSTDOCFLAGS",
    ];
    for name in names {
        if name == "CARGO_TARGET_DIR" {
            continue;
        }
        let gated = ALWAYS.contains(&name.as_str())
            || (doctest && DOCTEST.contains(&name.as_str()))
            || name.starts_with("CARGO_PROFILE_")
            || name.starts_with("CARGO_UNSTABLE_")
            || name.starts_with("CARGO_TARGET_")
            || (name.starts_with("CARGO_") && !cargo_env_modeled(&name));
        if gated {
            return Some(name);
        }
    }
    None
}

fn cargo_env_modeled(name: &str) -> bool {
    const EXACT: [&str; 18] = [
        "CARGO",
        "CARGO_CACHE_RUSTC_INFO",
        "CARGO_CRATE_NAME",
        "CARGO_BIN_NAME",
        "CARGO_PRIMARY_PACKAGE",
        "CARGO_HOME",
        "CARGO_LOG",
        "CARGO_MAKEFLAGS",
        "CARGO_TARGET_DIR",
        "CARGO_TARGET_TMPDIR",
        "CARGO_BUILD_JOBS",
        "CARGO_BUILD_INCREMENTAL",
        "CARGO_BUILD_DEP_INFO_BASEDIR",
        "CARGO_ENCODED_RUSTFLAGS",
        "CARGO_INCREMENTAL",
        "CARGO_BUILD_RUSTDOC",
        "CARGO_BUILD_RUSTDOCFLAGS",
        "CARGO_ENCODED_RUSTDOCFLAGS",
    ];
    const PREFIXES: [&str; 13] = [
        "CARGO_PKG_",
        "CARGO_MANIFEST",
        "CARGO_BIN_EXE_",
        "CARGO_CFG_",
        "CARGO_FEATURE_",
        "CARGO_TERM_",
        "CARGO_NET_",
        "CARGO_HTTP_",
        "CARGO_REGISTRY",
        "CARGO_REGISTRIES_",
        "CARGO_ALIAS_",
        "CARGO_BUILD_RUSTC_WRAPPER",
        "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER",
    ];
    EXACT.contains(&name) || PREFIXES.iter().any(|p| name.starts_with(p))
}

pub(crate) fn gated_env(doctest: bool) -> Option<String> {
    gated_name(std::env::vars().map(|(name, _)| name), doctest)
}

#[cfg(test)]
#[path = "gate_tests.rs"]
mod tests;
