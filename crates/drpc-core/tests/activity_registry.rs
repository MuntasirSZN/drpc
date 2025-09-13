use drpc_core::ActivityRegistry;
use serde_json::json;

#[test]
fn registry_set_and_clear_and_non_null() {
    let reg = ActivityRegistry::new();
    reg.set("sock1", json!({"name":"A"}));
    reg.set("sock2", json!({"name":"B"}));
    let snap = reg.snapshot();
    assert_eq!(snap.len(), 2);
    assert!(snap["sock1"]["name"].as_str().unwrap() == "A");
    // clear one
    reg.clear("sock1");
    let nn = reg.non_null();
    assert_eq!(nn.len(), 1);
    assert_eq!(nn[0].0, "sock2");
}
