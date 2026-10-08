use super::*;

const ROOT: &str = "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nanswer = { path = \"answer\" }\ncsum-sys = { path = \"csum-sys\" }\n\n[workspace]\nmembers = [\"answer\", \"csum-sys\"]\n";

const MACRO_TOML: &str = "[package]\nname = \"answer\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\nproc-macro = true\n";

const MACRO_LIB: &str = "use proc_macro::TokenStream;\n\n#[proc_macro]\npub fn answer(_: TokenStream) -> TokenStream {\n    \"41u32\".parse().unwrap()\n}\n";

const SYS_TOML: &str = "[package]\nname = \"csum-sys\"\nversion = \"0.1.0\"\nedition = \"2021\"\nlinks = \"csum\"\nbuild = \"build.rs\"\n";

const SYS_BUILD: &str = "use std::{env, path::PathBuf, process::Command};\n\nfn main() {\n    println!(\"cargo::rerun-if-changed=src/csum.c\");\n    let out = PathBuf::from(env::var(\"OUT_DIR\").unwrap());\n    let cc = env::var(\"CC\").unwrap_or_else(|_| \"cc\".into());\n    let object = out.join(\"csum.o\");\n    assert!(Command::new(cc).args([\"-c\", \"-fPIC\", \"src/csum.c\", \"-o\"]).arg(&object).status().unwrap().success());\n    assert!(Command::new(\"ar\").arg(\"crs\").arg(out.join(\"libcsum.a\")).arg(&object).status().unwrap().success());\n    println!(\"cargo::rustc-link-search=native={}\", out.display());\n    println!(\"cargo::rustc-link-lib=static=csum\");\n}\n";

const SYS_C: &str = "unsigned int csum(unsigned int a, unsigned int b) { return a + b; }\n";

const SYS_LIB: &str = "extern \"C\" {\n    pub fn csum(a: u32, b: u32) -> u32;\n}\n";

const MAIN: &str = "fn main() {\n    let total = unsafe { csum_sys::csum(answer::answer!(), 1) };\n    println!(\"native {total}\");\n}\n";

#[test]
fn a_workspace_with_a_proc_macro_and_a_native_sys_crate_runs_like_cargo() {
    let p = Project::new(&[
        ("Cargo.toml", ROOT),
        ("src/main.rs", MAIN),
        ("answer/Cargo.toml", MACRO_TOML),
        ("answer/src/lib.rs", MACRO_LIB),
        ("csum-sys/Cargo.toml", SYS_TOML),
        ("csum-sys/build.rs", SYS_BUILD),
        ("csum-sys/src/csum.c", SYS_C),
        ("csum-sys/src/lib.rs", SYS_LIB),
    ]);
    p.parity_run(&[]);
    let out = p.artificer(&["run"], &[]);
    assert!(out.stdout.contains("native 42"), "{}", out.stdout);
}

const DOC_LIB: &str = "extern \"C\" {\n    pub fn csum(a: u32, b: u32) -> u32;\n}\n\n/// ```\n/// assert_eq!(csum_sys::add(41, 1), 42);\n/// ```\npub fn add(a: u32, b: u32) -> u32 {\n    unsafe { csum(a, b) }\n}\n";

#[test]
fn a_doctest_of_a_native_sys_crate_runs_like_cargo() {
    let p = Project::new(&[
        ("Cargo.toml", SYS_TOML),
        ("build.rs", SYS_BUILD),
        ("src/csum.c", SYS_C),
        ("src/lib.rs", DOC_LIB),
    ]);
    p.parity(&["test", "--doc"], &[]);
}
