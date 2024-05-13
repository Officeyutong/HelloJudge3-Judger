use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
};

use async_zip::read::mem::ZipFileReader;
use celery::{prelude::TaskError, task::TaskResult};
use lazy_static::lazy_static;
use log::{debug, info};
use regex::Regex;
use serde_json::Value;

use crate::{
    core::{
        compare::{simple::SimpleLineComparator, special::SpecialJudgeComparator, Comparator},
        misc::ResultType,
        state::{AppState, GLOBAL_APP_STATE},
        util::get_language_config,
    },
    task::local::{
        compile::compile_program,
        dependency::{DependencyGraph, SkippedSubtask, DEPENDENCY_DEFINITION_FILENAME},
        model::{
            ProblemSubtask, SubmissionInfo, SubmissionSubtaskResult, SubmissionTestcaseResult,
        },
        submit_answer::handle_submit_answer,
        traditional::handle_traditional,
        util::{get_problem_data, sync_problem_files},
    },
};

use super::{
    compile::CompileResult,
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
    let _semaphore_guard = app_state_guard.task_count_lock.acquire().await.unwrap();
    let sid = submission_data.pointer("/id").unwrap().as_i64().unwrap();
    if let Err(e) = handle(submission_data, extra_config, app_state_guard).await {
        let err_str = format!("{}", e,);
        update_status(app_state_guard, &BTreeMap::new(), &err_str, None, sid, None).await;
        return Err(TaskError::UnexpectedError(err_str.clone()));
    }
    Ok(())
}
pub enum IntermediateValue {
    SubmitAnswer(HashMap<String, Vec<u8>>),
    Traditional(CompileResult),
}
impl IntermediateValue {
    pub fn traditional(self) -> Option<CompileResult> {
        match self {
            IntermediateValue::SubmitAnswer(_) => None,
            IntermediateValue::Traditional(v) => Some(v),
        }
    }
    pub fn submit_answer(&self) -> Option<&HashMap<String, Vec<u8>>> {
        match self {
            IntermediateValue::SubmitAnswer(v) => Some(v),
            IntermediateValue::Traditional(_) => None,
        }
    }
}
async fn handle(
    submission_info: Value,
    extra_config: ExtraJudgeConfig,
    app: &AppState,
) -> ResultType<()> {
    debug!("Raw task:\n{:#?}", submission_info);
    let sub_info = serde_json::from_value::<SubmissionInfo>(submission_info)
        .map_err(|e| anyhow!("Failed to deserialize submission info: {}", e))?;
    info!("Received local judge task:\n{:#?}", sub_info);
    let http_client = reqwest::Client::new();
    let problem_data = get_problem_data(&http_client, app, &sub_info).await?;
    debug!("Problem info:\n{:#?}", problem_data);
    let this_problem_path = app.testdata_dir.join(problem_data.id.to_string());
    let sid = sub_info.id;
    if extra_config.auto_sync_files {
        sync_problem_files(
            problem_data.id,
            &MyUpdater {
                judge_result: &sub_info.judge_result,
                submission_id: sub_info.id,
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
    let comparator: Box<dyn Comparator> = if !problem_data.spj_filename.is_empty() {
        let spj_filename = &problem_data.spj_filename;
        info!("SPJ filename: {}", spj_filename);
        let spj_file = this_problem_path.join(spj_filename);
        lazy_static! {
            static ref SPJ_FILENAME_REGEX: Regex = Regex::new(r#"spj_(.+)\..*"#).unwrap();
        };
        let spj_name_match = SPJ_FILENAME_REGEX.captures(spj_filename).ok_or(anyhow!(
            "Invalid spj filename: {}, expected spj_xxx.yyy",
            spj_filename
        ))?;
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
        None,
    )
    .await;
    let lang_config = get_language_config(app, &sub_info.language, &http_client)
        .await
        .map_err(|e| anyhow!("Failed to download language definition: {}", e))?;
    info!("Language definition:\n{:#?}", lang_config);
    let intermediate_value = if !extra_config.submit_answer {
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
        IntermediateValue::Traditional(compile_ret)
    } else {
        let mut required_files = HashSet::<String>::default();
        for subtask in problem_data.subtasks.iter() {
            for testcase in subtask.testcases.iter() {
                required_files.insert(testcase.output.clone());
            }
        }
        let b64dec = Arc::new(
            base64::decode(
                extra_config
                    .answer_data
                    .as_ref()
                    .ok_or(anyhow!("Missing answer data!"))?,
            )
            .map_err(|e| anyhow!("Failed to decode answer data: {}", e))?,
        );
        let mut zip = ZipFileReader::new(&b64dec)
            .await
            .map_err(|e| anyhow!("Failed to read zip file: {}", e))?;
        let mut answer_files = HashMap::<String, Vec<u8>>::default();
        for t in required_files.iter() {
            let entry = zip.entry(t.as_str()).map(|v| v.0);
            let to_insert = if let Some(v) = entry {
                let things = zip
                    .entry_reader(v)
                    .await
                    .map_err(|e| anyhow!("Failed to read file: {}, {}", t, e))?;
                things
                    .read_to_end_crc()
                    .await
                    .map_err(|e| anyhow!("Failed to decompress file: {}, {}", t, e))?
            } else {
                vec![]
            };
            answer_files.insert(t.clone(), to_insert);
        }
        info!(
            "Files in user zip: {:?}",
            answer_files.keys().collect::<Vec<&String>>()
        );
        IntermediateValue::SubmitAnswer(answer_files)
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
    update_status(app, &judge_result, "", None, sid, None).await;
    let dep_file = this_problem_path.join(DEPENDENCY_DEFINITION_FILENAME);

    let dependency_info = if dep_file.exists() {
        let val = serde_json::from_str::<serde_json::Value>(
            &tokio::fs::read_to_string(dep_file).await.map_err(|e| {
                anyhow!(
                    "{} exists, but failed to read: {}",
                    DEPENDENCY_DEFINITION_FILENAME,
                    e
                )
            })?,
        )
        .map_err(|e| {
            anyhow!(
                "{} exists, but failed to parse: {}",
                DEPENDENCY_DEFINITION_FILENAME,
                e
            )
        })?;
        info!("Loaded {}:\n{:#?}", DEPENDENCY_DEFINITION_FILENAME, val);
        Some(val)
    } else {
        info!("{} not found.", DEPENDENCY_DEFINITION_FILENAME);
        None
    };
    let mut dep_state_machine = DependencyGraph::new(
        &problem_data
            .subtasks
            .iter()
            .map(|v| v.name.clone())
            .collect::<Vec<_>>(),
        dependency_info,
    )
    .map_err(|e| anyhow!("Error when building dependency graph: {}", e))?;
    let subtask_data_by_name = HashMap::<String, &ProblemSubtask>::from_iter(
        problem_data.subtasks.iter().map(|v| (v.name.clone(), v)),
    );
    while let Some(subtask_name) = dep_state_machine.next_subtask_name() {
        let subtask = subtask_data_by_name
            .get(&subtask_name)
            .ok_or_else(|| anyhow!("Failed to get subtask `{}` by name!", subtask_name))?;
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
                None,
            )
            .await;
            if will_skip {
                let ret_ref = &mut judge_result.get_mut(&subtask.name).unwrap().testcases[i];
                ret_ref.score = 0;
                ret_ref.status = "skipped".to_string();
                ret_ref.message = "跳过".to_string();
                continue;
            }
            if extra_config.submit_answer {
                let testcase_result =
                    &mut judge_result.get_mut(&subtask.name).unwrap().testcases[i];
                handle_submit_answer(
                    testcase_result,
                    testcase,
                    this_problem_path.as_path(),
                    &intermediate_value,
                    &*comparator,
                )
                .await?;
            } else {
                handle_traditional(
                    &problem_data,
                    this_problem_path.as_path(),
                    working_dir_path,
                    testcase,
                    subtask,
                    time_scale,
                    &lang_config,
                    app,
                    &*comparator,
                    &extra_config,
                    i,
                    &mut will_skip,
                    &mut judge_result,
                )
                .await?;
            }
        } //subtask
        let subtask_result = judge_result.get_mut(&subtask.name).unwrap();
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
            dep_state_machine.report(true);
            "accepted"
        } else {
            dep_state_machine.report(false);
            "unaccepted"
        })
        .to_string();
    }
    let skipped_subtask = dep_state_machine.get_skipped_subtasks();
    info!(
        "Skipped subtasks:\n{}",
        skipped_subtask
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    );
    let skipped_subtask_message = {
        let mut buf = String::new();
        for item in skipped_subtask.into_iter() {
            let SkippedSubtask {
                ref name,
                ref reason,
            } = item;
            let curr_subtask_result = judge_result
                .get_mut(name)
                .ok_or_else(|| anyhow!("Unexpected missing subtask: {}", name))?;
            curr_subtask_result.status = "skipped".into();
            for testcase in curr_subtask_result.testcases.iter_mut() {
                testcase.status = "skipped".into();
                testcase.message = reason.clone();
            }
            buf.push_str(&item.to_string());
            buf.push('\n');
        }
        buf
    };
    info!("Judge result: {:?}", judge_result);
    if !extra_config.submit_answer {
        let compile_result = intermediate_value.traditional().unwrap().execute_result;
        update_status(
            app,
            &judge_result,
            &format!(
                "{}\n评测结束于: {}\n{}\n编译时间占用: {} ms\n编译内存占用: {} MB\n退出代码: {}\n跳过了以下子任务:\n{}",
                app.version_string,
                chrono::Local::now().format("%F %X"),
                compile_result.output,
                compile_result.time_cost / 1000,
                compile_result.memory_cost / 1024 / 1024,
                compile_result.exit_code,
                skipped_subtask_message
            ),
            None,
            sid,
            None
        )
        .await;
    } else {
        update_status(
            app,
            &judge_result,
            &format!("跳过了以下子任务:\n{}", skipped_subtask_message),
            None,
            sid,
            None,
        )
        .await;
    }
    info!("Judge task finished");
    Ok(())
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
            None,
        )
        .await;
    }
}
