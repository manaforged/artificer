use super::*;

#[cfg(unix)]
#[test]
fn real_cargo_refuses_the_shim_through_a_symlink_chain() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let shim = tmp.path().join("shim");
    std::fs::write(&shim, "shim")?;
    let first = tmp.path().join("first");
    std::os::unix::fs::symlink(&shim, &first)?;
    let chained = tmp.path().join("cargo");
    std::os::unix::fs::symlink(&first, &chained)?;
    let real = tmp.path().join("real");
    std::fs::write(&real, "real")?;
    let shims = [shim];
    assert_eq!(
        resolve_real(&[chained.clone(), real.clone()], &shims)?,
        real
    );
    resolve_real(&[chained], &shims).unwrap_err();
    Ok(())
}
