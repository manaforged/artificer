use super::*;
use serde_json::Value;
use std::error::Error;
use std::process::Output;

type FeatureSets = Vec<(String, Vec<String>)>;

fn fixture(root: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let project = root.join("project");
    for (name, content) in [
        (
            "dep/Cargo.toml",
            "[package]\nname='split_dep'\nversion='0.1.0'\nedition='2021'\n[features]\nextra=[]\n",
        ),
        (
            "dep/src/lib.rs",
            "pub fn enabled() -> bool { cfg!(feature = \"extra\") }\n",
        ),
        (
            "project/Cargo.toml",
            "[package]\nname='split_project'\nversion='0.1.0'\nedition='2021'\n[workspace]\nresolver='2'\n[dependencies]\nsplit_dep={path='../dep'}\n[build-dependencies]\nsplit_dep={path='../dep',features=['extra']}\n",
        ),
        (
            "project/build.rs",
            "fn main() { assert!(split_dep::enabled()); }\n",
        ),
        (
            "project/src/lib.rs",
            "pub fn enabled() -> bool { split_dep::enabled() }\n",
        ),
    ] {
        let path = root.join(name);
        fs::create_dir_all(path.parent().ok_or("fixture path has no parent")?)?;
        fs::write(path, content)?;
    }
    Ok(project)
}

fn artifacts(output: Output) -> Result<FeatureSets, Box<dyn Error>> {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut found = Vec::new();
    for line in String::from_utf8(output.stdout)?.lines() {
        let row: Value = serde_json::from_str(line)?;
        if row["reason"] == "compiler-artifact" && row["target"]["name"] == "split_dep" {
            found.push((
                row["package_id"]
                    .as_str()
                    .ok_or("missing package ID")?
                    .to_owned(),
                serde_json::from_value(row["features"].clone())?,
            ));
        }
    }
    found.sort();
    Ok(found)
}

#[test]
fn json_artifact_retains_split_host_features_and_cargo_ids() -> Result<(), Box<dyn Error>> {
    let tmp = tempfile::tempdir()?;
    let root = fixture(tmp.path())?;
    let direct = artifacts(
        stock(&root)
            .args(["check", "--message-format=json"])
            .output()?,
    )?;
    assert_eq!(direct.len(), 2);
    assert_eq!(direct[0].0, direct[1].0);
    assert_eq!(direct[0].1, Vec::<String>::new());
    assert_eq!(direct[1].1, ["extra"]);
    let home = tmp.path().join("home");
    let wrapped = artifacts(
        artificer(&home, &root)
            .args(["check", "--message-format=json"])
            .output()?,
    )?;
    assert_eq!(wrapped, direct);
    let restored = artifacts(
        artificer(&home, &root)
            .args(["check", "--message-format=json"])
            .output()?,
    )?;
    assert_eq!(restored, direct);
    Ok(())
}
