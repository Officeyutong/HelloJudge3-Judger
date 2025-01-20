use std::{collections::BTreeMap, time::Duration};

use anyhow::{bail, Context};
use http_auth_basic::Credentials;
use log::{debug, error, info};
use reqwest::header;
use serde_json::json;

use crate::{
    core::state::AppState,
    task::{
        local::util::update_status,
        remote::luogu::model::{LuoguJudgeResponse, LuoguTrackData, SimpleResponse},
    },
};

use super::model::RemoteJudgeConfig;
use anyhow::anyhow;
mod model;
static APP_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);
pub async fn handle_luogu_remote_judge(
    config: &RemoteJudgeConfig,
    app: &AppState,
) -> anyhow::Result<()> {
    let enable_o2 = config.extra_arguments.contains("[LUOGU-O2]");
    let client = {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            header::HeaderValue::from_str(
                Credentials::new(&config.luogu_openapp_id, &config.luogu_openapp_secret)
                    .as_http_header()
                    .as_str(),
            )
            .with_context(|| anyhow!("Unable to build auth header"))?,
        );
        reqwest::Client::builder()
            .default_headers(headers)
            .pool_max_idle_per_host(0)
            .user_agent(APP_USER_AGENT)
            .build()
    }
    .with_context(|| anyhow!("Unable to build client"))?;
    let track_data = serde_json::to_string(&LuoguTrackData {
        submission_id: config.submission_id,
    })
    .with_context(|| anyhow!("?"))?;
    let resp = client
        .post("https://open-v1.lgapi.cn/judge/problem")
        .json(&json! ({
            "pid" : config.remote_problem_id,
            "lang":config.language,
            "o2":enable_o2,
            "code":config.code,
            "trackId":track_data
        }))
        .send()
        .await
        .with_context(|| anyhow!("Unable to send request"))?;
    if !resp.status().is_success() {
        let code = resp.status();
        error!(
            "{:#?}",
            resp.text()
                .await
                .with_context(|| anyhow!("Unable to decode text from response"))?
        );
        bail!(
            "Unable to send submission to luogu, bad return code: {}",
            code.as_str()
        );
    }

    let SimpleResponse { request_id } = resp
        .json::<SimpleResponse>()
        .await
        .with_context(|| anyhow!("Unable to code json"))?;
    info!("requestId = {}", request_id);
    update_status(
        app,
        &BTreeMap::new(),
        "Submitted to luogu",
        Some("judging"),
        config.submission_id,
        Some(request_id.clone()),
    )
    .await;
    let mut timed_out: bool = true;
    info!(
        "Started polling, deley sequence: {:?}",
        config.luogu_delay_sequence
    );
    for (itr_idx, delay_time) in config.luogu_delay_sequence.iter().enumerate() {
        let resp = client
            .get("https://open-v1.lgapi.cn/judge/result")
            .query(&[("id", request_id.as_str())])
            .send()
            .await
            .with_context(|| anyhow!("Unable to send query request"))?;
        let resp_status = resp.status();
        if !resp_status.is_success() {
            error!(
                "{:#?}",
                resp.json::<serde_json::Value>()
                    .await
                    .with_context(|| anyhow!("Unable to decode json"))?
            );
            bail!(
                "Unable to fetch result, bad return code = {}",
                resp_status.as_str()
            );
        }
        debug!("response status: {}", resp_status.as_str());
        if resp_status.as_u16() == 200 {
            debug!("Handling..");
            let before_decoded_result = resp
                .text()
                .await
                .with_context(|| anyhow!("Unable to decode fetch result as text"))?;

            let decoded_result = serde_json::from_str::<LuoguJudgeResponse>(&before_decoded_result)
                .with_context(|| {
                    error!("Response: {}", before_decoded_result);
                    anyhow!("Unable to decode response as json")
                })?;
            info!("Track data: {:?}", decoded_result);
            if !decoded_result
                .update_hj2_judge_status(app, config.submission_id, Some(request_id.clone()))
                .await
            {
                timed_out = false;
                debug!("Early breaked");
                break;
            }
        }
        info!(
            "Round {}/{}, delay {}ms",
            itr_idx + 1,
            config.luogu_delay_sequence.len(),
            delay_time
        );
        tokio::time::sleep(Duration::from_millis(*delay_time as u64)).await;
    }
    if timed_out {
        debug!("Timed out");
        update_status(
            app,
            &BTreeMap::default(),
            "跟踪超时",
            Some("unaccepted"),
            config.submission_id,
            Some(request_id.clone()),
        )
        .await;
    }
    info!("Remote submission done: {}", config.submission_id);
    Ok(())
}
