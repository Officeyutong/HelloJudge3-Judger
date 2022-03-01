use std::{path::Path, sync::Arc};

use super::{
    executor::IntermediateValue,
    model::{ProblemTestcase, SubmissionTestcaseResult},
};
use crate::core::{
    compare::{Comparator, CompareResult},
    misc::ResultType,
};
use anyhow::anyhow;

pub async fn handle_submit_answer(
    testcase_result: &mut SubmissionTestcaseResult,
    testcase: &ProblemTestcase,
    this_problem_path: &Path,
    intermediate_value: &IntermediateValue,
    comparator: &dyn Comparator,
) -> ResultType<()> {
    testcase_result.memory_cost = 0;
    testcase_result.time_cost = 0;
    testcase_result.message = String::new();
    let input_file_name = &testcase.input;
    let output_file_name = &testcase.output;
    let input_data = tokio::fs::read(this_problem_path.join(input_file_name))
        .await
        .map_err(|e| anyhow!("Failed to read input file: {}", e))?;
    let output_data = tokio::fs::read(this_problem_path.join(output_file_name))
        .await
        .map_err(|e| anyhow!("Failed to read output file: {}", e))?;
    let files = intermediate_value.submit_answer().unwrap();
    let user_answer = files.get(output_file_name);
    if let Some(v) = user_answer {
        match comparator
            .compare(
                Arc::new(v.clone()),
                Arc::new(output_data),
                Arc::new(input_data),
                testcase.full_score,
            )
            .await
        {
            Ok(CompareResult { message, score }) => {
                testcase_result.score = score;
                if score < testcase.full_score {
                    testcase_result.status = "wrong_answer".to_string();
                } else if score == testcase.full_score {
                    testcase_result.status = "accepted".to_string();
                } else {
                    testcase_result.score = 0;
                    testcase_result.status = "judge_failed".to_string();
                    testcase_result.message = format!("Invalid score: {}", score);
                }
                testcase_result.message.push_str(&message);
            }
            Err(e) => {
                testcase_result.status = "judge_failed".to_string();
                testcase_result.score = 0;
                testcase_result.message.push_str(&e.to_string());
            }
        }
    } else {
        testcase_result.status = "wrong_answer".to_string();
        testcase_result.score = 0;
        testcase_result
            .message
            .push_str(&format!("Missing file: {}", output_file_name));
    }
    return Ok(());
}
