use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Deserialize, Debug, Clone, Serialize)]
pub struct ExtraJudgeConfig {
    //ms
    pub compile_time_limit: i64,
    //chars
    pub compile_result_length_limit: i64,
    //ms
    pub spj_execute_time_limit: i64,
    pub extra_compile_parameter: String,
    pub auto_sync_files: bool,
    // bytes
    pub output_file_size_limit: i64,
    pub submit_answer: bool,
    // in base64
    pub answer_data: Option<String>,
    pub time_scale: Option<f64>,
}
#[derive(Deserialize, Debug, Clone, Serialize)]
pub struct SubmissionInfo {
    pub code: String,
    pub contest_id: i64,
    pub extra_compile_parameter: String,
    pub id: i64,
    pub judger: String,
    pub language: String,
    pub memory_cost: i64,
    pub message: String,
    pub problem_id: i64,
    pub problemset_id: i64,
    pub public: i8,
    pub score: i64,
    pub selected_compile_parameters: Vec<i64>,
    pub status: String,
    pub submit_time: String,
    pub time_cost: i64,
    pub uid: i64,
    pub virtual_contest_id: Option<i64>,
    pub judge_result: SubmissionJudgeResult,
}

pub type SubmissionJudgeResult = BTreeMap<String, SubmissionSubtaskResult>;
#[derive(Deserialize, Debug, Clone, Serialize)]
pub struct SubmissionTestcaseResult {
    pub full_score: i64,
    pub input: String,
    pub memory_cost: i64,
    pub message: String,
    pub output: String,
    pub score: i64,
    pub status: String,
    pub time_cost: i64,
}
impl SubmissionTestcaseResult {
    pub fn update(&mut self, status: &str, message: &str) {
        self.status = status.to_string();
        self.message = message.to_string();
    }
    pub fn update_status(&mut self, status: &str) {
        self.status = status.to_string();
    }
}
#[derive(Deserialize, Debug, Clone, Serialize)]
pub struct SubmissionSubtaskResult {
    pub score: i64,
    pub status: String,
    pub testcases: Vec<SubmissionTestcaseResult>,
}

#[derive(Deserialize, Debug, Clone, Serialize)]
pub struct ProblemInfo {
    pub files: Vec<ProblemFile>,
    pub id: i64,
    pub input_file_name: String,
    pub output_file_name: String,
    pub problem_type: String,
    pub provides: Vec<String>,
    pub remote_judge_oj: Option<String>,
    pub remote_problem_id: Option<String>,
    pub spj_filename: String,
    pub using_file_io: i8,
    pub subtasks: Vec<ProblemSubtask>,
}
#[derive(Deserialize, Debug, Clone, Serialize)]
pub struct ProblemFile {
    pub name: String,
    pub size: i64,
}
#[derive(Deserialize, Debug, Clone, Serialize)]
pub struct ProblemTestcase {
    pub full_score: i64,
    pub input: String,
    pub output: String,
}
#[derive(Deserialize, Debug, Clone, Serialize)]
pub struct ProblemSubtask {
    pub time_limit: i64,
    pub memory_limit: i64,
    pub method: String,
    pub name: String,
    pub score: i64,
    pub testcases: Vec<ProblemTestcase>,
}
