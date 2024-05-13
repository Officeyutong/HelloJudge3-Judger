use std::collections::BTreeMap;

use celery::{error::TaskError, task::TaskResult};
use log::{error, info};

use crate::{
    core::state::GLOBAL_APP_STATE,
    task::{local::util::update_status, remote::luogu::handle_luogu_remote_judge},
};

use self::model::RemoteJudgeConfig;
use anyhow::anyhow;
mod luogu;
mod model;

#[celery::task(name = "judgers.remote.run")]
pub async fn remote_judge_task_handler(config: RemoteJudgeConfig) -> TaskResult<()> {
    let guard = GLOBAL_APP_STATE.read().await;
    let app_state_guard = guard.as_ref().unwrap();
    let _semaphore_guard = app_state_guard
        .remote_task_count_semaphore
        .acquire()
        .await
        .unwrap();
    info!("Received remote judge task: {:#?}", config);
    let result = match config.remote_judge_oj.as_str() {
        "luogu" => handle_luogu_remote_judge(&config, app_state_guard).await,
        s => Err(anyhow!("Unsupported remote judge oj: {}", s)),
    };
    if let Err(e) = result {
        error!("Failed to run remote judge: {:?}", e);
        let err_str = format!("{}", e);
        update_status(
            app_state_guard,
            &BTreeMap::new(),
            "Unable to judge, please report this incident to administrator",
            None,
            config.submission_id,
            None,
        )
        .await;
        return Err(TaskError::UnexpectedError(err_str.clone()));
    }
    Ok(())
}
