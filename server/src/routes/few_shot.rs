//! Few-Shot 片段库与检索端点（REQ-202610-1183）。
//!
//! - `GET  /api/v1/few-shot/snippets`：分页列出片段；
//! - `POST /api/v1/few-shot/snippets`：登记片段（生成管线 / 人工策展）；
//! - `POST /api/v1/few-shot/snippets/{id}/feedback`：记录满意度反馈；
//! - `POST /api/v1/few-shot/retrieve`：按场景 / 画像 / 语义检索并注入 Prompt。
//!
//! 所有端点要求 Bearer 鉴权；检索请求中的 `user_id` 必须与凭据归属一致，
//! 防止横向越权读取他人偏好。写接口在配置 `FEW_SHOT_ADMIN_TOKEN` 后还需
//! 携带 `X-Few-Shot-Token`。

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::error::AppError;
use crate::few_shot::{
    ComponentSnippet, FewShotConfig, FewShotResult, FewShotService, ListSnippetsQuery,
    RegisterSnippetRequest, RetrieveRequest, SnippetFeedbackRequest, SnippetSource, SnippetStyle,
};
use crate::middleware::auth::AuthUser;
use crate::repository::Repository;
use crate::response::ApiResponse;
use crate::state::AppState;

/// 列表响应。
#[derive(Debug, Serialize)]
pub struct SnippetListResponse {
    /// 当前页片段。
    pub items: Vec<SnippetView>,
    /// 符合条件的总数。
    pub total: i64,
    /// 本页条数。
    pub limit: i64,
    /// 偏移量。
    pub offset: i64,
}

/// 片段对外结构。
#[derive(Debug, Serialize)]
pub struct SnippetView {
    /// 主键。
    pub id: String,
    /// 来源类型。
    pub source_type: SnippetSource,
    /// 来源引用。
    pub source_ref: Option<String>,
    /// 场景编码。
    pub scene_code: String,
    /// 标题。
    pub title: String,
    /// 片段 HTML。
    pub snippet_html: String,
    /// 检索用纯文本。
    pub snippet_text: String,
    /// 标签。
    pub tags: Vec<String>,
    /// 风格特征。
    pub style: SnippetStyle,
    /// 显式满意度。
    pub satisfaction: f64,
    /// 隐式满意度。
    pub implicit_score: f64,
    /// 使用次数。
    pub usage_count: u64,
    /// 正向反馈数。
    pub positive_feedback: u64,
    /// 负向反馈数。
    pub negative_feedback: u64,
    /// 是否启用。
    pub enabled: bool,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
    /// 更新时间。
    pub updated_at: DateTime<Utc>,
}

impl From<ComponentSnippet> for SnippetView {
    fn from(snippet: ComponentSnippet) -> Self {
        Self {
            id: snippet.id,
            source_type: snippet.source_type,
            source_ref: snippet.source_ref,
            scene_code: snippet.scene_code,
            title: snippet.title,
            snippet_html: snippet.snippet_html,
            snippet_text: snippet.snippet_text,
            tags: snippet.tags,
            style: snippet.style,
            satisfaction: snippet.satisfaction,
            implicit_score: snippet.implicit_score,
            usage_count: snippet.usage_count,
            positive_feedback: snippet.positive_feedback,
            negative_feedback: snippet.negative_feedback,
            enabled: snippet.enabled,
            created_at: snippet.created_at,
            updated_at: snippet.updated_at,
        }
    }
}

/// `GET /api/v1/few-shot/snippets`
pub async fn list_snippets(
    State(state): State<AppState>,
    Query(query): Query<ListSnippetsQuery>,
    _auth: AuthUser,
) -> Result<Json<ApiResponse<SnippetListResponse>>, AppError> {
    query.validate().map_err(AppError::invalid_param)?;
    let (limit, offset) = query.pagination();
    let repository = Repository::from_db(&state.db);
    let (snippets, total) = repository
        .list_few_shot_snippets(query.scene_code.as_deref(), limit, offset)
        .await?;

    Ok(Json(ApiResponse::success(SnippetListResponse {
        items: snippets.into_iter().map(SnippetView::from).collect(),
        total,
        limit,
        offset,
    })))
}

/// `POST /api/v1/few-shot/snippets`
pub async fn register_snippet(
    State(state): State<AppState>,
    headers: HeaderMap,
    _auth: AuthUser,
    Json(request): Json<RegisterSnippetRequest>,
) -> Result<Json<ApiResponse<SnippetView>>, AppError> {
    let service = build_service(&state);
    ensure_admin(&service, &headers)?;
    let snippet = service.register(request).await?;
    Ok(Json(ApiResponse::success(SnippetView::from(snippet))))
}

/// `POST /api/v1/few-shot/snippets/{id}/feedback`
pub async fn snippet_feedback(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    _auth: AuthUser,
    Json(request): Json<SnippetFeedbackRequest>,
) -> Result<Json<ApiResponse<SnippetView>>, AppError> {
    let service = build_service(&state);
    ensure_admin(&service, &headers)?;
    request.validate().map_err(AppError::invalid_param)?;
    let snippet = service.feedback(&id, request.satisfaction).await?;
    Ok(Json(ApiResponse::success(SnippetView::from(snippet))))
}

/// `POST /api/v1/few-shot/retrieve`
pub async fn retrieve(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(request): Json<RetrieveRequest>,
) -> Result<Json<ApiResponse<FewShotResult>>, AppError> {
    request.validate().map_err(AppError::invalid_param)?;
    if let Some(user_id) = &request.user_id {
        if user_id != &auth.user_id {
            return Err(AppError::forbidden());
        }
    }

    let service = build_service(&state);
    let result = service
        .retrieve(&auth.user_id, request.into_query())
        .await?;
    Ok(Json(ApiResponse::success(result)))
}

fn build_service(state: &AppState) -> FewShotService {
    FewShotService::new(
        Repository::from_db(&state.db),
        FewShotConfig::from_app(&state.config),
    )
}

/// 写接口管理令牌校验；未配置令牌时保持开发模式可用。
fn ensure_admin(service: &FewShotService, headers: &HeaderMap) -> Result<(), AppError> {
    let Some(expected) = service.config().admin_token.as_deref() else {
        return Ok(());
    };
    let provided = headers
        .get("x-few-shot-token")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if constant_time_eq(provided.as_bytes(), expected.as_bytes()) {
        Ok(())
    } else {
        Err(AppError::forbidden())
    }
}

/// 常量时间比较，避免按字节短路泄露令牌前缀信息。
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (left_byte, right_byte) in left.iter().zip(right.iter()) {
        diff |= left_byte ^ right_byte;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_eq_matches_equal_values() {
        assert!(constant_time_eq(b"token", b"token"));
        assert!(!constant_time_eq(b"token", b"toke"));
        assert!(!constant_time_eq(b"token", b"other"));
    }
}
