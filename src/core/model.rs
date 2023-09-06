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

impl LanguageConfig {
    pub fn source(&self, n: &str) -> String {
        self.source_file.replace("{filename}", n)
    }
    pub fn output(&self, n: &str) -> String {
        self.output_file.replace("{filename}", n)
    }
    pub fn compile_s(&self, source: &str, output: &str, extra: &str) -> String {
        self.compile
            .replace("{source}", source)
            .replace("{output}", output)
            .replace("{extra}", extra)
    }
    pub fn run_s(&self, program: &str, redirect: &str) -> String {
        self.run
            .replace("{program}", program)
            .replace("{redirect}", redirect)
    }
}
