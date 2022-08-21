use std::sync::Arc;

use async_trait::async_trait;

use super::{Comparator, CompareResult};
use crate::core::misc::ResultType;
use anyhow::anyhow;

pub struct SimpleLineComparator;
#[async_trait]
impl Comparator for SimpleLineComparator {
    async fn compare(
        &self,
        user_out: Arc<Vec<u8>>,
        answer: Arc<Vec<u8>>,
        _input_data: Arc<Vec<u8>>,
        full_score: i64,
    ) -> ResultType<CompareResult> {
        let resp = tokio::task::spawn_blocking(move || compare(&user_out, &answer, full_score))
            .await
            .map_err(|e| anyhow!("Failed to compare: {}", e))?;
        return resp;
    }
}
fn compare(user_out: &[u8], answer: &[u8], full_score: i64) -> ResultType<CompareResult> {
    let t1 =
        String::from_utf8(user_out.into()).map_err(|e| anyhow!("Failed to decode chars: {}", e))?;
    let t2 =
        String::from_utf8(answer.into()).map_err(|e| anyhow!("Failed to decode chars: {}", e))?;
    let mut user_lines = t1.split("\n").collect::<Vec<&str>>();
    let mut answer_lines = t2.split("\n").collect::<Vec<&str>>();
    while !user_lines.is_empty() && user_lines.last().unwrap().trim_end() == "" {
        user_lines.pop();
    }
    while !answer_lines.is_empty() && answer_lines.last().unwrap().trim_end() == "" {
        answer_lines.pop();
    }
    if user_lines.len() != answer_lines.len() {
        return Ok(CompareResult {
            message: format!(
                "Expected {} lines, received {} lines",
                answer_lines.len(),
                user_lines.len()
            ),
            score: 0,
        });
    }
    for (i, (user, answer)) in user_lines
        .into_iter()
        .zip(answer_lines.into_iter())
        .enumerate()
    {
        if user.trim_end() != answer.trim_end() {
            return Ok(CompareResult {
                message: format!("Different at line {}.", i+1),
                score: 0,
            });
        }
    }
    return Ok(CompareResult {
        message: "OK!".to_string(),
        score: full_score,
    });
}
