use crate::core::{misc::ResultType, state::AppState};
use anyhow::anyhow;
use log::error;
use serde::Deserialize;

pub async fn update_ide_status(app: &AppState, run_id: &str, message: &str, status: &str) {
    let handle = async {
        let text_resp = reqwest::Client::new()
            .post(app.config.suburl("/api/ide/update"))
            .form(&[
                ("uuid", app.config.judger_uuid.as_str()),
                ("run_id", run_id),
                ("message", message),
                ("status", status),
            ])
            .send()
            .await
            .map_err(|e| anyhow!("Failed to send request: {}", e))?
            .text()
            .await
            .map_err(|e| anyhow!("Failedto receive response: {}", e))?;
        #[derive(Deserialize)]
        struct Local {
            pub code: i64,
            pub message: Option<String>,
        }
        let parsed = serde_json::from_str::<Local>(&text_resp)
            .map_err(|e| anyhow!("Failed to deserialize: {}", e))?;
        if parsed.code != 0 {
            return Err(anyhow!(
                "Server responded error: {}",
                parsed.message.unwrap_or("".to_string())
            ));
        }
        Ok(())
    };
    let ret: ResultType<()> = handle.await;
    if let Err(e) = ret {
        error!("Failed to report ide run status: {}", e);
    }
}
