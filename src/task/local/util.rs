use std::{collections::HashSet, future::Future, sync::Arc, time::UNIX_EPOCH};

use anyhow::{anyhow, bail};
use log::{debug, error, info};
use serde::Deserialize;
use tokio::sync::Mutex;

use crate::core::{misc::ResultType, state::AppState};

use super::model::{ProblemInfo, SubmissionInfo, SubmissionJudgeResult};
pub async fn update_status(
    app: &AppState,
    judge_result: &SubmissionJudgeResult,
    message: &str,
    extra_status: Option<&str>,
    submission_id: i64,
    extra_remote_data: Option<String>,
) {
    let handle = async {
        let url = app.config.suburl("/api/judge/update");
        let mut form_data = vec![
            ("uuid", app.config.judger_uuid.clone()),
            ("judge_result", serde_json::to_string(judge_result).unwrap()),
            ("submission_id", submission_id.to_string()),
            ("message", message.to_string()),
            (
                "extra_status",
                extra_status
                    .map(|v| v.to_string())
                    .unwrap_or("".to_string()),
            ),
        ];
        if let Some(v) = extra_remote_data {
            form_data.push(("extra_information_by_remote_judge", v));
        }
        let text_resp = reqwest::Client::new()
            .post(url)
            .form(&form_data)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to send request: {}", e))?
            .text()
            .await
            .map_err(|e| anyhow!("Failed to read response: {}", e))?;
        #[derive(Deserialize)]
        struct Local {
            pub code: i64,
            pub message: Option<String>,
        }
        match serde_json::from_str::<Local>(&text_resp) {
            Ok(des) => {
                if des.code != 0 {
                    return Err(anyhow!(
                        "Received failing message: {}",
                        des.message.unwrap_or("<Not available>".to_string())
                    ));
                }
            }
            Err(e) => {
                bail!("Invalid response from hj2 server: {}, {}", text_resp, e);
            }
        }

        Ok(())
    };
    let ret: ResultType<()> = handle.await;
    if let Err(e) = ret {
        error!("Failed to report status:\n{}", e);
    }
}

pub async fn get_problem_data(
    http_client: &reqwest::Client,
    app: &AppState,
    sub_info: &SubmissionInfo,
) -> ResultType<ProblemInfo> {
    #[derive(Deserialize)]
    struct ProblemInfoResp {
        pub code: i64,
        pub message: Option<String>,
        pub data: Option<ProblemInfo>,
    }
    let problem_data_pack = serde_json::from_str::<ProblemInfoResp>(
        &http_client
            .post(app.config.suburl("/api/judge/get_problem_info"))
            .form(&[
                ("uuid", &app.config.judger_uuid),
                ("problem_id", &sub_info.problem_id.to_string()),
            ])
            .send()
            .await
            .map_err(|e| anyhow!("Failed to send http request: {}", e))?
            .text()
            .await
            .map_err(|e| anyhow!("Failed to receive http response: {}", e))?,
    )
    .map_err(|e| anyhow!("Failed to deserialize problem data: {}", e))?;
    if problem_data_pack.code != 0 {
        return Err(anyhow!(
            "Failed to get problem info: {}",
            problem_data_pack.message.unwrap_or(String::from("<>"))
        ));
    }
    let problem_data = problem_data_pack
        .data
        .ok_or(anyhow!("Missing data field!"))?;
    Ok(problem_data)
}
#[derive(Deserialize)]
pub struct ProblemFile {
    pub name: String,
    pub size: i64,
    pub last_modified_time: f64,
}
#[derive(Deserialize)]
pub struct Resp {
    pub code: i64,
    pub message: Option<String>,
    pub data: Option<Vec<ProblemFile>>,
}
#[async_trait::async_trait]
pub trait AsyncStatusUpdater: Sync + Send {
    async fn update(&self, message: &str);
}
pub fn sync_problem_files<'a>(
    problem_id: i64,
    updater: &'a dyn AsyncStatusUpdater,
    http_client: &'a reqwest::Client,
    app: &'a AppState,
) -> impl Future<Output = ResultType<()>> + 'a {
    async move {
        let text = http_client
            .post(app.config.suburl("/api/judge/get_file_list"))
            .form(&[
                ("uuid", app.config.judger_uuid.as_str()),
                ("problem_id", &problem_id.to_string()),
            ])
            .send()
            .await
            .map_err(|e| anyhow!("Failed to send http request when getting file list: {}", e))?
            .text()
            .await
            .map_err(|e| anyhow!("Failed to read response: {}", e))?;
        let parsed = serde_json::from_str::<Resp>(&text)
            .map_err(|e| anyhow!("Failed to deserialize problem file list: {}", e))?;
        if parsed.code != 0 {
            return Err(anyhow!(
                "Failed to get problem file list: {}",
                parsed.message.unwrap_or(String::from("<>"))
            ));
        }
        let files = parsed.data.ok_or(anyhow!("Missing files!"))?;
        let problem_lock = {
            let mut lock = app.file_dir_locks.lock().await;
            if let std::collections::hash_map::Entry::Vacant(e) = lock.entry(problem_id) {
                let v = Arc::new(Mutex::new(()));
                e.insert(v.clone());
                v
            } else {
                lock.get(&problem_id).unwrap().clone()
            }
        };
        let _guard = problem_lock.lock().await;
        info!("Syncing problem files for problem {}", problem_id);
        updater.update("Syncing files..").await;
        let data_path = app.testdata_dir.join(problem_id.to_string());
        if !data_path.exists() {
            std::fs::create_dir(&data_path)
                .map_err(|e| anyhow!("Failed to create problem data dir: {}", e))?;
        }
        let file_list_set = HashSet::<String>::from_iter(files.iter().map(|v| v.name.clone()));
        // 删除掉服务端已经不用了的文件
        for entry in std::fs::read_dir(data_path.as_path())
            .map_err(|e| anyhow!("Failed to open data dir: {}", e))?
        {
            let entry = entry?;
            let file_name = entry.file_name().to_string_lossy().to_string();
            if !file_name.ends_with(".lock") {
                if entry
                    .file_type()
                    .map_err(|e| {
                        anyhow!("Failed to get filetype for {:?}: {}", entry.file_name(), e)
                    })?
                    .is_file()
                {
                    if !file_list_set.contains(&file_name) {
                        info!(
                            "Removing {} since the server has already removed this file..",
                            file_name
                        );
                        if let Err(e) = std::fs::remove_file(data_path.join(&file_name)) {
                            error!("Failed to remove {}: {}", file_name, e);
                        }
                        let lock_file = format!("{}.lock", file_name);
                        if let Err(e) = std::fs::remove_file(data_path.join(&lock_file)) {
                            error!("Failed to remove {}: {}", lock_file, e);
                        }
                    }
                } else {
                    debug!(
                        "Ignore check for {:?} since it's a directory",
                        entry.file_name()
                    );
                }
            } else {
                debug!(
                    "Ignore check for {:?} since it's a lock file",
                    entry.file_name()
                );
            }
        }
        for file in files.into_iter() {
            let lock_file = data_path.join(format!("{}.lock", file.name));
            let data_file = data_path.join(&file.name);
            let should_download = if lock_file.exists() {
                let lock_file_content =
                    tokio::fs::read_to_string(&lock_file).await.map_err(|e| {
                        anyhow!(
                            "Failed to read lock file: {}\n{}",
                            lock_file.to_str().unwrap_or(""),
                            e
                        )
                    })?;
                if let Ok(v) = lock_file_content.parse::<f64>() {
                    // 硬盘上的文件太旧了
                    v < file.last_modified_time
                } else {
                    true
                }
            } else {
                true
            };
            if should_download {
                info!("Downloading {}", file.name);
                updater
                    .update(&format!("Syncing file: {}", file.name))
                    .await;
                let data = http_client
                    .post(app.config.suburl("/api/judge/download_file"))
                    .form(&[
                        ("problem_id", problem_id.to_string().as_str()),
                        ("filename", file.name.as_str()),
                        ("uuid", &app.config.judger_uuid),
                    ])
                    .send()
                    .await
                    .map_err(|e| {
                        anyhow!("Failed to send http request when downloading data: {}", e)
                    })?
                    .bytes()
                    .await
                    .map_err(|e| anyhow!("Failed to read response: {}", e))?;
                info!("Downloaded: {}, saving..", file.name);
                tokio::fs::write(&data_file, data.to_vec())
                    .await
                    .map_err(|e| anyhow!("Failed to save `{}`: {}", file.name, e))?;
                let current_timestamp = std::time::SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|e| anyhow!("Failed to get timestamp: {}", e))?
                    .as_secs();
                tokio::fs::write(&lock_file, format!("{}", current_timestamp))
                    .await
                    .map_err(|_| {
                        anyhow!(
                            "Failed to write lock file: {}",
                            lock_file.as_os_str().to_str().unwrap_or("")
                        )
                    })?;
                info!("Success: {}", file.name);
            }
        }
        Ok(())
    }
}
