use std::{path::Path, sync::Arc};

use log::{error, info};
use tokio::io::AsyncReadExt;

use crate::{
    core::{
        compare::{Comparator, CompareResult},
        misc::ResultType,
        model::LanguageConfig,
        runner::docker::execute_in_docker,
        state::AppState,
    },
    task::local::DEFAULT_PROGRAM_FILENAME,
};

use super::model::{
    ExtraJudgeConfig, ProblemInfo, ProblemSubtask, ProblemTestcase, SubmissionJudgeResult,
};
use anyhow::anyhow;
#[inline]
pub async fn handle_traditional(
    problem_data: &ProblemInfo,
    this_problem_path: &Path,
    working_dir_path: &Path,
    testcase: &ProblemTestcase,
    subtask: &ProblemSubtask,
    time_scale: f64,
    lang_config: &LanguageConfig,
    app: &AppState,
    comparator: &dyn Comparator,
    extra_config: &ExtraJudgeConfig,
    i: usize,
    will_skip: &mut bool,
    judge_result: &mut SubmissionJudgeResult,
) -> ResultType<()> {
    let input_file = if problem_data.using_file_io == 1 {
        problem_data.input_file_name.as_str()
    } else {
        "in"
    };
    let output_file = if problem_data.using_file_io == 1 {
        problem_data.output_file_name.as_str()
    } else {
        "out"
    };
    info!("Input file: {}, output file: {}", input_file, output_file);
    tokio::fs::copy(
        this_problem_path.join(&testcase.input),
        working_dir_path.join(input_file),
    )
    .await
    .map_err(|e| anyhow!("Failed to copy input file: {}", e))?;
    let scaled_time = (subtask.time_limit as f64 * time_scale) as i64;
    let execute_cmdline = lang_config.run_s(
        &lang_config.output(DEFAULT_PROGRAM_FILENAME),
        &(if problem_data.using_file_io == 1 {
            "".to_string()
        } else {
            format!("< {} > {}", input_file, output_file)
        }),
    );
    info!("Run command line: {}", execute_cmdline);
    let run_result = execute_in_docker(
        &app.config.docker_image,
        working_dir_path.to_str().unwrap(),
        &vec!["sh".to_string(), "-c".to_string(), execute_cmdline],
        subtask.memory_limit * 1024 * 1024,
        scaled_time * 1000,
        1000,
    )
    .await
    .map_err(|e| anyhow!("Fatal error: {}", e))?;
    info!("Run result:\n{:#?}", run_result);
    {
        let testcase_result = &mut judge_result.get_mut(&subtask.name).unwrap().testcases[i];
        testcase_result.memory_cost = run_result.memory_cost;
        testcase_result.time_cost = (run_result.time_cost as f64 / 1000.0).ceil() as i64;
        if run_result.memory_cost / 1024 / 1024 >= subtask.memory_limit {
            testcase_result.update_status("memory_limit_exceed");
        } else if run_result.time_cost >= scaled_time * 1000 {
            testcase_result.update_status("time_limit_exceed");
        } else if run_result.exit_code != 0 {
            testcase_result.update(
                "runtime_error",
                &format!("退出代码: {}", run_result.exit_code),
            );
        } else {
            let user_out = match tokio::fs::File::open(working_dir_path.join(output_file)).await {
                Ok(mut f) => match f.metadata().await {
                    Ok(d) => {
                        if d.len() > extra_config.output_file_size_limit as u64 {
                            testcase_result.update("output_size_limit_exceed", "输出文件过大");
                            return Ok(());
                        }
                        let mut v: Vec<u8> = vec![];
                        match f.read_to_end(&mut v).await {
                            Ok(_) => v,
                            Err(_) => vec![],
                        }
                    }
                    Err(e) => {
                        error!("Failed to get metadata: {}", e);
                        vec![]
                    }
                },
                Err(e) => {
                    error!("Failed to open output file: {}", e);
                    vec![]
                }
            };
            let full_score = testcase.full_score;
            let input_data = tokio::fs::read(this_problem_path.join(&testcase.input))
                .await
                .map_err(|e| anyhow!("Failed to read input data: {}, {}", testcase.input, e))?;
            let answer_data = tokio::fs::read(this_problem_path.join(&testcase.output))
                .await
                .map_err(|e| anyhow!("Failed to read answer data: {}, {}", testcase.output, e))?;
            let CompareResult { score, message } = match comparator
                .compare(
                    Arc::new(user_out),
                    Arc::new(answer_data),
                    Arc::new(input_data),
                    full_score,
                )
                .await
            {
                Ok(v) => v,
                Err(e) => CompareResult {
                    score: 0,
                    message: e.to_string(),
                },
            };
            if score < full_score {
                testcase_result.update_status("wrong_answer");
            } else if score == full_score {
                testcase_result.update_status("accepted");
            } else {
                testcase_result.update("unaccepted", &format!("Illegal score: {}", score));
            }
            testcase_result.score = score;
            testcase_result.message = message;
        }
        if testcase_result.status != "accepted" && subtask.method == "min" {
            *will_skip = true;
        }
    }
    Ok(())
}
