use std::{collections::HashMap, path::PathBuf, sync::Arc};

use tokio::sync::{Mutex, RwLock};

use super::config::JudgerConfig;

pub struct AppState {
    pub config: JudgerConfig,
    pub file_dir_locks: tokio::sync::Mutex<HashMap<i64, Arc<Mutex<()>>>>,
    pub testdata_dir: PathBuf,
}
use lazy_static::lazy_static;
lazy_static! {
    pub static ref GLOBAL_APP_STATE: RwLock<Option<AppState>> = RwLock::new(None);
}
