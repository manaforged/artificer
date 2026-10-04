use super::*;

fn pass(name: &str, ms: u64) -> Pass {
    Pass {
        name: name.to_string(),
        us: ms * 1000,
    }
}

#[test]
fn monomorphization_is_taken_out_of_the_pass_that_contains_it() {
    let passes = [
        pass("parse_crate", 5),
        pass("monomorphization_collector_graph_walk", 182),
        pass("generate_crate_metadata", 223),
        pass("total", 1259),
    ];
    assert_eq!(
        categorize(&passes),
        [
            (Category::Parse, 5_000),
            (Category::Mono, 182_000),
            (Category::Metadata, 41_000),
            (Category::Other, 1_031_000),
        ]
    );
}

#[test]
fn harvest_keeps_diagnostics_and_strips_every_timing_line() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("diagnostics");
    let body = concat!(
        "{\"$message_type\":\"diagnostic\",\"message\":\"one\"}\n",
        "time: {\"pass\":\"drop_ast\",\"time\":0.000001,\"rss_start\":1,\"rss_end\":2}\n",
        "time: {\"pass\":\"type_check_crate\",\"time\":0.486,\"rss_start\":1,\"rss_end\":2}\n",
        "{\"$message_type\":\"diagnostic\",\"message\":\"two\"}\n",
        "time: {\"pass\":\"total\",\"time\":1.259,\"rss_start\":1,\"rss_end\":2}\n",
    );
    std::fs::write(&path, body).unwrap();
    let passes = harvest(&path).unwrap();
    assert_eq!(passes, [pass("type_check_crate", 486), pass("total", 1259)]);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        concat!(
            "{\"$message_type\":\"diagnostic\",\"message\":\"one\"}\n",
            "{\"$message_type\":\"diagnostic\",\"message\":\"two\"}\n",
        )
    );
}
