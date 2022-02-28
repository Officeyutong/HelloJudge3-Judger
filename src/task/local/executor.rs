use std::{collections::BTreeMap, sync::Arc};

use celery::{prelude::TaskError, task::TaskResult};
use lazy_static::lazy_static;
use log::{debug, error, info};
use regex::Regex;
use serde_json::Value;
use tokio::io::AsyncReadExt;

use crate::{
    core::{
        compare::{
            simple::SimpleLineComparator, special::SpecialJudgeComparator, Comparator,
            CompareResult,
        },
        misc::ResultType,
        runner::docker::execute_in_docker,
        state::{AppState, GLOBAL_APP_STATE},
        util::get_language_config,
    },
    task::local::{
        compile::compile_program,
        model::{SubmissionInfo, SubmissionSubtaskResult, SubmissionTestcaseResult},
        util::{get_problem_data, sync_problem_files},
        DEFAULT_PROGRAM_FILENAME,
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
    let sid = sub_info.id.clone();
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
    let comparator: Box<dyn Comparator> = if &problem_data.spj_filename != "" {
        let spj_filename = &problem_data.spj_filename;
        info!("SPJ filename: {}", spj_filename);
        let spj_file = this_problem_path.join(spj_filename);
        lazy_static! {
            static ref SPJ_FILENAME_REGEX: Regex = Regex::new(r#"spj_(.+)\..*"#).unwrap();
        };
        let spj_name_match = SPJ_FILENAME_REGEX
            .captures(spj_filename)
            .ok_or(anyhow!("Invalid spj filename: {}", spj_filename))?;
        let lang = spj_name_match
            .get(1)
            .ok_or(anyhow!("Failed to match spjfilename!"))?
            .as_str();
        info!("SPJ language: {}", lang);
        let lang_config = get_language_config(app, lang, &http_client)
            .await
            .map_err(|e| anyhow!("Failed to get spj language definition: {}", e))?;
        let spj = SpecialJudgeComparator::try_new(
            spj_file.as_path(),
            &lang_config,
            extra_config.spj_execute_time_limit * 1000,
            app.config.docker_image.clone(),
        )
        .map_err(|e| anyhow!("Failed to create spj comprator: {}", e))?;
        spj.compile().await.map_err(|e| {
            anyhow!(
                "Error occurred when compiling special judge program:\n{}",
                e
            )
        })?;
        Box::new(spj)
    } else {
        Box::new(SimpleLineComparator {})
    };
    let working_dir =
        tempfile::tempdir().map_err(|e| anyhow!("Failed to create working directory: {}", e))?;
    // let s = PathBuf::from("/test");
    let working_dir_path = working_dir.path();
    info!(
        "Working at: {}",
        working_dir_path.as_os_str().to_str().unwrap_or("")
    );
    update_status(
        app,
        &sub_info.judge_result,
        "Downloading language definition..",
        None,
        sid,
    )
    .await;
    let lang_config = get_language_config(app, &sub_info.language, &http_client)
        .await
        .map_err(|e| anyhow!("Failed to download language definition: {}", e))?;
    info!("Language definition:\n{:#?}", lang_config);
    let compile_result = if !extra_config.submit_answer {
        let compile_ret = compile_program(
            app,
            working_dir_path,
            sid,
            &sub_info,
            &lang_config,
            &problem_data,
            this_problem_path.as_path(),
            &extra_config,
            &sub_info.judge_result,
        )
        .await?;
        if compile_ret.compile_error {
            return Ok(());
        }
        Some(compile_ret.execute_result)
    } else {
        todo!();
    };
    let time_scale = extra_config.time_scale.unwrap_or(1.02);
    let mut judge_result = sub_info.judge_result.clone();
    // 先上传一遍全新的测试点
    problem_data.subtasks.iter().for_each(|v| {
        judge_result.insert(
            v.name.clone(),
            SubmissionSubtaskResult {
                score: 0,
                status: "waiting".to_string(),
                testcases: v
                    .testcases
                    .iter()
                    .map(|q| SubmissionTestcaseResult {
                        full_score: q.full_score,
                        input: q.input.clone(),
                        memory_cost: 0,
                        message: "".to_string(),
                        output: q.output.clone(),
                        score: 0,
                        status: "waiting".to_string(),
                        time_cost: 0,
                    })
                    .collect(),
            },
        );
    });
    update_status(app, &judge_result, "", None, sid).await;
    for subtask in problem_data.subtasks.iter() {
        info!("Judging subtask: {:?}", subtask);
        // let mut subtask_result = judge_result.get_mut(&subtask.name).unwrap();

        let mut will_skip = false;
        for (i, testcase) in subtask.testcases.iter().enumerate() {
            judge_result.get_mut(&subtask.name).unwrap().testcases[i].status =
                "judging".to_string();
            update_status(
                app,
                &judge_result.clone(),
                &format!("评测: 子任务 {}, 测试点 {}", subtask.name, i + 1),
                None,
                sid,
            )
            .await;
            if will_skip {
                let mut ret_ref = &mut judge_result.get_mut(&subtask.name).unwrap().testcases[i];
                ret_ref.score = 0;
                ret_ref.status = "skipped".to_string();
                ret_ref.message = "跳过".to_string();
                continue;
            }
            if extra_config.submit_answer {
                todo!();
            } else {
                let input_file = if problem_data.using_file_io == 1 {
                    problem_data.input_file_name.as_str()
                } else {
                    "in"
                };
                let output_file = if problem_data.using_file_io == 1 {
                    problem_data.output_file_name.as_str()
                } else {
                    "out"
                };
                info!("Input file: {}, output file: {}", input_file, output_file);
                tokio::fs::copy(
                    this_problem_path.join(&testcase.input),
                    working_dir_path.join(input_file),
                )
                .await
                .map_err(|e| anyhow!("Failed to copy input file: {}", e))?;
                let scaled_time = (subtask.time_limit as f64 * time_scale) as i64;
                let execute_cmdline = lang_config.run_s(
                    &lang_config.output(DEFAULT_PROGRAM_FILENAME),
                    &(if problem_data.using_file_io == 1 {
                        "".to_string()
                    } else {
                        format!("< {} > {}", input_file, output_file)
                    }),
                );
                info!("Run command line: {}", execute_cmdline);
                let run_result = execute_in_docker(
                    &app.config.docker_image,
                    working_dir_path.to_str().unwrap(),
                    &vec!["sh".to_string(), "-c".to_string(), execute_cmdline],
                    subtask.memory_limit * 1024 * 1024,
                    scaled_time * 1000,
                    1000,
                )
                .await
                .map_err(|e| anyhow!("Fatal error: {}", e))?;
                info!("Run result:\n{:#?}", run_result);
                {
                    let mut testcase_result =
                        &mut judge_result.get_mut(&subtask.name).unwrap().testcases[i];
                    testcase_result.memory_cost = run_result.memory_cost;
                    testcase_result.time_cost =
                        (run_result.time_cost as f64 / 1000.0).ceil() as i64;
                    if run_result.memory_cost / 1024 / 1024 >= subtask.memory_limit {
                        testcase_result.update_status("memory_limit_exceed");
                    } else if run_result.time_cost >= scaled_time * 1000 {
                        testcase_result.update_status("time_limit_exceed");
                    } else if run_result.exit_code != 0 {
                        testcase_result.update(
                            "runtime_error",
                            &format!("退出代码: {}", run_result.exit_code),
                        );
                    } else {
                        let user_out =
                            match tokio::fs::File::open(working_dir_path.join(output_file)).await {
                                Ok(mut f) => match f.metadata().await {
                                    Ok(d) => {
                                        if d.len() > extra_config.output_file_size_limit as u64 {
                                            testcase_result
                                                .update("output_size_limit_exceed", "输出文件过大");
                                            continue;
                                        }
                                        let mut v: Vec<u8> = vec![];
                                        match f.read_to_end(&mut v).await {
                                            Ok(_) => v,
                                            Err(_) => vec![],
                                        }
                                    }
                                    Err(e) => {
                                        error!("Failed to get metadata: {}", e);
                                        vec![]
                                    }
                                },
                                Err(e) => {
                                    error!("Failed to open output file: {}", e);
                                    vec![]
                                }
                            };
                        let full_score = testcase.full_score;
                        let input_data = tokio::fs::read(this_problem_path.join(&testcase.input))
                            .await
                            .map_err(|e| {
                                anyhow!("Failed to read input data: {}, {}", testcase.input, e)
                            })?;
                        let answer_data = tokio::fs::read(this_problem_path.join(&testcase.output))
                            .await
                            .map_err(|e| {
                                anyhow!("Failed to read answer data: {}, {}", testcase.output, e)
                            })?;
                        let CompareResult { score, message } = match comparator
                            .compare(
                                Arc::new(user_out.into()),
                                Arc::new(answer_data.into()),
                                Arc::new(input_data.into()),
                                full_score,
                            )
                            .await
                        {
                            Ok(v) => v,
                            Err(e) => CompareResult {
                                score: 0,
                                message: e.to_string(),
                            },
                        };
                        if score < full_score {
                            testcase_result.update_status("wrong_answer");
                        } else if score == full_score {
                            testcase_result.update_status("accepted");
                        } else {
                            testcase_result
                                .update("unaccepted", &format!("Illegal score: {}", score));
                        }
                        testcase_result.score = score;
                        testcase_result.message = message;
                        if testcase_result.status != "accepted" && subtask.method == "min" {
                            will_skip = true;
                        }
                    }
                }
            }
        } //subtask
        let mut subtask_result = judge_result.get_mut(&subtask.name).unwrap();
        if subtask.method == "min" {
            if subtask_result
                .testcases
                .iter()
                .all(|v| v.status == "accepted")
            {
                subtask_result.score = subtask.score;
            } else {
                subtask_result.score = 0;
            }
        } else if subtask.method == "sum" {
            subtask_result.score = subtask_result.testcases.iter().map(|v| v.score).sum();
        }
        subtask_result.status = (if subtask_result.score == subtask.score {
            "accepted"
        } else {
            "unaccepted"
        })
        .to_string();
    }
    info!("Judge result: {:?}", judge_result);
    if !extra_config.submit_answer {
        let compile_result = compile_result.unwrap();
        update_status(app, &judge_result, &format!("HelloJudge3-Judger, version {}\n{}\n编译时间占用: {} ms\n编译内存占用: {} MB\n退出代码: {}",
        env!("CARGO_PKG_VERSION"),
        compile_result.output,
        compile_result.time_cost/1000,
        compile_result.memory_cost/1024/1024,
        compile_result.exit_code
    ), None, sid).await;
    } else {
        update_status(app, &judge_result, "", None, sid).await;
    }
    info!("Judge task finished");
    return Ok(());
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
