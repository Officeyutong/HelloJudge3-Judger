use crate::core::{
    misc::ResultType,
    runner::docker::execute_in_docker,
    state::{AppState, GLOBAL_APP_STATE},
    util::get_language_config,
};
use anyhow::anyhow;
use celery::{prelude::TaskError, task::TaskResult};
use log::info;
use tempfile::tempdir;
use tokio::io::AsyncReadExt;

use super::{model::ExtraIDERunConfig, util::update_ide_status};

#[celery::task(name = "judgers.ide_run.run")]
pub async fn online_ide_handler(
    lang_id: String,
    run_id: String,
    code: String,
    input: String,
    extra_config: ExtraIDERunConfig,
) -> TaskResult<()> {
    let guard = GLOBAL_APP_STATE.read().await;
    let app_state_guard = guard.as_ref().unwrap();
    let _semaphore_guard = app_state_guard.task_count_lock.acquire().await.unwrap();
    if let Err(e) = handle(
        lang_id,
        run_id.clone(),
        code,
        input,
        extra_config,
        app_state_guard,
    )
    .await
    {
        let err_str = e.to_string();
        update_ide_status(app_state_guard, &run_id, &err_str, "done").await;
        return Err(TaskError::UnexpectedError(err_str.clone()));
    }
    Ok(())
}
const IDE_RUN_PROG_NAME: &str = "iderun";
const IDE_RUN_INPUT: &str = "in";
const IDE_RUN_OUTPUT: &str = "out";

async fn handle(
    lang_id: String,
    run_id: String,
    code: String,
    input: String,
    extra_config: ExtraIDERunConfig,
    app: &AppState,
) -> ResultType<()> {
    info!("Received IDE run task: {}", run_id);
    info!("Extra config: {:#?}", extra_config);
    let http_client = reqwest::Client::builder()
        .pool_max_idle_per_host(0)
        .build()?;
    let work_dir = tempdir().map_err(|e| anyhow!("Failed to create temporary directory: {}", e))?;
    update_ide_status(
        app,
        &run_id,
        "Downloading language definitions..",
        "running",
    )
    .await;
    let lang_config = get_language_config(app, &lang_id, &http_client)
        .await
        .map_err(|e| anyhow!("Failed to get language definitions: {}", e))?;
    update_ide_status(app, &run_id, "Compiling..", "running").await;
    let app_source_file = lang_config.source(IDE_RUN_PROG_NAME);
    let app_output_file = lang_config.output(IDE_RUN_PROG_NAME);
    tokio::fs::write(work_dir.path().join(&app_source_file), &code)
        .await
        .map_err(|e| anyhow!("Failed to write code: {}", e))?;
    let compile_cmdline = vec![
        "sh".to_string(),
        "-c".to_string(),
        lang_config.compile_s(&app_source_file, &app_output_file, &extra_config.parameter),
    ];
    info!("Compile with: {:?}", compile_cmdline);
    let compile_result = execute_in_docker(
        &app.config.docker_image,
        work_dir.path().to_str().unwrap(),
        &compile_cmdline,
        extra_config.memory_limit * 1024 * 1024,
        extra_config.time_limit * 1000,
        extra_config.compile_result_length_limit as usize,
    )
    .await
    .map_err(|e| anyhow!("Failed to compile: {}", e))?;
    info!("Compile result: {:#?}", compile_result);
    if compile_result.exit_code != 0 {
        update_ide_status(
            app,
            &run_id,
            &format!(
                "编译失败！\n{}{}时间占用: {}ms\n内存占用: {}KB\n退出代码: {}",
                compile_result.output,
                if compile_result.output_truncated {
                    "[已截断]"
                } else {
                    ""
                },
                compile_result.time_cost / 1000,
                compile_result.memory_cost / 1024,
                compile_result.exit_code
            ),
            "done",
        )
        .await;
        return Ok(());
    }
    tokio::fs::write(work_dir.path().join(IDE_RUN_INPUT), &input)
        .await
        .map_err(|e| anyhow!("Failed to write user input: {}", e))?;
    update_ide_status(app, &run_id, "Running..", "running").await;
    let run_cmdline = vec![
        "sh".to_string(),
        "-c".to_string(),
        lang_config.run_s(
            &app_output_file,
            &format!("< {} > {}", IDE_RUN_INPUT, IDE_RUN_OUTPUT),
        ),
    ];
    info!("Run with: {:?}", run_cmdline);
    let run_result = execute_in_docker(
        &app.config.docker_image,
        work_dir.path().to_str().unwrap(),
        &run_cmdline,
        extra_config.memory_limit * 1024 * 1024,
        extra_config.time_limit * 1000,
        extra_config.result_length_limit as usize,
    )
    .await
    .map_err(|e| anyhow!("Failed to run: {}", e))?;
    let app_stdout = {
        let mut file = tokio::fs::File::open(work_dir.path().join(IDE_RUN_OUTPUT))
            .await
            .map_err(|e| anyhow!("Failed to open output file: {}", e))?;
        let mut buf = vec![0; extra_config.result_length_limit as usize];
        let sread = file
            .read(&mut buf[..])
            .await
            .map_err(|e| anyhow!("Failed to read result: {}", e))?;
        buf.resize(sread, 0);
        String::from_utf8(buf).map_err(|e| anyhow!("Illegal utf8 char!: {}", e))?
    };
    let app_stderr = run_result.output;
    update_ide_status(
        app,
        &run_id,
        &format!(
            "运行完成！\n退出代码: {}\n\
    内存占用: {} KB\n时间占用: {} ms\n标准输出: {}\n标准错误: {}\n",
            run_result.exit_code,
            run_result.memory_cost / 1024,
            run_result.time_cost / 1000,
            app_stdout,
            app_stderr
        ),
        "done",
    )
    .await;
    info!("Task done: {}", run_id);
    Ok(())
}
