pub mod ancestry;
pub mod capsule_service;
pub mod change_service;
pub mod client;
pub mod error;
pub mod event_service;
pub mod intent_service;
pub mod negotiation;
pub mod partial_clone;
pub mod server;
pub mod workstream_service;

pub mod proto {
    pub mod common {
        tonic::include_proto!("claw.common");
    }
    pub mod objects {
        tonic::include_proto!("claw.objects");
    }
    pub mod sync {
        tonic::include_proto!("claw.sync");
    }
    pub mod intent {
        tonic::include_proto!("claw.intent");
    }
    pub mod change {
        tonic::include_proto!("claw.change");
    }
    pub mod capsule {
        tonic::include_proto!("claw.capsule");
    }
    pub mod workstream {
        tonic::include_proto!("claw.workstream");
    }
    pub mod event {
        tonic::include_proto!("claw.event");
    }
}

pub use error::SyncError;
