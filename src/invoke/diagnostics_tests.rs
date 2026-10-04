use super::*;

fn notice(emit: &str, artifact: &str) -> Vec<u8> {
    format!("{{\"$message_type\":\"artifact\",\"artifact\":\"{artifact}\",\"emit\":\"{emit}\"}}")
        .into_bytes()
}

#[test]
fn artifact_notices_tell_early_metadata_from_metadata() {
    assert_eq!(
        artifact_notice(&notice("early-metadata", "out/libx-1.early.rmeta")),
        Some(Emitted::EarlyMetadata(PathBuf::from(
            "out/libx-1.early.rmeta"
        )))
    );
    assert_eq!(
        artifact_notice(&notice("metadata", "out/libx-1.rmeta")),
        Some(Emitted::Metadata(PathBuf::from("out/libx-1.rmeta")))
    );
    assert_eq!(
        artifact_notice(&notice("link", "out/libx-1.rlib")),
        Some(Emitted::Other)
    );
    assert_eq!(artifact_notice(b"{\"$message_type\":\"diagnostic\"}"), None);
}
