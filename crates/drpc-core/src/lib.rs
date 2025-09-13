pub mod activity_registry;
pub mod detectables;
pub mod event;
pub mod frame;
pub mod metrics;
pub mod protocol;

pub use activity_registry::*;
pub use detectables::*;
pub use event::*;
pub use frame::*;
pub use protocol::*;

#[cfg(feature = "test-helpers")]
pub mod test_helpers {
    use super::ActivityRegistry;
    use std::sync::Arc;
    pub fn registry_snapshot(reg: &Arc<ActivityRegistry>) -> Vec<(String, serde_json::Value)> {
        reg.non_null()
    }
}
