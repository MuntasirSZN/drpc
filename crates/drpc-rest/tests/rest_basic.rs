use reqwest::Client;

#[tokio::test]
async fn rest_activity_and_metrics() {
    // Spin minimal components: build a bus + registry and rest server directly
    let bus = drpc_core::EventBus::new();
    let registry = drpc_core::ActivityRegistry::new();
    let port = drpc_rest::run_rest(bus.clone(), registry.clone(), None, 24, 0).await.expect("rest");
    let base = format!("http://127.0.0.1:{}", port);
    let client = Client::new();
    // Post activity
    let resp = client.post(format!("{}/activities", base))
        .json(&serde_json::json!({"activity":{"name":"RestGame"}}))
        .send().await.unwrap()
        .json::<serde_json::Value>().await.unwrap();
    let sid = resp["socket_id"].as_str().unwrap().to_string();
    // List
    let list = client.get(format!("{}/activities", base)).send().await.unwrap().json::<serde_json::Value>().await.unwrap();
    assert!(list["activities"].as_array().unwrap().iter().any(|e| e[1]["name"]=="RestGame"));
    // Metrics
    let _ = client.get(format!("{}/metrics", base)).send().await.unwrap();
    // Privacy allow-only mismatch should block new name
    client.post(format!("{}/privacy", base))
        .json(&serde_json::json!({"allow":["Alpha"],"deny":[]}))
        .send().await.unwrap();
    client.post(format!("{}/activities", base))
        .json(&serde_json::json!({"activity":{"name":"BlockedGame"}}))
        .send().await.unwrap();
    let list2 = client.get(format!("{}/activities", base)).send().await.unwrap().json::<serde_json::Value>().await.unwrap();
    assert!(!list2["activities"].as_array().unwrap().iter().any(|e| e[1]["name"]=="BlockedGame"));
    // Clear
    client.delete(format!("{}/activities/{}", base, sid)).send().await.unwrap();
}

