use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct JudgerConfig {
    pub broker_url: String,
    pub data_dir: String,
    pub web_api_url: String,
    pub judger_uuid: String,
    pub docker_image: String,
    pub logging_level: String,
    pub prefetch_count: u16,
    pub max_tasks_sametime: usize,
}

impl Default for JudgerConfig {
    fn default() -> Self {
        Self {
            broker_url: "redis://127.0.0.1".to_string(),
            data_dir: "testdata".to_string(),
            web_api_url: "http://127.0.0.1:8080/".to_string(),
            judger_uuid: "7222dcd8-96fb-11ec-864e-9cda3efd56be".to_string(),
            docker_image: "python".to_string(),
            logging_level: "info".to_string(),
            prefetch_count: 2,
            max_tasks_sametime: 1,
        }
    }
}

impl JudgerConfig {
    pub fn suburl(&self, sub: &str) -> String {
        let t = if sub.starts_with('/') {
            sub.trim_start_matches('/').to_string()
        } else {
            sub.to_string()
        };
        let suburl = url::Url::parse(&self.web_api_url)
            .unwrap()
            .join(&t)
            .unwrap();
        suburl.to_string()
    }
}
