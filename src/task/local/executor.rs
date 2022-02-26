use std::collections::BTreeMap;

use celery::{prelude::TaskError, task::TaskResult};
use log::{debug, info};
use serde_json::Value;

use crate::{
    core::{
        misc::ResultType,
        state::{AppState, GLOBAL_APP_STATE},
    },
    task::local::{
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

    return Err(anyhow!("?????"));
    // return Ok(());
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
