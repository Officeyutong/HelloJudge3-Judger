use super::{misc::ResultType, model::LanguageConfig, state::AppState};
use anyhow::anyhow;
use serde::Deserialize;
pub async fn get_language_config(
    app: &AppState,
    language_id: &str,
    client: &reqwest::Client,
) -> ResultType<LanguageConfig> {
    let text_resp = client
        .post(app.config.suburl("/api/judge/get_lang_config_as_json"))
        .form(&[("lang_id", language_id), ("uuid", &app.config.judger_uuid)])
        .send()
        .await
        .map_err(|e| {
            anyhow!(
                "Failed to send request when getting language setting: {}",
                e
            )
        })?
        .text()
        .await
        .map_err(|e| anyhow!("Failed to receive response: {}", e))?;
    #[derive(Deserialize)]
    struct Local {
        pub code: i64,
        pub message: Option<String>,
        pub data: Option<LanguageConfig>,
    }
    let parsed = serde_json::from_str::<Local>(&text_resp)
        .map_err(|e| anyhow!("Failed to deserialize language config: {}", e))?;
    if parsed.code != 0 {
        return Err(anyhow!(
            "Invalid code {} when getting language config: {}",
            parsed.code,
            parsed.message.unwrap_or(String::from("<>"))
        ));
    }
    return Ok(parsed.data.ok_or(anyhow!("Missing field!"))?);
}
