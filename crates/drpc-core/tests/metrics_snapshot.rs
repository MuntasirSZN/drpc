use drpc_core::metrics;

#[test]
fn metrics_snapshot_increments() {
    metrics::ACTIVE_CONNECTIONS.fetch_add(2, std::sync::atomic::Ordering::Relaxed);
    metrics::PROCESSES_DETECTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    metrics::DETECTABLES_COUNT.store(5, std::sync::atomic::Ordering::Relaxed);
    let snap = metrics::snapshot();
    assert_eq!(snap["active_connections"].as_u64().unwrap(), 2);
    assert_eq!(snap["processes_detected"].as_u64().unwrap(), 1);
    assert_eq!(snap["detectables_count"].as_u64().unwrap(), 5);
}

