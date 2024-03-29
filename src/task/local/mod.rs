pub mod compile;
pub mod dependency;
pub mod executor;
pub mod model;
pub mod submit_answer;
pub mod traditional;
pub mod util;
pub use executor::local_judge_task_handler;

pub const DEFAULT_PROGRAM_FILENAME: &str = "user-app";
