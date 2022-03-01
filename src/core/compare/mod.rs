use async_trait::async_trait;

use super::misc::ResultType;
use std::sync::Arc;
#[derive(Debug)]
pub struct CompareResult {
    pub score: i64,
    pub message: String,
}
#[async_trait]
pub trait Comparator: Sync + Send {
    async fn compare(
        &self,
        user_out: Arc<Vec<u8>>,
        answer: Arc<Vec<u8>>,
        input_data: Arc<Vec<u8>>,
        full_score: i64,
    ) -> ResultType<CompareResult>;
}

pub mod simple;
pub mod special;
