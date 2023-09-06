use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::core::{misc::ResultType, model::LanguageConfig, runner::docker::execute_in_docker};
use anyhow::anyhow;
use async_trait::async_trait;
use log::info;
use tempfile::TempDir;
const SPJ_FILENAME: &str = "specialjudge";
use super::{Comparator, CompareResult};

/*
    SPJ可以为任何所支持的语言编写的程序，但是文件名格式应该为 spj_语言ID.xxx,扩展名不限
    例如spj_cpp11.cpp ,spj_java8.java
    SPJ运行时间不会被计算在这个测试点的时间开销中
    评测时spj所在目录下将会有以下文件:
    user_out: 用户程序输出
    answer: 测试点标准答案
    SPJ应该在限制的时间内将结果输出到以下文件
    score: 该测试点得分(0~100,自动折合)
    message: 发送给用户的信息
*/
pub struct SpecialJudgeComparator {
    spj_file: PathBuf,
    // status_updater: T,
    language_config: LanguageConfig,
    run_time_limit: i64,
    docker_image: String,
    working_dir: TempDir,
}
#[async_trait]
impl Comparator for SpecialJudgeComparator {
    async fn compare(
        &self,
        user_out: Arc<Vec<u8>>,
        answer: Arc<Vec<u8>>,
        input_data: Arc<Vec<u8>>,
        full_score: i64,
    ) -> ResultType<CompareResult> {
        return self
            .my_compare(user_out, answer, input_data, full_score)
            .await;
    }
}
impl SpecialJudgeComparator {
    pub async fn compile(&self) -> ResultType<()> {
        // let working_path = PathBuf::from("/spj");
        let working_path = self.working_dir.path();
        let source_filename = self.language_config.source(SPJ_FILENAME);
        let output_filename = self.language_config.output(SPJ_FILENAME);
        tokio::fs::copy(
            self.spj_file.as_path(),
            &working_path.join(&source_filename),
        )
        .await
        .map_err(|e| anyhow!("Failed to create special judge program: {}", e))?;
        info!("SPJ working dir: {}", working_path.to_str().unwrap_or(""));
        let compile_cmdline = self
            .language_config
            .compile_s(&source_filename, &output_filename, "")
            .split_ascii_whitespace()
            .map(|v| v.to_string())
            .collect::<Vec<String>>();
        let run_result = execute_in_docker(
            &self.docker_image,
            working_path.to_str().unwrap_or(""),
            &compile_cmdline,
            1024 * 1024 * 1024,
            10 * 1000 * 1000,
            1024 * 1024,
        )
        .await
        .map_err(|e| anyhow!("Failed to compile special judge program: {}", e))?;
        info!("SPJ compile result:\n{:#?}", run_result);
        if !working_path.join(output_filename).exists() || run_result.exit_code != 0 {
            return Err(anyhow!(
                "Failed to compile special judge program (exit code = {}):\n{}",
                run_result.exit_code,
                run_result.output
            ));
        }
        Ok(())
    }
    async fn my_compare(
        &self,
        user_out: Arc<Vec<u8>>,
        answer: Arc<Vec<u8>>,
        input_data: Arc<Vec<u8>>,
        full_score: i64,
    ) -> ResultType<CompareResult> {
        // let working_path = PathBuf::from("/spj");
        let working_path = self.working_dir.path();
        tokio::fs::write(working_path.join("user_out"), &*user_out)
            .await
            .map_err(|e| anyhow!("Failed to write user_out: {}", e))?;
        tokio::fs::write(working_path.join("answer"), &*answer)
            .await
            .map_err(|e| anyhow!("Failed to write answer: {}", e))?;
        tokio::fs::write(working_path.join("input"), &*input_data)
            .await
            .map_err(|e| anyhow!("Failed to write input: {}", e))?;
        // let run_cmdline =
        //     .map(|v| v.to_string())
        //     .collect::<Vec<String>>();
        let run_cmdline = vec![
            "sh".to_string(),
            "-c".to_string(),
            self.language_config
                .run_s(&self.language_config.output(SPJ_FILENAME), ""),
        ];
        info!("Run special judge program: {:?}", run_cmdline);
        let run_result = execute_in_docker(
            &self.docker_image,
            working_path.to_str().unwrap_or(""),
            &run_cmdline,
            2048 * 2048 * 2048,
            self.run_time_limit,
            1024 * 1024,
        )
        .await
        .map_err(|e| anyhow!("Failed to run special judge program: {}", e))?;
        info!("SPJ run result: {:#?}", run_result);
        let usage_message = format!(
            "{} MB, {} ms",
            run_result.memory_cost / 1024 / 1024,
            run_result.time_cost / 1000
        );
        let message_file = working_path.join("message");
        let message = if message_file.exists() {
            tokio::fs::read_to_string(message_file)
                .await
                .map_err(|e| anyhow!("Failed to read message file: {}", e))?
        } else {
            "".to_string()
        };
        if run_result.exit_code != 0 {
            return Ok(CompareResult {
                message: format!(
                    "SPJ exited: {}({})|{}",
                    run_result.exit_code, usage_message, message
                ),
                score: 0,
            });
        }
        let score_file = working_path.join("score");
        let score_str = if !score_file.exists() {
            return Ok(CompareResult {
                message: "SPJ exited with no score file".to_string(),
                score: 0,
            });
        } else {
            tokio::fs::read_to_string(score_file)
                .await
                .map_err(|e| anyhow!("Failed to read score: {}", e))?
        };
        let score = score_str
            .trim()
            .parse::<i64>()
            .map_err(|e| anyhow!("Failed to parse score: {}", e))?;

        if !(0..=100).contains(&score) {
            return Err(anyhow!("Invalid score: {}", score));
        }
        Ok(CompareResult {
            message,
            score: (score as f64 / 100.0 * (full_score as f64)).floor() as i64,
        })
    }
    pub fn try_new(
        spj_file: &Path,
        // status_updater: T,
        language_config: &LanguageConfig,
        run_time_limit: i64,
        docker_image: String,
    ) -> ResultType<Self> {
        Ok(Self {
            docker_image,
            // status_updater,
            language_config: language_config.clone(),
            run_time_limit,
            spj_file: spj_file.to_path_buf(),
            working_dir: tempfile::tempdir()
                .map_err(|e| anyhow!("Failed to create spj working directory: {}", e))?,
        })
    }
}
