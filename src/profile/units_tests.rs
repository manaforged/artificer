use super::*;
use crate::profile::capture;

#[test]
fn early_and_full_metadata_marks_are_kept_apart() {
    let (marks, part) = capture(None, || {
        let index = plan(vec![PlannedUnit {
            package: "dep".into(),
            name: "dep".into(),
            version: "0.1.0".into(),
            role: Role::Package,
            links: false,
            early: true,
            deps: Vec::new(),
        }]);
        unit(index, || {
            let mark = meta_mark().ok_or(())?;
            mark.mark(MetaStage::Early);
            std::thread::sleep(std::time::Duration::from_millis(2));
            mark.mark(MetaStage::Full);
            mark.mark(MetaStage::Early);
            Ok::<_, ()>(())
        })
    });
    assert_eq!(marks, Ok(()));
    let unit = &part.units[0];
    let (early, full) = (unit.early_us.unwrap(), unit.meta_us.unwrap());
    assert!(early < full, "early {early} full {full}");
    assert!(unit.early);
}
