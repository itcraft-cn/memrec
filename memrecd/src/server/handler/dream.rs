//! # Dream 与元信息处理器
//!
//! [`Router`] 的 Dream、DreamReport、GetProjectInfo、GetVersion 方法。

use super::*;

impl Router {
    pub(super) async fn handle_dream(&self, params: Option<RequestParams>, id: u64) -> JsonRpcResponse {
        let (force, dry_run) = match params {
            Some(RequestParams::Dream(p)) => (p.force, p.dry_run),
            _ => (false, false),
        };

        match self.dream_processor.execute_ext(force, dry_run).await {
            Ok(result) => JsonRpcResponse::success(
                ResponseResult::Dream(DreamResult {
                    integrated_count: result.integrated_count,
                    created_memory_id: result.created_memory_id,
                    summary: result.summary,
                    run_id: result.run_id,
                    cluster_count: result.cluster_count,
                    contradiction_count: result.contradiction_count,
                    edge_count: result.edge_count,
                    propagation_count: result.propagation_count,
                    dry_run: result.dry_run,
                    phase_results: result.phase_results.iter().map(|p| p.to_record()).collect(),
                    clues: result.summary_clues,
                    metrics: result.metrics,
                }),
                id,
            ),
            Err(e) => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32006,
                    message: e.to_string(),
                    data: None,
                },
                id,
            ),
        }
    }

    /// 处理 DreamReport 请求：返回最近的 DreamRun 审计报告。
    pub(super) async fn handle_dream_report(&self, params: Option<RequestParams>, id: u64) -> JsonRpcResponse {
        let (last, offset) = match params {
            Some(RequestParams::DreamReport(p)) => (p.last.unwrap_or(10), p.offset.unwrap_or(0)),
            _ => (10, 0),
        };

        let Some(store) = &self.dream_run_store else {
            return JsonRpcResponse::success(
                ResponseResult::DreamReport(DreamReportResult {
                    runs: Vec::new(),
                    total: 0,
                }),
                id,
            );
        };

        let last = last.min(100);
        match store.list_runs(last, offset).await {
            Ok(runs) => {
                let total = store.count_runs().await.unwrap_or(runs.len());
                JsonRpcResponse::success(
                    ResponseResult::DreamReport(DreamReportResult { runs, total }),
                    id,
                )
            }
            Err(e) => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32007,
                    message: e.to_string(),
                    data: None,
                },
                id,
            ),
        }
    }

    /// 处理 GetProjectInfo 请求：返回项目 ID、根目录和记忆数量。
    pub(super) async fn handle_project_info(&self, params: Option<RequestParams>, id: u64) -> JsonRpcResponse {
        let params: GetProjectInfoParams = match params {
            Some(RequestParams::GetProjectInfo(p)) => p,
            _ => GetProjectInfoParams::default(),
        };

        match detect_project_id(params.working_dir.as_deref()) {
            Ok(project_id) => {
                let project_root = find_project_root(params.working_dir.as_deref())
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();

                let mr_pid_path = std::path::Path::new(&project_root).join(".mr_pid");
                let mr_pid_exists = mr_pid_path.exists();

                let memory_count = self
                    .storage
                    .list_by_project(&project_id)
                    .await
                    .unwrap_or_default()
                    .len();

                JsonRpcResponse::success(
                    ResponseResult::ProjectInfo(ProjectInfoResult {
                        project_id,
                        project_name: None,
                        project_root,
                        memory_count,
                        mr_pid_exists,
                    }),
                    id,
                )
            }
            Err(e) => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32004,
                    message: e.to_string(),
                    data: None,
                },
                id,
            ),
        }
    }

    /// 处理 GetVersion 请求：返回守护进程版本号。
    pub(super) async fn handle_version(&self, id: u64) -> JsonRpcResponse {
        JsonRpcResponse::success(
            ResponseResult::Version(VersionResult {
                version: env!("CARGO_PKG_VERSION").to_string(),
            }),
            id,
        )
    }
}
