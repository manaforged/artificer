use super::*;

#[test]
fn parse_tree_drops_the_dedup_marker_and_unions_contexts() {
    let text = "\
glam v0.32.1 (registry)|bytemuck,default,serde,std (*)
glam v0.32.1 (registry)|bytemuck,std
serde_core v1.0.228 (registry)| (*)
bincode v2.0.1 (registry)|alloc,std (*)

";
    let map = parse_tree(text);
    let key = |name: &str, version: &str| TreePkg {
        name: name.to_string(),
        version: version.to_string(),
        source: TreeSource::Other("registry".to_string()),
    };
    let glam = &map[&(key("glam", "0.32.1"), Side::Normal)];
    assert_eq!(glam, &["bytemuck", "default", "serde", "std"]);
    let serde = &map[&(key("serde_core", "1.0.228"), Side::Normal)];
    assert!(serde.is_empty());
    let bincode = &map[&(key("bincode", "2.0.1"), Side::Normal)];
    assert_eq!(bincode, &["alloc", "std"]);
    assert!(
        map.values()
            .flatten()
            .all(|feature| !feature.contains("(*)"))
    );
}

#[test]
fn a_subtree_cargo_prints_once_reaches_the_other_side_without_its_features() {
    let text = "\
app v0.1.0 (/w)|
├── base v0.1.0 (/w/base)|extra
└── mid v0.1.0 (/w/mid)|
    ├── base v0.1.0 (/w/base)|extra
    └── leaf v0.1.0 (/w/leaf)|
        └── base v0.1.0 (/w/base)|extra
[build-dependencies]
├── base v0.1.0 (/w/base)|
└── mid v0.1.0 (/w/mid)| (*)
";
    let map = parse_tree(text);
    let key = |name: &str| TreePkg {
        name: name.to_string(),
        version: "0.1.0".to_string(),
        source: TreeSource::Path(std::path::PathBuf::from(format!("/w/{name}"))),
    };
    assert_eq!(map.get(&(key("leaf"), Side::Host)), Some(&Vec::new()));
    assert_eq!(map.get(&(key("base"), Side::Host)), Some(&Vec::new()));
    assert_eq!(
        map.get(&(key("base"), Side::Normal)),
        Some(&vec!["extra".to_string()])
    );
}
