pub mod compile;
pub mod executor;
pub mod model;
pub mod util;
pub use executor::local_judge_task_handler;

pub const DEFAULT_PROGRAM_FILENAME: &str = "user-app";
