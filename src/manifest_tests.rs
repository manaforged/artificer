use super::*;

#[test]
fn named_override_beats_wildcard_and_wildcard_needs_source() {
    let overrides = Overrides {
        named: vec![("dep".into(), vec!["-C".into(), "opt-level=2".into()])],
        wildcard: Some(vec!["-C".into(), "opt-level=3".into()]),
    };
    assert_eq!(overrides.for_package("dep", true), &["-C", "opt-level=2"]);
    assert_eq!(overrides.for_package("other", true), &["-C", "opt-level=3"]);
    assert!(overrides.for_package("other", false).is_empty());
}

#[test]
fn unmodelable_override_key_is_rejected_not_dropped() {
    let table: toml::Value = toml::from_str("opt-level = 3\nfrobnicate = true").expect("fixture");
    decode_override(&table).expect_err("unknown key");
    let valid: toml::Value =
        toml::from_str("opt-level = 3\ndebug = false\nstrip = \"symbols\"").expect("fixture");
    decode_override(&valid).expect("known keys");
}

#[test]
fn build_override_anywhere_in_the_chain_rejects() {
    let tmp = tempfile::tempdir().expect("tmp");
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        "[profile.custom]\ninherits = \"dev\"\n\n[profile.custom.build-override]\nopt-level = 0\n",
    )
    .expect("write");
    overrides(tmp.path(), "custom").expect_err("build-override");
    overrides(tmp.path(), "dev").expect("dev profile");
}

fn workspace(body: &str) -> anyhow::Result<tempfile::TempDir> {
    let tmp = tempfile::tempdir()?;
    std::fs::write(tmp.path().join("Cargo.toml"), body)?;
    Ok(tmp)
}

#[test]
fn profile_gate_rejects_unknown_keys_and_walks_the_chain() {
    let tmp = workspace(
        "[profile.custom]\ninherits = \"dev\"\n\n[profile.dev]\nsplit-debuginfo = \"packed\"\nrpath = true\nincremental = false\n",
    )
    .expect("fixture");
    profile_gate(tmp.path(), "custom").expect("modeled keys");
    profile_gate(tmp.path(), "dev").expect("modeled keys");

    let unknown = workspace("[profile.dev]\ntrim-paths = \"macro\"\n").expect("fixture");
    let err = profile_gate(unknown.path(), "dev").expect_err("unknown key");
    assert!(err.contains("trim-paths"), "{err}");

    let inherited = workspace(
        "[profile.custom]\ninherits = \"release\"\n\n[profile.release]\nopt-level = 3\nfrobnicate = true\n",
    )
    .expect("fixture");
    let err = profile_gate(inherited.path(), "custom").expect_err("unknown key in ancestor");
    assert!(err.contains("frobnicate"), "{err}");
}

#[test]
fn profile_applies_split_debuginfo_and_rpath() -> anyhow::Result<()> {
    let tmp = workspace("[profile.dev]\nsplit-debuginfo = \"packed\"\nrpath = true\n")?;
    let args = profile(tmp.path(), "dev", UnitUse::Runtime);
    let pairs: Vec<&str> = args.iter().map(String::as_str).collect();
    assert!(
        pairs
            .windows(2)
            .any(|w| w == ["-C", "split-debuginfo=packed"]),
        "{args:?}"
    );
    assert!(pairs.windows(2).any(|w| w == ["-C", "rpath"]), "{args:?}");
    Ok(())
}

#[test]
fn lints_gate_rejects_an_unknown_level() {
    let tmp = tempfile::tempdir().expect("fixture");
    let pkg = tmp.path().join("pkg");
    std::fs::create_dir_all(&pkg).expect("fixture");
    let manifest = pkg.join("Cargo.toml");
    std::fs::write(
        &manifest,
        "[package]\nname = \"p\"\nversion = \"0.1.0\"\n\n[lints.rust]\nunused = \"deny\"\n",
    )
    .expect("fixture");
    lints(&manifest, tmp.path()).expect("known level");

    std::fs::write(
        &manifest,
        "[package]\nname = \"p\"\nversion = \"0.1.0\"\n\n[lints.rust]\nunused = \"explode\"\n",
    )
    .expect("fixture");
    let err = lints(&manifest, tmp.path()).expect_err("unknown level");
    assert!(err.contains("explode"), "{err}");
}

#[test]
fn test_and_bench_inherit_their_implicit_parents() {
    let tmp = workspace("[profile.dev]\nopt-level = 1\n\n[profile.release]\nopt-level = 2\n")
        .expect("fixture");
    let test = profile(tmp.path(), "test", UnitUse::Runtime);
    assert!(
        test.windows(2).any(|w| w == ["-C", "opt-level=1"]),
        "{test:?}"
    );
    let bench = profile(tmp.path(), "bench", UnitUse::Runtime);
    assert!(
        bench.windows(2).any(|w| w == ["-C", "opt-level=2"]),
        "{bench:?}"
    );
}

#[test]
fn gate_covers_the_implicit_test_parent() {
    let tmp = workspace("[profile.dev]\ntrim-paths = \"macro\"\n").expect("fixture");
    let err = profile_gate(tmp.path(), "test").expect_err("unknown key in dev");
    assert!(err.contains("trim-paths"), "{err}");
}

#[test]
fn overrides_reach_the_implicit_test_parent() {
    let tmp = workspace("[profile.dev.package.\"*\"]\nopt-level = 2\n").expect("fixture");
    let ov = overrides(tmp.path(), "test").expect("profile chain");
    assert_eq!(
        ov.wildcard,
        Some(vec!["-C".to_string(), "opt-level=2".to_string()])
    );
}

#[test]
fn package_root_finds_the_owning_workspace() {
    let tmp = tempfile::tempdir().expect("fixture");
    let ws = tmp.path().join("ws");
    let dep = ws.join("dep");
    std::fs::create_dir_all(&dep).expect("fixture");
    std::fs::write(
        ws.join("Cargo.toml"),
        "[workspace]\nmembers = [\"dep\"]\n\n[workspace.lints.rust]\nunsafe_code = \"forbid\"\n",
    )
    .expect("fixture");
    std::fs::write(
        dep.join("Cargo.toml"),
        "[package]\nname = \"dep\"\nversion = \"0.1.0\"\n\n[lints]\nworkspace = true\n",
    )
    .expect("fixture");

    let root = package_root(&dep.join("Cargo.toml"));
    assert_eq!(root, ws);
    let args = lints(&dep.join("Cargo.toml"), &root).expect("valid lints");
    assert!(
        args.windows(2).any(|w| w == ["--forbid", "unsafe_code"]),
        "{args:?}"
    );

    let lone = tmp.path().join("lone");
    std::fs::create_dir_all(&lone).expect("fixture");
    std::fs::write(
        lone.join("Cargo.toml"),
        "[package]\nname = \"lone\"\nversion = \"0.1.0\"\n",
    )
    .expect("fixture");
    assert_eq!(package_root(&lone.join("Cargo.toml")), lone);
}

#[test]
fn profile_booleans_map_to_rustc_values() -> anyhow::Result<()> {
    let tmp = workspace(
        "[profile.release]\nstrip = true\ndebug = true\nlto = false\n\n[profile.dev]\nstrip = false\nlto = true\ndebug = \"limited\"\n",
    )?;
    let release = profile(tmp.path(), "release", UnitUse::Runtime);
    assert!(release.contains(&"strip=symbols".to_string()));
    assert!(release.contains(&"debuginfo=2".to_string()));
    assert!(!release.iter().any(|a| a.starts_with("lto")));
    let dev = profile(tmp.path(), "dev", UnitUse::Runtime);
    assert!(dev.contains(&"strip=none".to_string()));
    assert!(dev.contains(&"lto=fat".to_string()));
    assert!(dev.contains(&"debuginfo=1".to_string()));
    let table: toml::Value = toml::from_str("strip = true\ndebug = false")?;
    assert_eq!(
        decode_override(&table).map_err(anyhow::Error::msg)?,
        ["-C", "debuginfo=0", "-C", "strip=symbols"]
    );
    Ok(())
}

#[test]
fn a_test_target_with_harness_false_reads_as_harness_free() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = dir.path().join("Cargo.toml");
    std::fs::write(
        &manifest,
        "[package]\nname = \"h\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[[test]]\nname = \"custom\"\nharness = false\n",
    )
    .unwrap();
    let target = |name: &str, kind: &str| crate::cargo::Target {
        name: name.to_string(),
        kind: vec![kind.to_string()],
        crate_types: vec!["bin".to_string()],
        src_path: dir.path().join("tests/custom.rs"),
        edition: "2021".to_string(),
        required_features: Vec::new(),
        test: true,
        doc: false,
        doctest: false,
    };
    assert!(!harness(&manifest, &target("custom", "test")));
    assert!(harness(&manifest, &target("other", "test")));
    assert!(harness(&manifest, &target("h", "lib")));
}
