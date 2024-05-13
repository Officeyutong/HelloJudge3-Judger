use serde::{Deserialize, Serialize};

#[derive(Deserialize, Debug, Clone, Serialize)]
pub struct RemoteJudgeConfig {
    pub luogu_openapp_secret: String,
    pub luogu_openapp_id: String,
    pub luogu_delay_sequence: Vec<usize>,
    pub problem_id: i64,
    pub remote_judge_oj: String,
    pub remote_problem_id: String,
    pub code: String,
    pub submission_id: i64,
    pub language: String,
    pub extra_arguments: String,
    pub extra_information_by_remote_judge: String,
}
