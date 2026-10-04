use super::{EARLY_DIR, meta_file};
use crate::session::Artifact;
use anyhow::Result;
use std::fs;
use std::path::Path;

fn artifact(path: &Path, rmeta: Option<&Path>) -> Artifact {
    Artifact {
        crate_name: "dep".into(),
        path: path.to_path_buf(),
        rmeta: rmeta.map(Path::to_path_buf),
        proc_macro: false,
    }
}

#[test]
fn early_consumers_read_the_same_metadata_while_built_and_once_restored() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let out = tmp.path();
    let full = out.join("libdep-abc.rmeta");
    let early = out.join(EARLY_DIR).join("libdep-abc.rmeta");
    fs::create_dir_all(out.join(EARLY_DIR))?;
    fs::write(&full, "full")?;
    fs::write(&early, "early")?;
    let rlib = out.join("libdep-abc.rlib");
    let building = artifact(&rlib, Some(&full));
    let restored_lib = artifact(&rlib, Some(&full));
    let restored_check = artifact(&full, None);
    for art in [&building, &restored_lib, &restored_check] {
        assert_eq!(meta_file(art, true), early);
        assert_eq!(meta_file(art, false), full);
    }
    fs::remove_file(&early)?;
    assert_eq!(meta_file(&restored_check, true), full);
    Ok(())
}
