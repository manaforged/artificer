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
    let glam = &map[&("glam".to_string(), "0.32.1".to_string())];
    assert_eq!(glam, &["bytemuck", "default", "serde", "std"]);
    let serde = &map[&("serde_core".to_string(), "1.0.228".to_string())];
    assert!(serde.is_empty());
    let bincode = &map[&("bincode".to_string(), "2.0.1".to_string())];
    assert_eq!(bincode, &["alloc", "std"]);
    assert!(
        map.values()
            .flatten()
            .all(|feature| !feature.contains("(*)"))
    );
}
