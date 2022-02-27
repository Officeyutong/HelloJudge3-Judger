use std::{io::BufReader, ptr::null_mut};

use libc::{fclose, fopen, fscanf, kill, usleep};
use log::error;

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
    return curr.tv_sec * 1_000_000 + curr.tv_usec;
}
pub unsafe fn watch_container(pid: i32, time_limit: i64) -> ResultType<WatchResult> {
    let cgroup_root = format!("/proc/{}/cgroup", pid);
    // let mut cpu_cgp: Option<String> = None;
    let mut memory_cgp: Option<String> = None;
    {
        use std::io::prelude::*;
        let file = match std::fs::File::open(&cgroup_root) {
            Ok(v) => v,
            Err(e) => {
                error!("Failed to read cgroup control file: {}", e);
                return Ok(WatchResult {
                    memory_result: 0,
                    time_result: 0,
                });
            }
        };
        // .map_err(|e| anyhow!("Failed to read cgroup control file: {}, {}", cgroup_root, e))?;
        let reader = BufReader::new(file);
        for line in reader.split('\n' as u8) {
            let s = String::from_utf8(
                line.map_err(|e| anyhow!("Failed to read cgroup control file: {}", e))?,
            )
            .map_err(|e| anyhow!("Failed to decode cgroup control file: {}", e))?;
            let splitted = s.split(":").collect::<Vec<&str>>();
            if splitted.len() == 3 {
                match splitted[..] {
                    // [_, tp, id] if tp.contains("cpu") => {
                    //     cpu_cgp = Some(format!("/sys/fs/cgroup/cpu{}/cpu.stat", id));
                    // }
                    [_, tp, id] if tp.contains("memory") => {
                        memory_cgp =
                            Some(format!("/sys/fs/cgroup/memory{}/memory.usage_in_bytes", id));
                    }
                    _ => (),
                };
            }
        }
    }
    // let cpu = cpu_cgp
    //     .as_ref()
    //     .ok_or(anyhow!("Failed to find cpu cgroup control file!"))?
    //     .as_bytes();
    let memory = memory_cgp
        .as_ref()
        .ok_or(anyhow!("Failed to find memory cgroup control file!"))?
        .as_bytes();
    let begin = get_current_usec();
    let mut total_memory: i64 = 0;
    let mut memory_cost_count: i64 = 0;
    let mut time_result: i64 = -1;
    let time_mul_1000 = time_limit * 1000;
    const FILE_FLAG: *const i8 = "r".as_ptr() as *const i8;
    while kill(pid, 0) == 0 {
        time_result = get_current_usec() - begin;
        if time_result >= time_mul_1000 {
            break;
        }
        let fp = fopen(memory.as_ptr() as *mut i8, FILE_FLAG);
        if !fp.is_null() {
            let mut curr_usage: i64 = 0;
            if fscanf(
                fp,
                "%lld".as_ptr() as *const i8,
                &mut curr_usage as *mut i64,
            ) > 0
            {
                total_memory += curr_usage;
                memory_cost_count += 1;
            }
            fclose(fp);
        } else {
            break;
        }
        usleep(100);
    }
    let memory_result = if memory_cost_count == 0 {
        0
    } else {
        total_memory / memory_cost_count
    };
    if time_result == -1 {
        time_result = 0;
    }
    return Ok(WatchResult {
        memory_result,
        time_result,
    });
}
