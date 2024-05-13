use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_repr::Deserialize_repr;

use crate::{
    core::state::AppState,
    task::local::{
        model::{SubmissionJudgeResult, SubmissionSubtaskResult, SubmissionTestcaseResult},
        util::update_status,
    },
};

#[derive(Deserialize, Serialize, Debug)]
pub struct LuoguTrackData {
    pub submission_id: i64,
}
#[derive(Deserialize, Serialize)]
pub struct SimpleResponse {
    #[serde(rename = "requestId")]
    pub request_id: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct LuoguJudgeResponse {
    pub r#type: String,
    #[serde(rename = "requestId")]
    pub request_id: String,
    #[serde(rename = "trackId")]
    pub track_id: String,
    pub data: LuoguJudgeRecord,
}
impl LuoguJudgeResponse {
    // Return true if we should continue to track. Return false indicate the judging process is done
    pub async fn update_hj2_judge_status(
        &self,
        app: &AppState,
        submission_id: i64,
        extra_remote_data: Option<String>,
    ) -> bool {
        if let Some(ref judge_data) = self.data.judge {
            // 已经有运行结果了
            // 是waiting或者judging，代表还在评测
            let should_continue = matches!(
                judge_data.status,
                LuoguJudgeStatus::Judging | LuoguJudgeStatus::Waiting
            );
            let mut subtask_results = SubmissionJudgeResult::default();
            for (idx, subtask) in judge_data.subtasks.iter().enumerate() {
                subtask_results.insert(
                    format!("Subtask{:03}", idx + 1),
                    SubmissionSubtaskResult {
                        score: subtask.score as i64,
                        status: subtask.status.to_hj2_status().to_string(),
                        testcases: subtask
                            .cases
                            .iter()
                            .map(|v| SubmissionTestcaseResult {
                                input: "-".to_string(),
                                output: "-".to_string(),
                                memory_cost: v.memory * 1000,
                                message: v.description.clone().unwrap_or_default(),
                                status: v.status.to_hj2_status().to_string(),
                                time_cost: v.time as i64,
                                score: v.score as i64,
                                full_score: 0,
                            })
                            .collect(),
                    },
                );
            }
            let compile_message = if let Some(ref comp_data) = self.data.compile {
                format!("O2: {}\n{}", comp_data.opt2, comp_data.message)
            } else {
                String::default()
            };
            update_status(
                app,
                &subtask_results,
                &compile_message,
                if should_continue {
                    Some(judge_data.status.to_hj2_status())
                } else {
                    None
                },
                submission_id,
                extra_remote_data,
            )
            .await;
            should_continue
        } else {
            // 还没有运行
            if let Some(ref comp_data) = self.data.compile {
                // 编译完成了
                if comp_data.success {
                    // 编译成功
                    update_status(
                        app,
                        &BTreeMap::default(),
                        &format!("O2: {}\n{}", comp_data.opt2, comp_data.message),
                        Some("judging"),
                        submission_id,
                        extra_remote_data,
                    )
                    .await;
                    false
                } else {
                    // 编译失败
                    update_status(
                        app,
                        &BTreeMap::default(),
                        &format!("O2: {}, message: {}", comp_data.opt2, comp_data.message),
                        Some("compile_error"),
                        submission_id,
                        extra_remote_data,
                    )
                    .await;
                    true
                }
            } else {
                // 还没有编译
                update_status(
                    app,
                    &BTreeMap::default(),
                    "Waiting...",
                    Some("waiting"),
                    submission_id,
                    extra_remote_data,
                )
                .await;
                false
            }
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct LuoguJudgeRecord {
    pub compile: Option<LuoguCompileResult>,
    pub judge: Option<LuoguJudgeResult>,
}

#[derive(Deserialize_repr, Debug, Clone)]
#[repr(i32)]
pub enum LuoguJudgeStatus {
    Waiting = 0,
    Judging = 1,
    CompileError = 2,
    OutputLimitExceeded = 3,
    MemoryLimitExceeded = 4,
    TimeLimitExceeded = 5,
    WrongAnswer = 6,
    RuntimeError = 7,
    Invalid = 11,
    Accepted = 12,
    OverallUnaccepted = 14,
}

impl LuoguJudgeStatus {
    pub fn to_hj2_status(&self) -> &'static str {
        match self {
            LuoguJudgeStatus::Waiting => "waiting",
            LuoguJudgeStatus::Judging => "judging",
            LuoguJudgeStatus::CompileError => "compile_error",
            LuoguJudgeStatus::OutputLimitExceeded => "unaccepted",
            LuoguJudgeStatus::MemoryLimitExceeded => "memory_limit_exceed",
            LuoguJudgeStatus::TimeLimitExceeded => "time_limit_exceed",
            LuoguJudgeStatus::WrongAnswer => "wrong_answer",
            LuoguJudgeStatus::RuntimeError => "runtime_error",
            LuoguJudgeStatus::Invalid => "unknown",
            LuoguJudgeStatus::Accepted => "accepted",
            LuoguJudgeStatus::OverallUnaccepted => "unaccepted",
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct LuoguJudgeResult {
    pub id: i64,
    pub status: LuoguJudgeStatus,
    pub score: i32,
    // Time in millisecond
    pub time: i32,
    // Memory in KiB
    pub memory: i64,
    pub subtasks: Vec<LuoguSubtaskJudgeResult>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct LuoguTestcaseJudgeResult {
    pub id: i32,
    pub status: LuoguJudgeStatus,
    pub score: i32,
    // Time in milliseconds
    pub time: i32,
    // memory in KiB
    pub memory: i64,
    pub signal: i32,
    #[serde(rename = "exitCode")]
    pub exit_code: i32,
    pub description: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct LuoguSubtaskJudgeResult {
    pub id: i32,
    pub status: LuoguJudgeStatus,
    pub score: i32,
    // Time in milliseconds
    pub time: i32,
    // Memory in KiB
    pub memory: i64,
    pub cases: Vec<LuoguTestcaseJudgeResult>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct LuoguCompileResult {
    pub success: bool,
    pub message: String,
    pub opt2: bool,
}
