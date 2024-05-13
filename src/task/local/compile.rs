use std::path::Path;

use crate::{
    core::{
        misc::ResultType,
        model::LanguageConfig,
        runner::docker::{execute_in_docker, ExecuteResult},
        state::AppState,
    },
    task::local::{model::SubmissionJudgeResult, util::update_status, DEFAULT_PROGRAM_FILENAME},
};

use super::model::{ExtraJudgeConfig, ProblemInfo, SubmissionInfo};
use anyhow::anyhow;
use log::{error, info};
pub struct CompileResult {
    pub execute_result: ExecuteResult,
    pub compile_error: bool,
}
pub async fn compile_program(
    app: &AppState,
    working_dir: &Path,
    sid: i64,
    sub_info: &SubmissionInfo,
    lang_config: &LanguageConfig,
    problem_data: &ProblemInfo,
    this_problem_path: &Path,
    extra_config: &ExtraJudgeConfig,
    default_status: &SubmissionJudgeResult,
) -> ResultType<CompileResult> {
    update_status(
        app,
        &sub_info.judge_result,
        "Compiling your program..",
        None,
        sid,
        None,
    )
    .await;
    let app_source_file_name = lang_config.source(DEFAULT_PROGRAM_FILENAME);
    let app_output_file_name = lang_config.output(DEFAULT_PROGRAM_FILENAME);
    tokio::fs::write(working_dir.join(&app_source_file_name), &sub_info.code)
        .await
        .map_err(|e| anyhow!("Failed to write code: {}", e))?;
    for file in problem_data.provides.iter() {
        tokio::fs::copy(this_problem_path.join(file), working_dir.join(file))
            .await
            .map_err(|e| anyhow!("Failed to copy compile-time provided file: {}, {}", file, e))?;
    }
    let compile_cmdline = lang_config
        .compile_s(
            &app_source_file_name,
            &app_output_file_name,
            &extra_config.extra_compile_parameter,
        )
        .split_ascii_whitespace()
        .map(|v| v.to_string())
        .collect::<Vec<String>>();
    info!("Compiling user program: {:?}", compile_cmdline);
    let execute_result = execute_in_docker(
        &app.config.docker_image,
        working_dir.to_str().ok_or(anyhow!("?"))?,
        &compile_cmdline,
        2048 * 1024 * 1024,
        extra_config.compile_time_limit * 1000,
        extra_config.compile_result_length_limit as usize,
    )
    .await
    .map_err(|e| anyhow!("Failed to compile your program: {}", e))?;
    info!("Compile result:\n{:#?}", execute_result);
    if execute_result.exit_code != 0 {
        update_status(
            app,
            &SubmissionJudgeResult::default(),
            &format!(
                "{}{}\nTime usage: {} ms\nMemory usage: {} bytes\nExit code: {}",
                execute_result.output,
                if execute_result.output_truncated {
                    "[Truncated]"
                } else {
                    ""
                },
                execute_result.time_cost / 1000,
                execute_result.memory_cost,
                execute_result.exit_code
            ),
            Some("compile_error"),
            sid,
            None,
        )
        .await;
        error!("Failed to compile!\n{}", execute_result.output);
        return Ok(CompileResult {
            compile_error: true,
            execute_result,
        });
    } else {
        update_status(app, default_status, "Compile successfully", None, sid, None).await;
    }

    Ok(CompileResult {
        compile_error: false,
        execute_result,
    })
}
