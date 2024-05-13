use std::{collections::HashMap, path::PathBuf, sync::Arc};

use tokio::sync::{Mutex, RwLock, Semaphore};

use super::config::JudgerConfig;

pub struct AppState {
    pub config: JudgerConfig,
    pub file_dir_locks: tokio::sync::Mutex<HashMap<i64, Arc<Mutex<()>>>>,
    pub testdata_dir: PathBuf,
    pub version_string: String,
    pub task_count_lock: Arc<Semaphore>,
    pub remote_task_count_semaphore: Arc<Semaphore>,
}
use lazy_static::lazy_static;
lazy_static! {
    pub static ref GLOBAL_APP_STATE: RwLock<Option<AppState>> = RwLock::new(None);
}
