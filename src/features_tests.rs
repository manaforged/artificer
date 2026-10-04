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
    let glam = &map[&key("glam", "0.32.1")];
    assert_eq!(glam, &["bytemuck", "default", "serde", "std"]);
    let serde = &map[&key("serde_core", "1.0.228")];
    assert!(serde.is_empty());
    let bincode = &map[&key("bincode", "2.0.1")];
    assert_eq!(bincode, &["alloc", "std"]);
    assert!(
        map.values()
            .flatten()
            .all(|feature| !feature.contains("(*)"))
    );
}
