use super::*;
use std::fs;

fn member(root: &Path, name: &str, extra: &str) -> Result<()> {
    let dir = root.join(name);
    fs::create_dir_all(dir.join("src"))?;
    fs::write(
        dir.join("Cargo.toml"),
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n{extra}"),
    )?;
    fs::write(dir.join("src/lib.rs"), "pub fn n() -> u8 { 1 }\n")?;
    Ok(())
}

#[test]
fn sibling_manifest_edit_invalidates_meta_cache() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let home = tmp.path().join("home");
    let root = tmp.path().join("ws");
    fs::create_dir_all(&root)?;
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"a\", \"b\"]\nresolver = \"2\"\n",
    )?;
    member(&root, "a", "")?;
    member(&root, "b", "")?;

    let manifest = root.join("a/Cargo.toml");
    let before = metadata_extra(&manifest, &[], &home)?;
    let package = before
        .packages
        .iter()
        .find(|package| package.name == "b")
        .expect("workspace member b");
    assert!(package.description.is_none());

    member(&root, "b", "description = \"edited\"\n")?;
    let after = metadata_extra(&manifest, &[], &home)?;
    let package = after
        .packages
        .iter()
        .find(|package| package.name == "b")
        .expect("workspace member b");
    assert_eq!(package.description.as_deref(), Some("edited"));
    Ok(())
}

#[test]
fn member_rename_invalidates_meta_cache() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let home = tmp.path().join("home");
    let root = tmp.path().join("ws");
    fs::create_dir_all(&root)?;
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"*\"]\nresolver = \"2\"\n",
    )?;
    member(&root, "a", "")?;
    member(&root, "b", "")?;

    let manifest = root.join("a/Cargo.toml");
    let before = metadata_extra(&manifest, &[], &home)?;
    let package = before
        .packages
        .iter()
        .find(|package| package.name == "b")
        .expect("workspace member b");
    assert!(package.manifest_path.ends_with("b/Cargo.toml"));

    fs::rename(root.join("b"), root.join("c"))?;
    let after = metadata_extra(&manifest, &[], &home)?;
    let package = after
        .packages
        .iter()
        .find(|package| package.name == "b")
        .expect("renamed workspace member b");
    assert!(package.manifest_path.ends_with("c/Cargo.toml"));
    Ok(())
}

#[test]
fn meta_key_ignores_checkout_location() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let one = tmp.path().join("one");
    let two = tmp.path().join("two");
    for dir in [&one, &two] {
        fs::create_dir_all(dir)?;
        fs::write(
            dir.join("Cargo.toml"),
            "[workspace]\nmembers = [\"a\"]\nresolver = \"2\"\n",
        )?;
        member(dir, "a", "")?;
    }
    let host = "aarch64-apple-darwin";
    let rustc = "rustc 1.97.0\nhost: aarch64-apple-darwin\n";
    let first = meta_key(&one.join("a/Cargo.toml"), &[], host, rustc)?;
    let second = meta_key(&two.join("a/Cargo.toml"), &[], host, rustc)?;
    assert_eq!(first, second);
    let changed = meta_key(
        &one.join("a/Cargo.toml"),
        &[],
        host,
        "rustc 1.97.1\nhost: aarch64-apple-darwin\n",
    )?;
    assert_ne!(first, changed, "metadata cache must include rustc version");
    Ok(())
}

#[test]
fn rustup_proxy_is_not_a_channel_pin() {
    let pinned = Path::new("rustup")
        .join("toolchains")
        .join("stable")
        .join("bin")
        .join(format!("cargo{}", std::env::consts::EXE_SUFFIX));
    assert!(pinned_toolchain_cargo(&pinned));
    let proxy = Path::new("cargo-home")
        .join("bin")
        .join(format!("cargo{}", std::env::consts::EXE_SUFFIX));
    assert!(!pinned_toolchain_cargo(&proxy));
    assert!(!pinned_toolchain_cargo(&rustup_cargo()));
}

#[test]
fn reroot_accepts_a_virtual_workspace_root() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let home = tmp.path().join("home");
    let root = tmp.path().join("ws1");
    fs::create_dir_all(root.join("a/src"))?;
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"a\"]\nresolver = \"2\"\n",
    )?;
    fs::write(
        root.join("a/Cargo.toml"),
        "[package]\nname = \"a\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::write(root.join("a/src/lib.rs"), "pub fn f() {}\n")?;
    let manifest = root.join("Cargo.toml");
    let first = metadata(&manifest, &home)?;
    assert!(!first.packages.is_empty());
    let mut cached = first.clone();
    assert!(reroot(&mut cached, &manifest));

    let two = tmp.path().join("ws2");
    fs::create_dir_all(two.join("a/src"))?;
    for file in ["Cargo.toml", "a/Cargo.toml", "a/src/lib.rs"] {
        fs::copy(root.join(file), two.join(file))?;
    }
    let mut moved = first;
    assert!(reroot(&mut moved, &two.join("Cargo.toml")));
    assert_eq!(moved.workspace_root, two);
    let defaults = moved.workspace_default_members.clone().unwrap_or_default();
    assert!(!defaults.is_empty());
    for id in &defaults {
        assert!(moved.packages.iter().any(|p| &p.id == id), "{id}");
    }
    Ok(())
}

#[test]
fn reroot_rewrites_paths_to_new_checkout() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let home = tmp.path().join("home");
    let root = tmp.path().join("ws1");
    fs::create_dir_all(&root)?;
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"a\", \"b\"]\nresolver = \"2\"\n",
    )?;
    member(&root, "a", "")?;
    member(&root, "b", "")?;
    let manifest = root.join("a/Cargo.toml");
    let meta = metadata_extra(&manifest, &[], &home)?;

    let other = tmp.path().join("ws2/a/Cargo.toml");
    let mut moved = meta;
    assert!(reroot(&mut moved, &other));
    assert_eq!(moved.workspace_root, tmp.path().join("ws2"));
    let package = moved
        .packages
        .iter()
        .find(|package| package.name == "b")
        .expect("workspace member b");
    assert!(package.manifest_path.starts_with(tmp.path().join("ws2")));
    let target = package.targets.first().expect("member library target");
    assert!(target.src_path.starts_with(tmp.path().join("ws2")));
    Ok(())
}

#[test]
fn reroot_refuses_a_graph_with_external_path_deps() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let root = tmp.path().join("ws");
    let package = |manifest_path: PathBuf| Package {
        name: "p".into(),
        version: "0.1.0".into(),
        id: format!("p {}", manifest_path.display()),
        authors: Vec::new(),
        description: None,
        homepage: None,
        license: None,
        license_file: None,
        repository: None,
        rust_version: None,
        source: None,
        manifest_path,
        targets: Vec::new(),
        default_run: None,
        links: None,
        declared: std::collections::BTreeMap::new(),
    };
    let mut meta = Metadata {
        packages: vec![
            package(root.join("a/Cargo.toml")),
            package(tmp.path().join("elsewhere/Cargo.toml")),
        ],
        resolve: None,
        workspace_members: Vec::new(),
        workspace_default_members: None,
        pkg_ix: OnceLock::new(),
        node_ix: OnceLock::new(),
        workspace_root: root.clone(),
    };
    assert!(!reroot(&mut meta, &root.join("a/Cargo.toml")));
    Ok(())
}

#[test]
fn required_features_deserialise_from_metadata() -> Result<()> {
    let json = serde_json::json!({
        "name": "gated",
        "kind": ["bin"],
        "crate_types": ["bin"],
        "src_path": "/tmp/x.rs",
        "edition": "2021",
        "required-features": ["window-feature"],
    });
    let target: Target = serde_json::from_value(json)?;
    assert_eq!(target.required_features, vec!["window-feature"]);
    assert!(!Package::covered(&target, &["default".into()]));
    assert!(Package::covered(&target, &["window-feature".into()]));
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_package_id_matches_through_another_spelling_of_its_path() {
    let tmp = tempfile::tempdir().unwrap();
    let real = tmp.path().join("real");
    std::fs::create_dir_all(real.join("pkg")).unwrap();
    let link = tmp.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let id =
        |root: &std::path::Path| format!("path+file://{}#pkg@0.1.0", root.join("pkg").display());
    assert!(same_package_id(&id(&real), &id(&link)));
    assert!(!same_package_id(
        &id(&real),
        &format!("path+file://{}#pkg@0.2.0", real.join("pkg").display())
    ));
}
