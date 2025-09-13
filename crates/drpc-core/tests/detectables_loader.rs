use std::sync::OnceLock;

static ENV_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

#[tokio::test]
async fn ttl_and_fresh_load_behavior() {
    let _g = ENV_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;
    let base = std::env::temp_dir().join(format!(
        "drpc-detectables-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(base.join(".drpc")).unwrap();

    #[cfg(windows)]
    unsafe {
        std::env::set_var("USERPROFILE", &base);
    }

    #[cfg(not(windows))]
    unsafe {
        std::env::set_var("HOME", &base);
    }

    let path = base.join(".drpc").join("detectables.json");
    let json = r#"[
        {"id":"app1","name":"App1","executables":[{"name":"app1.exe","is_launcher":false}]}
    ]"#;
    std::fs::write(&path, json).unwrap();

    // Fresh load should parse existing file
    // Ensure offline to avoid any network fetch regardless of feature flags
    unsafe {
        std::env::set_var("DRPC_OFFLINE", "1");
    }
    let d = drpc_core::load_detectables_async(false, 1000)
        .await
        .unwrap();
    let list = d.list();
    assert_eq!(list.len(), 1);

    // After some time and TTL=0, should be considered stale.
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    let d2 = drpc_core::load_detectables_async(false, 0).await.unwrap();
    let list2 = d2.list();
    // With network feature disabled, stale path returns empty set
    assert_eq!(list2.len(), 0);
}

#[tokio::test]
async fn parse_fallback_on_invalid_json() {
    let _g = ENV_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;
    let base = std::env::temp_dir().join(format!(
        "drpc-detectables-invalid-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(base.join(".drpc")).unwrap();

    #[cfg(windows)]
    unsafe {
        std::env::set_var("USERPROFILE", &base);
    }

    #[cfg(not(windows))]
    unsafe {
        std::env::set_var("HOME", &base);
    }

    let path = base.join(".drpc").join("detectables.json");
    std::fs::write(&path, "not-json").unwrap();

    // Should fall back to empty list when parse fails; ensure offline to avoid network provider needs
    unsafe {
        std::env::set_var("DRPC_OFFLINE", "1");
    }
    let d = drpc_core::load_detectables_async(false, 1000)
        .await
        .unwrap();
    let list = d.list();
    assert_eq!(list.len(), 0);
}
