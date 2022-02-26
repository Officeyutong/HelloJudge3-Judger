use serde::{Deserialize, Serialize};

#[derive(Deserialize, Debug, Clone, Serialize)]
pub struct LanguageConfig {
    pub source_file: String,
    pub output_file: String,
    pub compile: String,
    pub run: String,
    pub display: String,
    pub version: String,
    pub ace_mode: String,
    pub hljs_mode: String,
}
