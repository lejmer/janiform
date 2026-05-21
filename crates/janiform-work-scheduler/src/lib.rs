#![forbid(unsafe_code)]

mod dynamic_pool;
mod static_pool;
mod streaming_pool;

pub use dynamic_pool::{
    DynamicWorkPoolInput, DynamicWorkPoolStatus, DynamicWorkPoolSummary, run_dynamic_work_pool,
};
pub use static_pool::{StaticWorkPoolInput, run_static_work_pool};
pub use streaming_pool::{
    StreamingWorkContext, StreamingWorkOutput, StreamingWorkPoolInput, StreamingWorkPoolSummary,
    StreamingWorkProgress, run_streaming_work_pool,
};
