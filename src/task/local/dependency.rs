use anyhow::anyhow;
use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap},
    fmt::Display,
};

pub const DEPENDENCY_DEFINITION_FILENAME: &str = "subtask_dependency.json";
#[derive(Debug)]
pub struct SkippedSubtask {
    pub name: String,
    pub reason: String,
}

impl Display for SkippedSubtask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.name, self.reason)
    }
}

/*
A->B表示A依赖B，即B完成后才可以做A
*/
pub struct DependencyGraph {
    name_to_index: HashMap<String, usize>,
    index_to_name: Vec<String>,
    graph: Vec<Vec<usize>>,
    rev_graph: Vec<Vec<usize>>,
    outdeg: Vec<i32>,
    // 存储点编号，让编号小的靠前
    heap: BinaryHeap<Reverse<usize>>,
    dropped: Vec<bool>,
}

impl DependencyGraph {
    pub fn new<T: AsRef<str>>(
        names: &[T],
        graph_json: Option<serde_json::Value>,
    ) -> anyhow::Result<Self> {
        let mut name_to_index = HashMap::default();
        let mut index_to_name = Vec::new();
        for (idx, name) in names.iter().enumerate() {
            let s = name.as_ref().to_string();
            name_to_index.insert(s.clone(), idx);
            index_to_name.push(s);
        }
        let mut graph = vec![Vec::default(); names.len()];
        let mut rev_graph = vec![Vec::default(); names.len()];
        let mut outdeg = vec![0; names.len()];
        if let Some(v) = graph_json {
            let parsed_graph = serde_json::from_value::<HashMap<String, Vec<String>>>(v)
                .map_err(|e| anyhow!("Failed to parse dependency json: {}", e))?;
            for (u, edges) in parsed_graph.into_iter() {
                let u_idx = name_to_index
                    .get(&u)
                    .ok_or_else(|| anyhow!("Invalid subtask name `{}` in dependency json!", u))?;
                for v in edges.into_iter() {
                    let v_idx = name_to_index.get(&v).ok_or_else(|| {
                        anyhow!("Invalid subtask name `{}` in dependency json!", v)
                    })?;
                    // 存在边u->v
                    graph[*u_idx].push(*v_idx);
                    rev_graph[*v_idx].push(*u_idx);
                    outdeg[*u_idx] += 1;
                }
            }
        }
        let mut heap = BinaryHeap::new();
        for (idx, v) in outdeg.iter().enumerate() {
            if *v == 0 {
                heap.push(Reverse(idx));
            }
        }
        Ok(Self {
            name_to_index,
            index_to_name,
            rev_graph,
            graph,
            outdeg,
            heap,
            dropped: vec![false; names.len()],
        })
    }
    pub fn next(&mut self) -> Option<String> {
        if let Some(v) = self.heap.peek() {
            let idx = v.0;

            Some(self.index_to_name[idx].clone())
        } else {
            None
        }
    }
    pub fn report(&mut self, ok: bool) {
        if let Some(Reverse(idx)) = self.heap.pop() {
            if ok {
                for from_idx in self.rev_graph[idx].iter() {
                    let r = &mut self.outdeg[*from_idx];
                    *r -= 1;
                    if *r == 0 {
                        self.heap.push(Reverse(*from_idx));
                    }
                }
                self.dropped[idx] = true;
            }
        }
    }
    pub fn get_skipped_subtasks(&self) -> Vec<SkippedSubtask> {
        let mut result = vec![];
        for i in 0..self.name_to_index.len() {
            if self.outdeg[i] != 0 {
                result.push(SkippedSubtask {
                    name: self.index_to_name[i].clone(),
                    reason: format!(
                        "Skipped for failing `{}`",
                        self.graph[i]
                            .iter()
                            .filter(|s| !self.dropped[**s])
                            .map(|v| self.index_to_name[*v].clone())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                })
            }
        }
        result
    }
}
