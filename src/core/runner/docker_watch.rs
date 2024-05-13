use std::{io::Write, ptr::null_mut};

use libc::{gettid, usleep};
use log::{error, info, warn};

use crate::core::misc::ResultType;
use anyhow::anyhow;
#[derive(Debug)]
pub struct WatchResult {
    // time, microsecond
    pub time_result: i64,
    // memory, bytes
    pub memory_result: i64,
}
#[inline]
unsafe fn get_current_usec() -> i64 {
    use libc::{gettimeofday, timeval};
    let mut curr = timeval {
        tv_sec: 0,
        tv_usec: 0,
    };
    gettimeofday(&mut curr as *mut timeval, null_mut());
    curr.tv_sec * 1_000_000 + curr.tv_usec
}

// const FILE_FLAG: *const i8 = "r".as_ptr() as *const i8;
// const FORMAT_STR: *const i8 = "%lld".as_ptr() as *const i8;
// # Safety
// It's very safe!
pub unsafe fn watch_container(
    _pid: i32,
    time_limit: i64,
    container_long_id: String,
) -> ResultType<WatchResult> {
    let tid = gettid();
    info!("Watcher tid: {}", tid);
    let main_group_file = "/sys/fs/cgroup/memory/tasks";
    let main_dir = format!("/sys/fs/cgroup/memory/docker/{}", container_long_id);
    let tasks_file = format!("/sys/fs/cgroup/memory/docker/{}/tasks", container_long_id);
    let max_mem_usage_file = format!(
        "/sys/fs/cgroup/memory/docker/{}/memory.max_usage_in_bytes",
        container_long_id
    );
    // if let Err(e) =.
    match std::fs::File::options().append(true).open(&tasks_file) {
        Ok(mut f) => {
            if let Err(e) = f.write(tid.to_string().as_bytes()) {
                error!("Failed to write my tid: {}", e);
                return Ok(WatchResult {
                    memory_result: 0,
                    time_result: 0,
                });
            }
        }
        Err(e) => {
            error!("Failed to open tasks file: {}", e);
            return Ok(WatchResult {
                memory_result: 0,
                time_result: 0,
            });
        }
    };
    let begin = get_current_usec();
    let mut time_result: i64;
    let should_cleanup = loop {
        time_result = get_current_usec() - begin;
        if time_result >= time_limit {
            break false;
        }
        let s = std::fs::read_to_string(&tasks_file).unwrap();
        if s.as_bytes().iter().filter(|v| **v == b'\n').count() == 1 {
            break true;
        }
        // let mut fp = std::fs::File::open(&tasks_file)
        //     .map_err(|e| anyhow!("Fatal error: Can not open tasks file: {}", e))?;
        // fp.read_to_end(&mut read_buf)
        //     .map_err(|e| anyhow!("Fatal error: failed to read tasks file: {}", e))?;
        // let mut cnt = 0;
        // for c in read_buf.iter() {
        //     if *c == '\n' as u8 {
        //         cnt += 1;
        //     }
        //     if cnt >= 2 {
        //         break;
        //     }
        // }
        // if cnt == 1 {
        //     break true;
        // }
        usleep(150);
    };
    info!("Break: should_cleanup={}", should_cleanup);
    let usage_str = std::fs::read_to_string(max_mem_usage_file)?
        .trim()
        .to_string();
    let memory_usage = usage_str
        .parse::<i64>()
        .map_err(|_| anyhow!("Failed to parse: {}", usage_str))?;
    std::fs::File::options()
        .append(true)
        .open(main_group_file)?
        .write_all(tid.to_string().as_bytes())?;
    if should_cleanup {
        if let Err(e) = std::fs::remove_dir(&main_dir) {
            warn!("Failed to cleanup cgroup dir {}: {}", main_dir, e);
        }
    }
    Ok(WatchResult {
        time_result,
        memory_result: memory_usage,
    })
}
