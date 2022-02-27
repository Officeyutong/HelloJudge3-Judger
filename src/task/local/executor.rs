use std::{collections::BTreeMap, path::PathBuf};

use celery::{prelude::TaskError, task::TaskResult};
use lazy_static::lazy_static;
use log::{debug, info};
use regex::Regex;
use serde_json::Value;

use crate::{
    core::{
        compare::{simple::SimpleLineComparator, special::SpecialJudgeComparator, Comparator},
        misc::ResultType,
        state::{AppState, GLOBAL_APP_STATE},
        util::get_language_config,
    },
    task::local::{
        compile::compile_program,
        model::SubmissionInfo,
        util::{get_problem_data, sync_problem_files},
    },
};

use super::{
    model::{ExtraJudgeConfig, SubmissionJudgeResult},
    util::{update_status, AsyncStatusUpdater},
};
use anyhow::anyhow;
#[celery::task(name = "judgers.local.run")]
pub async fn local_judge_task_handler(
    submission_data: Value,
    extra_config: ExtraJudgeConfig,
) -> TaskResult<()> {
    let guard = GLOBAL_APP_STATE.read().await;
    let app_state_guard = guard.as_ref().unwrap();
    let sid = submission_data.pointer("/id").unwrap().as_i64().unwrap();
    if let Err(e) = handle(submission_data, extra_config, app_state_guard).await {
        let err_str = format!("{}", e,);
        update_status(app_state_guard, &BTreeMap::new(), &err_str, None, sid).await;
        return Err(TaskError::UnexpectedError(err_str.clone()));
    }
    return Ok(());
}

async fn handle(
    submission_info: Value,
    extra_config: ExtraJudgeConfig,
    app: &AppState,
) -> ResultType<()> {
    debug!("Raw task:\n{:#?}", submission_info);
    let sub_info = serde_json::from_value::<SubmissionInfo>(submission_info)
        .map_err(|e| anyhow!("Failed to deserialize submission info: {}", e))?;
    info!("Received judge task:\n{:#?}", sub_info);
    let http_client = reqwest::Client::new();
    let problem_data = get_problem_data(&http_client, app, &sub_info).await?;
    debug!("Problem info:\n{:#?}", problem_data);
    let this_problem_path = app.testdata_dir.join(problem_data.id.to_string());
    let sid = sub_info.id.clone();
    if extra_config.auto_sync_files {
        sync_problem_files(
            problem_data.id.clone(),
            &MyUpdater {
                judge_result: &sub_info.judge_result,
                submission_id: sub_info.id.clone(),
            },
            &http_client,
            app,
        )
        .await
        .map_err(|e| anyhow!("Error occurred when syncing problem files:\n{}", e))?;
    }
    if extra_config.submit_answer && problem_data.spj_filename.is_empty() {
        return Err(anyhow!(
            "Special judge must be used when using submit-answer problems!"
        ));
    }
    let comparator: Box<dyn Comparator> = if &problem_data.spj_filename != "" {
        let spj_filename = &problem_data.spj_filename;
        info!("SPJ filename: {}", spj_filename);
        let spj_file = this_problem_path.join(spj_filename);
        lazy_static! {
            static ref SPJ_FILENAME_REGEX: Regex = Regex::new(r#"spj_(.+)\..*"#).unwrap();
        };
        let spj_name_match = SPJ_FILENAME_REGEX
            .captures(spj_filename)
            .ok_or(anyhow!("Invalid spj filename: {}", spj_filename))?;
        let lang = spj_name_match
            .get(1)
            .ok_or(anyhow!("Failed to match spjfilename!"))?
            .as_str();
        info!("SPJ language: {}", lang);
        let lang_config = get_language_config(app, lang, &http_client)
            .await
            .map_err(|e| anyhow!("Failed to get spj language definition: {}", e))?;
        let spj = SpecialJudgeComparator::try_new(
            spj_file.as_path(),
            &lang_config,
            extra_config.spj_execute_time_limit,
            app.config.docker_image.clone(),
        )
        .map_err(|e| anyhow!("Failed to create spj comprator: {}", e))?;
        spj.compile().await.map_err(|e| {
            anyhow!(
                "Error occurred when compiling special judge program:\n{}",
                e
            )
        })?;
        Box::new(spj)
    } else {
        Box::new(SimpleLineComparator {})
    };
    let working_dir =
        tempfile::tempdir().map_err(|e| anyhow!("Failed to create working directory: {}", e))?;
    // let s = PathBuf::from("/test");
    let working_dir_path = working_dir.path();
    info!(
        "Working at: {}",
        working_dir_path.as_os_str().to_str().unwrap_or("")
    );
    update_status(
        app,
        &sub_info.judge_result,
        "Downloading language definition..",
        None,
        sid,
    )
    .await;
    let lang_config = get_language_config(app, &sub_info.language, &http_client)
        .await
        .map_err(|e| anyhow!("Failed to download language definition: {}", e))?;
    info!("Language definition:\n{:#?}", lang_config);
    if !extra_config.submit_answer {
        compile_program(
            app,
            working_dir_path,
            sid,
            &sub_info,
            &lang_config,
            &problem_data,
            this_problem_path.as_path(),
            &extra_config,
        )
        .await?;
    } else {
        todo!();
    }
    let time_scale = extra_config.time_scale.unwrap_or(1.02);
    for subtask in problem_data.subtasks.iter() {
        info!("Judging subtask: {:?}", subtask);
    }
    return Ok(());
}

struct MyUpdater<'a> {
    pub judge_result: &'a SubmissionJudgeResult,
    pub submission_id: i64,
}
#[async_trait::async_trait]
impl<'a> AsyncStatusUpdater for MyUpdater<'a> {
    async fn update(&self, message: &str) {
        let guard = GLOBAL_APP_STATE.read().await;
        let app_state_guard = guard.as_ref().unwrap();
        update_status(
            app_state_guard,
            self.judge_result,
            message,
            None,
            self.submission_id,
        )
        .await;
    }
}
