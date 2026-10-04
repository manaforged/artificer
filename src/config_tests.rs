use super::*;

#[test]
fn reads_build_rustflags_from_config() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    let dir = tmp.path().join("proj");
    std::fs::create_dir_all(dir.join(".cargo"))?;
    std::fs::write(
        dir.join(".cargo/config.toml"),
        "[build]\nrustflags = [\"--cfg\", \"viaconfig\"]\n",
    )?;
    let config = config(&dir);
    assert_eq!(config.rustflags, vec!["--cfg", "viaconfig"]);
    Ok(())
}

#[test]
fn cfg_evaluator_matches_rustc_semantics() {
    let mut host = BTreeMap::new();
    host.insert("target_os".to_string(), Some("macos".to_string()));
    host.insert("windows".to_string(), None);
    host.insert("target_env".to_string(), Some("msvc".to_string()));
    assert_eq!(eval_cfg("target_os = \"macos\"", &host), Some(true));
    assert_eq!(eval_cfg("target_os = \"linux\"", &host), Some(false));
    assert_eq!(eval_cfg("windows", &host), Some(true));
    assert_eq!(eval_cfg("unix", &host), Some(false));
    assert_eq!(eval_cfg("not(windows)", &host), Some(false));
    assert_eq!(
        eval_cfg("all(target_os = \"macos\", any(windows, not(unix)))", &host),
        Some(true)
    );
    assert_eq!(eval_cfg("postfix_macros", &host), Some(false));
}

#[test]
fn target_flags_replace_build_flags_on_a_match() {
    let config = Config {
        target_rustflags: vec![
            (
                "cfg(target_os = \"macos\")".into(),
                vec!["--cfg".into(), "hosthit".into()],
            ),
            (
                "cfg(all(windows, target_env = \"msvc\"))".into(),
                vec!["-C".into(), "link-arg=/MANIFEST:EMBED".into()],
            ),
        ],
        ..Default::default()
    };
    let print = vec![
        "target_os=\"macos\"".to_string(),
        "target_family=\"unix\"".to_string(),
    ];
    let hit = resolve_target_flags(&config, "aarch64-apple-darwin", &print)
        .expect("known target matchers");
    assert_eq!(hit, Some(vec!["--cfg".into(), "hosthit".into()]));
    let linux = vec![
        "target_os=\"linux\"".to_string(),
        "target_family=\"unix\"".to_string(),
        "target_env=\"gnu\"".to_string(),
    ];
    let none = resolve_target_flags(&config, "x86_64-unknown-linux-gnu", &linux)
        .expect("known target matchers");
    assert_eq!(none, None);
    let partial = Config {
        target_rustflags: vec![(
            "cfg(target_env = \"msvc\")".into(),
            vec!["--cfg".into(), "x".into()],
        )],
        ..Default::default()
    };
    resolve_target_flags(&partial, "aarch64-apple-darwin", &print).unwrap_err();
}

#[test]
fn config_names_unmodeled_settings_and_reads_wrappers() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    std::fs::create_dir_all(tmp.path().join(".cargo"))?;
    std::fs::write(
        tmp.path().join(".cargo/config.toml"),
        "[build]\nrustc-wrapper = \"sccache\"\nrustdocflags = [\"--cfg\", \"docflag\"]\n\n[profile.dev]\nopt-level = 1\n\n[target.aarch64-apple-darwin]\nlinker = \"clang\"\n",
    )?;
    let cfg = config(tmp.path());
    assert_eq!(cfg.rustc_wrapper.as_deref(), Some("sccache"));
    assert!(
        cfg.unmodeled.iter().any(|r| r.contains("[profile]")),
        "{:?}",
        cfg.unmodeled
    );
    assert!(
        cfg.target_tools
            .iter()
            .any(|t| t.matcher == "aarch64-apple-darwin" && t.kind == ToolKind::Linker),
        "{:?}",
        cfg.target_tools
    );
    assert!(
        cfg.unmodeled_doctest
            .iter()
            .any(|r| r.contains("rustdocflags")),
        "{:?}",
        cfg.unmodeled_doctest
    );
    Ok(())
}

#[test]
fn env_table_is_read_with_force_and_relative_paths() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    std::fs::create_dir_all(tmp.path().join(".cargo"))?;
    std::fs::write(
        tmp.path().join(".cargo/config.toml"),
        "[env]\nFOO = \"bar\"\nDATA = { value = \"assets\", relative = true, force = true }\n",
    )?;
    let cfg = config(tmp.path());
    assert!(cfg.unmodeled.is_empty(), "{:?}", cfg.unmodeled);
    let foo = cfg.env.iter().find(|v| v.name == "FOO").expect("FOO");
    assert_eq!((foo.value.as_str(), foo.force), ("bar", false));
    let data = cfg.env.iter().find(|v| v.name == "DATA").expect("DATA");
    assert_eq!(
        std::path::Path::new(&data.value),
        tmp.path().join("assets").as_path()
    );
    assert!(data.force);

    std::fs::write(
        tmp.path().join(".cargo/config.toml"),
        "[env]\nCARGO_HOME = \"/elsewhere\"\n",
    )?;
    assert!(
        config(tmp.path())
            .unmodeled
            .iter()
            .any(|r| r.contains("CARGO_HOME")),
        "Cargo refuses CARGO_HOME in [env]"
    );
    Ok(())
}

#[test]
fn unstable_table_is_unmodeled() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    std::fs::create_dir_all(tmp.path().join(".cargo"))?;
    std::fs::write(
        tmp.path().join(".cargo/config.toml"),
        "[unstable]\nsome-key = true\n",
    )?;
    let cfg = config(tmp.path());
    assert!(
        cfg.unmodeled.iter().any(|r| r.contains("[unstable]")),
        "{:?}",
        cfg.unmodeled
    );
    Ok(())
}

#[test]
fn home_config_is_discovered() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    let home = tmp.path().join("cargo-home");
    std::fs::create_dir_all(&home)?;
    std::fs::write(home.join("config.toml"), "[build]\nrustflags = []\n")?;
    let dir = tmp.path().join("proj");
    std::fs::create_dir_all(&dir)?;
    let files = config_files(&dir, &home);
    assert!(files.iter().any(|p| p == &home.join("config.toml")));
    Ok(())
}

#[test]
fn broken_config_is_unmodeled() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    std::fs::create_dir_all(tmp.path().join(".cargo"))?;
    std::fs::write(
        tmp.path().join(".cargo/config.toml"),
        "this is not = = toml\n",
    )?;
    let cfg = config(tmp.path());
    assert!(
        cfg.unmodeled.iter().any(|r| r.contains("cannot parse")),
        "{:?}",
        cfg.unmodeled
    );
    Ok(())
}

#[test]
fn unknown_config_surfaces_are_unmodeled() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    std::fs::create_dir_all(tmp.path().join(".cargo"))?;
    std::fs::write(
        tmp.path().join(".cargo/config.toml"),
        "[future-table]\nkey = 1\n\n[build]\nfuture-key = true\njobs = 4\n\n[target.aarch64-apple-darwin]\nfuture-key = true\n",
    )?;
    let cfg = config(tmp.path());
    for expected in [
        "[future-table]",
        "build.future-key",
        "target.aarch64-apple-darwin.future-key",
    ] {
        assert!(
            cfg.unmodeled.iter().any(|r| r.contains(expected)),
            "{expected} missing from {:?}",
            cfg.unmodeled
        );
    }
    assert!(
        !cfg.unmodeled.iter().any(|r| r.contains("build.jobs")),
        "jobs only paces work: {:?}",
        cfg.unmodeled
    );
    Ok(())
}

#[test]
fn a_linker_for_another_target_does_not_block_the_host() {
    let tool = |matcher: &str, kind: ToolKind, program: &str| TargetTool {
        matcher: matcher.to_string(),
        kind,
        command: vec![program.to_string()],
        listed: false,
    };
    let tools = vec![
        tool("aarch64-unknown-linux-musl", ToolKind::Linker, "musl-gcc"),
        tool("cfg(target_os = \"none\")", ToolKind::Runner, "qemu"),
    ];
    let print = ["target_os=\"linux\"".to_string()];
    let host = resolve_host_tools(&tools, "x86_64-unknown-linux-gnu", &print).unwrap();
    assert_eq!(host.linker, None);
    assert_eq!(host.runner, None);
    let musl = resolve_host_tools(&tools, "aarch64-unknown-linux-musl", &print).unwrap();
    assert_eq!(musl.linker.as_deref(), Some("musl-gcc"));
}
