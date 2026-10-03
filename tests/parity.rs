use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[path = "parity/release.rs"]
mod release;

#[path = "parity/adversarial.rs"]
mod adversarial;

fn rustup_tool(name: &str) -> PathBuf {
    let output = Command::new("rustup")
        .args(["which", name])
        .output()
        .expect("run rustup which");
    assert!(
        output.status.success(),
        "rustup could not resolve {name}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let path = PathBuf::from(
        String::from_utf8(output.stdout)
            .expect("rustup path is UTF-8")
            .trim(),
    );
    assert!(
        path.is_file(),
        "rustup returned a missing {name}: {}",
        path.display()
    );
    path
}

fn stock_cargo() -> PathBuf {
    rustup_tool("cargo")
}

struct Run {
    code: Option<i32>,
    stdout: String,
}

impl Run {
    fn ok(&self) -> bool {
        self.code == Some(0)
    }
}

struct Project {
    dir: PathBuf,
    home: PathBuf,
    stock_target: PathBuf,
    _tmp: tempfile::TempDir,
}

impl Project {
    fn new(files: &[(&str, &str)]) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("proj");
        for (rel, body) in files {
            let path = dir.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, body).unwrap();
        }
        let config = dir.join(".cargo/config.toml");
        if !config.exists() {
            fs::create_dir_all(config.parent().unwrap()).unwrap();
            fs::write(&config, "[build]\ntarget-dir = \"target\"\n").unwrap();
        }
        let st = Command::new(stock_cargo())
            .args(["generate-lockfile", "--manifest-path"])
            .arg(dir.join("Cargo.toml"))
            .status()
            .unwrap();
        assert!(st.success(), "fixture must resolve");
        Self {
            home: tmp.path().join("artificer-home"),
            stock_target: tmp.path().join("stock-target"),
            dir,
            _tmp: tmp,
        }
    }

    fn artificer(&self, args: &[&str], env: &[(&str, &str)]) -> Run {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_artificer"));
        cmd.env("ARTIFICER_HOME", &self.home)
            .env("ARTIFICER_NOSERVE", "1")
            .env_remove("CARGO_TARGET_DIR")
            .current_dir(&self.dir)
            .args(args);
        for (k, v) in env {
            cmd.env(k, v);
        }
        let out = cmd.output().unwrap();
        Run {
            code: out.status.code(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        }
    }

    fn stock(&self, args: &[&str], env: &[(&str, &str)]) -> Run {
        let mut cmd = Command::new(stock_cargo());
        cmd.env("CARGO_TARGET_DIR", &self.stock_target)
            .current_dir(&self.dir)
            .args(args);
        for (k, v) in env {
            cmd.env(k, v);
        }
        let out = cmd.output().unwrap();
        Run {
            code: out.status.code(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        }
    }

    fn parity(&self, args: &[&str], env: &[(&str, &str)]) {
        let f = self.artificer(args, env);
        let s = self.stock(args, env);
        assert_eq!(
            f.code,
            s.code,
            "`{}` {env:?}: artificer exit {:?}, cargo exit {:?}",
            args.join(" "),
            f.code,
            s.code,
        );
    }

    fn parity_run(&self, env: &[(&str, &str)]) {
        let f = self.artificer(&["run"], env);
        let s = self.stock(&["run", "-q"], env);
        assert!(
            s.ok(),
            "fixture must run under cargo: {:?} {}",
            s.code,
            s.stdout
        );
        assert!(
            f.ok(),
            "fixture must run under artificer: {:?} {}",
            f.code,
            f.stdout
        );
        assert_eq!(
            f.stdout.trim(),
            s.stdout.trim(),
            "program output differs {env:?}",
        );
    }

    fn artificer_served(&self, args: &[&str], env: &[(&str, &str)]) -> Run {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_artificer"));
        cmd.env("ARTIFICER_HOME", &self.home)
            .env_remove("ARTIFICER_NOSERVE")
            .env_remove("CARGO_TARGET_DIR")
            .current_dir(&self.dir)
            .args(args);
        for (k, v) in env {
            cmd.env(k, v);
        }
        let out = cmd.output().unwrap();
        Run {
            code: out.status.code(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.dir.join(rel)
    }
}

fn manifest(name: &str, extra: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n{extra}")
}

fn nothing_compiled(log: &str) -> bool {
    log.split_whitespace()
        .zip(log.split_whitespace().skip(1))
        .any(|(a, b)| a == "0" && b.starts_with("rustc"))
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap().flatten() {
        let dst = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &dst);
        } else {
            fs::copy(entry.path(), dst).unwrap();
        }
    }
}

#[path = "parity/configuration.rs"]
mod configuration;

#[path = "parity/scripts.rs"]
mod scripts;

#[path = "parity/inputs.rs"]
mod inputs;

#[path = "parity/profiles.rs"]
mod profiles;

#[path = "parity/outputs.rs"]
mod outputs;

#[cfg(unix)]
#[path = "parity/native.rs"]
mod native;

#[path = "parity/cap.rs"]
mod cap;
