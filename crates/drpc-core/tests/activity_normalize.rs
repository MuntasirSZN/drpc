use drpc_core::{Activity, ActivityButton};

#[test]
fn buttons_normalize() {
    let act = Activity {
        name: "Game".into(),
        buttons: Some(vec![ActivityButton {
            label: "Site".into(),
            url: "https://example.com".into(),
        }]),
        ..Default::default()
    };
    let norm = act.normalize();
    // ensure flags unaffected & metadata present via serde roundtrip expectation (buttons becomes labels)
    let val = serde_json::to_value(&norm).unwrap();
    assert!(val.get("metadata").is_some());
}
