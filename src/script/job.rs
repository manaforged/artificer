use super::directives::parse_print_cfg;
use crate::action::Key;
use crate::cargo::{Package, Target};
use crate::inputs::portable;
use crate::platform::env_path;
use crate::settings::Settings;
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

const BIN_CRATE: &str = "build_script_build";
const BIN_KEY_TAG: &[u8] = b"build-script-bin-v1";
const RUN_KEY_TAG: &[u8] = b"build-script-env-v9";

pub(super) struct Job<'a> {
    pub(super) pkg: &'a Package,
    pub(super) settings: &'a Settings,
    pub(super) script: &'a Target,
    pub(super) features: &'a [String],
    pub(super) externs: &'a [(String, PathBuf)],
    pub(super) search: &'a [PathBuf],
    pub(super) dep_env: &'a [(String, String)],
    pub(super) opt_level: &'a str,
    pub(super) debug: bool,
    pub(super) source_key: &'a str,
}

fn feed_sorted(key: &mut Key, mut items: Vec<String>) {
    items.sort();
    for item in &items {
        key.feed(item.as_bytes());
    }
}

impl Job<'_> {
    pub(super) fn bin_digest(&self) -> Result<String> {
        let mut key = Key::new();
        key.feed(BIN_KEY_TAG);
        self.feed_compile(&mut key)?;
        Ok(key.digest())
    }

    pub(super) fn digest(&self, bin_unit: &str) -> Result<String> {
        let settings = self.settings;
        let mut key = Key::new();
        key.feed(RUN_KEY_TAG);
        self.feed_compile(&mut key)?;
        key.feed_str(&std::env::var("RUSTDOC").unwrap_or_else(|_| "rustdoc".into()));
        feed_sorted(
            &mut key,
            self.dep_env
                .iter()
                .map(|(k, v)| format!("{k}={}", portable(&settings.home, v)))
                .collect(),
        );
        key.feed_str(bin_unit);
        key.feed(self.source_key.as_bytes());
        Ok(key.digest())
    }

    fn feed_compile(&self, key: &mut Key) -> Result<()> {
        let (pkg, settings) = (self.pkg, self.settings);
        let home = Some(settings.home.as_path());
        key.feed(&[u8::from(settings.release), u8::from(self.debug)]);
        key.feed_str(self.opt_level);
        key.feed_str(&crate::key::rustc_bin());
        for flag in &settings.rustflags {
            key.feed_str(flag);
        }
        key.feed_list(
            settings
                .host_linker
                .iter()
                .map(|a| portable(&settings.home, a)),
        );
        feed_sorted(key, settings.portable_env());
        for w in &settings.wrapper_chain(pkg) {
            key.feed(w.as_bytes());
        }
        key.feed(settings.rustc.as_bytes());
        key.feed_str(&crate::key::explicit_rustc_identity().unwrap_or_default());
        key.feed(pkg.name.as_bytes());
        key.feed(pkg.version.as_bytes());
        key.feed(pkg.source.as_deref().unwrap_or("path").as_bytes());
        feed_sorted(key, self.features.to_vec());
        if self.script.src_path.is_file() {
            key.feed_str(&crate::digest::file(home, &self.script.src_path)?);
        }
        feed_sorted(
            key,
            self.externs
                .iter()
                .map(|(n, p)| format!("{n}={}", portable(&settings.home, &p.display().to_string())))
                .collect(),
        );
        for (_, path) in self.externs {
            key.feed_str(&crate::digest::file(home, path)?);
        }
        self.feed_manifest(key);
        key.feed_str(&self.script.edition);
        Ok(())
    }

    fn feed_manifest(&self, key: &mut Key) {
        let pkg = self.pkg;
        key.feed(pkg.authors.join(":").as_bytes());
        for v in [
            pkg.description.as_deref().unwrap_or_default(),
            pkg.homepage.as_deref().unwrap_or_default(),
            pkg.license.as_deref().unwrap_or_default(),
            pkg.repository.as_deref().unwrap_or_default(),
        ] {
            key.feed(v.as_bytes());
        }
        key.feed(
            pkg.license_file
                .as_deref()
                .unwrap_or_else(|| Path::new(""))
                .to_string_lossy()
                .as_bytes(),
        );
    }

    pub(super) fn rustc_cmd(&self, bin_dir: &Path) -> Command {
        let (pkg, settings) = (self.pkg, self.settings);
        let mut cmd = settings.rustc_cmd(pkg);
        cmd.args([
            "--crate-name",
            BIN_CRATE,
            "--crate-type",
            "bin",
            "--edition",
            &self.script.edition,
            "--out-dir",
        ]);
        cmd.arg(bin_dir);
        cmd.arg("--emit=dep-info,link");
        cmd.args(&settings.rustflags);
        cmd.args(&settings.host_linker);
        cmd.env("CARGO_CRATE_NAME", BIN_CRATE);
        crate::cargo::set_package_env(&mut cmd, pkg);
        for feat in self.features {
            cmd.arg("--cfg").arg(format!("feature=\"{feat}\""));
        }
        if pkg.source.is_some() {
            cmd.arg("--cap-lints").arg("allow");
        }
        for (name, path) in self.externs {
            cmd.arg("--extern")
                .arg(format!("{name}={}", path.display()));
        }
        for dir in self.search {
            cmd.arg("-L").arg(format!("dependency={}", dir.display()));
        }
        cmd
    }

    pub(super) fn run(&self, bin_dir: &Path, out_dir: &Path) -> Result<String> {
        let bin = bin_dir.join(bin_file());
        let mut ran = self.run_cmd(&bin, out_dir)?;
        let ran = ran.output().context("run build.rs")?;
        if !ran.status.success() {
            bail!(run_failure(self.pkg, &bin, &ran));
        }
        let output = String::from_utf8(ran.stdout)?;
        for line in output.lines() {
            let Some(rest) = line
                .strip_prefix("cargo::")
                .or_else(|| line.strip_prefix("cargo:"))
            else {
                continue;
            };
            if let Some(msg) = rest.strip_prefix("warning=") {
                crate::out::err(format!("warning: {}: {msg}", self.pkg.name));
            }
            if let Some(msg) = rest.strip_prefix("error=") {
                bail!("{}: {msg}", self.pkg.name);
            }
        }
        Ok(output)
    }

    fn run_cmd(&self, bin: &Path, out_dir: &Path) -> Result<Command> {
        let (pkg, settings) = (self.pkg, self.settings);
        let host = &settings.host;
        let mut ran = Command::new(bin);
        settings.apply_env(&mut ran);
        ran.env("OUT_DIR", env_path(out_dir))
            .env("HOST", host)
            .env("TARGET", host)
            .env("RUSTC", crate::key::rustc_bin())
            .env(
                "RUSTDOC",
                std::env::var_os("RUSTDOC").unwrap_or_else(|| "rustdoc".into()),
            )
            .env("CARGO_ENCODED_RUSTFLAGS", settings.rustflags.join("\x1f"))
            .env("OPT_LEVEL", self.opt_level)
            .env("DEBUG", self.debug.to_string())
            .env(
                "PROFILE",
                if settings.release { "release" } else { "debug" },
            )
            .env("NUM_JOBS", crate::schedule::job_cap().to_string())
            .current_dir(pkg.root());
        crate::cargo::set_package_env(&mut ran, pkg);
        if let Some(links) = &pkg.links {
            ran.env("CARGO_MANIFEST_LINKS", links);
        }
        for (k, v) in self.dep_env {
            ran.env(k, v);
        }
        for feat in self.features {
            let key = format!("CARGO_FEATURE_{}", feat.to_uppercase().replace('-', "_"));
            ran.env(key, "1");
        }
        ran.env_remove("RUSTFLAGS");
        crate::jobs::isolate(&mut ran);
        let cfgs = crate::key::rustc_print_cfg_with_flags(
            &settings.home,
            &settings.toolchain_dir,
            &settings.rustflags,
        )?;
        for cfg in parse_print_cfg(&cfgs.join("\n"))? {
            ran.env(format!("CARGO_CFG_{}", cfg.0), cfg.1);
        }
        Ok(ran)
    }
}

pub(super) fn bin_file() -> String {
    format!("{BIN_CRATE}{}", std::env::consts::EXE_SUFFIX)
}

fn run_failure(pkg: &Package, bin: &Path, ran: &std::process::Output) -> String {
    let mut text = format!(
        "failed to run custom build command for `{}`\n\nCaused by:\n  process didn't exit successfully: `{}` ({})",
        crate::session::package_label(pkg),
        bin.display(),
        ran.status
    );
    for (name, bytes) in [("stdout", &ran.stdout), ("stderr", &ran.stderr)] {
        if bytes.is_empty() {
            continue;
        }
        text.push_str(&format!("\n  --- {name}"));
        for line in String::from_utf8_lossy(bytes).lines() {
            text.push_str(&format!("\n  {line}"));
        }
    }
    text
}
