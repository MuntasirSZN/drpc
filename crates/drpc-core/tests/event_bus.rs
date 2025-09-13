use drpc_core::{EventBus, EventKind};
use serde_json::json;

#[test]
fn bus_publish_subscribe_and_metrics() {
    let bus = EventBus::new();
    let mut rx1 = bus.subscribe();
    let mut rx2 = bus.subscribe();
    bus.publish(EventKind::ActivityUpdate { socket_id: "s1".into(), payload: json!({"name":"X"}) });
    let e1 = rx1.try_recv().expect("rx1 evt");
    let e2 = rx2.try_recv().expect("rx2 evt");
    match (e1, e2) {
        (EventKind::ActivityUpdate { socket_id, .. }, EventKind::ActivityUpdate { socket_id: socket_id2, .. }) => {
            assert_eq!(socket_id, "s1");
            assert_eq!(socket_id2, "s1");
        }
        _ => panic!("unexpected events"),
    }
    assert!(drpc_core::metrics::ACTIVITIES_SET.load(std::sync::atomic::Ordering::Relaxed) >= 1);
}

