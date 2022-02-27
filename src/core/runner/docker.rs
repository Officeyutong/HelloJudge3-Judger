use crate::core::{
    misc::ResultType,
    runner::docker_watch::{watch_container, WatchResult},
};
use anyhow::anyhow;
use bollard::{
    container::{Config, LogOutput, LogsOptions},
    models::{
        ContainerStateStatusEnum, HostConfig, HostConfigCgroupnsModeEnum, Mount, MountTypeEnum,
        ResourcesUlimits,
    },
};
use log::{error, info};
#[derive(Debug)]
pub struct ExecuteResult {
    pub exit_code: i32,
    // in microsecond
    pub time_cost: i64,
    // in bytes
    pub memory_cost: i64,
    pub output: String,
    pub output_truncated: bool,
}

pub async fn execute_in_docker(
    image_name: &str,
    mount_dir: &str,
    command: &Vec<String>,
    // in bytes
    memory_limit: i64,
    // in microsecond
    time_limit: i64,
    // task_name: &str,
    max_output_length: usize,
) -> ResultType<ExecuteResult> {
    let docker_client = bollard::Docker::connect_with_socket_defaults()
        .map_err(|e| anyhow!("Failed to initialize docker: {}", e))?;
    let container = docker_client
        .create_container::<String, String>(
            None,
            Config {
                image: Some(image_name.to_string()),
                cmd: Some(command.clone()),
                tty: Some(true),
                open_stdin: Some(true),
                network_disabled: Some(true),
                working_dir: Some("/temp".to_string()),
                host_config: Some(HostConfig {
                    cgroupns_mode: Some(HostConfigCgroupnsModeEnum::PRIVATE),
                    privileged: Some(false),
                    readonly_rootfs: Some(true),
                    mounts: Some(vec![Mount {
                        target: Some("/temp".to_string()),
                        source: Some(mount_dir.to_string()),
                        read_only: Some(false),
                        typ: Some(MountTypeEnum::BIND),
                        ..Default::default()
                    }]),
                    memory: Some(memory_limit),
                    memory_swap: Some(memory_limit),
                    oom_kill_disable: Some(false),
                    // nano_cpus: Some((0.4 / 1e-9) as i64),
                    network_mode: Some("none".to_string()),
                    ulimits: Some(vec![ResourcesUlimits {
                        name: Some("stack".to_string()),
                        soft: Some(8277716992_i64),
                        hard: Some(8277716992_i64),
                    }]),
                    cpu_period: Some(1000000),
                    cpu_quota: Some(1000000),
                    auto_remove: Some(false),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await
        .map_err(|e| anyhow!("Failed to create docker container: {}", e))?;
    info!("Running container with command: {:?}", command);
    docker_client
        .start_container::<&str>(&container.id, None)
        .await
        .map_err(|e| anyhow!("Failed to start container: {}", e))?;
    // docker_client
    //     .stats(container_name, options)
    let attrs = docker_client
        .inspect_container(container.id.as_str(), None)
        .await
        .map_err(|e| anyhow!("Failed to get contaier details: {}", e))?;
    let pid = attrs
        .state
        .ok_or(anyhow!("Missing field: 'state'"))?
        .pid
        .ok_or(anyhow!("Missing field: pid"))?;
    let watch_result =
        tokio::task::spawn_blocking(move || unsafe { watch_container(pid as i32, time_limit) })
            .await
            .map_err(|e| anyhow!("Failed to run blocking task: {}", e))?
            .map_err(|e| anyhow!("Failed to watch the status: {}", e))?;
    info!("Watch result: {:#?}", watch_result);
    {
        let details = docker_client
            .inspect_container(container.id.as_str(), None)
            .await
            .map_err(|e| anyhow!("Failed to get contaier details: {}", e))?;
        if let ContainerStateStatusEnum::EXITED = details
            .state
            .ok_or(anyhow!("Missing field: stats"))?
            .status
            .unwrap_or(bollard::models::ContainerStateStatusEnum::EMPTY)
        {
        } else {
            if let Err(e) = docker_client
                .kill_container::<&str>(container.id.as_str(), None)
                .await
            {
                error!("Failed to kill container: {}", e);
            }
        }
    }
    use futures_util::stream::StreamExt;
    let mut truncated = false;
    let output = {
        let mut out = String::new();
        for line in docker_client
            .logs::<&str>(
                container.id.as_str(),
                Some(LogsOptions {
                    stderr: true,
                    stdout: true,
                    timestamps: false,
                    follow: true,
                    ..Default::default()
                }),
            )
            .collect::<Vec<Result<LogOutput, bollard::errors::Error>>>()
            .await
            .into_iter()
        {
            out.push_str(line?.to_string().as_str());
            if out.len() > max_output_length as usize {
                out = String::from(&out[..max_output_length]);
                truncated = true;
                break;
            }
        }
        out
    };

    let attr = docker_client
        .inspect_container(container.id.as_str(), None)
        .await
        .map_err(|e| anyhow!("Failed to get contaier details: {}", e))?;
    if let Err(e) = docker_client
        .remove_container(container.id.as_str(), None)
        .await
    {
        error!("Failed to remove container: {}", e);
    }
    let WatchResult {
        time_result,
        mut memory_result,
    } = watch_result;
    let is_oom_killed = attr
        .state
        .as_ref()
        .ok_or(anyhow!("?"))?
        .oom_killed
        .ok_or(anyhow!("??"))?;
    if is_oom_killed {
        memory_result = attr
            .host_config
            .ok_or(anyhow!("???"))?
            .memory
            .ok_or(anyhow!("????"))?;
    } else if memory_result > memory_limit && !is_oom_killed {
        memory_result = 0;
    }
    let exit_code = attr.state.ok_or(anyhow!("?????"))?.exit_code.unwrap_or(0);
    return Ok(ExecuteResult {
        exit_code: exit_code as i32,
        memory_cost: memory_result,
        time_cost: time_result,
        output,
        output_truncated: truncated,
    });
}
