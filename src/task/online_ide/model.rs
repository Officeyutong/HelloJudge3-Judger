use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ExtraIDERunConfig {
    pub compile_time_limit: i64,
    pub compile_result_length_limit: i64,
    //milliseconds
    pub time_limit: i64,
    pub memory_limit: i64,
    pub result_length_limit: i64,
    pub parameter: String,
}
