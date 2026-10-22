//! 偏好对业务编排：入库、调研归集、审核、抽样与导出。

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use chrono::{DateTime, Utc};
use serde_json::json;
use sha2::Digest;

use crate::preference::cleaning::{clean_pair, salted_hash};
use crate::preference::config::PreferenceConfig;
use crate::preference::model::{
    DpoMetadata, DpoRecord, ExportBundle, ExportManifest, ExportOptions, ExportSkipStats,
    FromSurveyInput, IngestOutcome, PairDraft, PairFilter, PairPage, PairStatus, PreferenceError,
    PreferenceExportRecord, PreferencePair, PreferenceStats, ReviewDecision, SampleResult,
};
use crate::repository::{new_id, Repository};

/// 单次抽样扫描的最大候选数。
const SAMPLE_SCAN_LIMIT: i64 = 5_000;
/// 单次导出扫描的最大候选数。
const EXPORT_SCAN_LIMIT: usize = 20_000;
/// HTTP / CLI 单次导出的字节预算（16 MiB）。
const EXPORT_BYTE_BUDGET: usize = 16 * 1024 * 1024;

/// 偏好数据服务。
pub struct PreferenceService<'a> {
    repository: &'a Repository,
    config: &'a PreferenceConfig,
}

impl<'a> PreferenceService<'a> {
    /// 构造服务。
    pub fn new(repository: &'a Repository, config: &'a PreferenceConfig) -> Self {
        Self { repository, config }
    }

    /// 通用入库（隐式指标 / A/B / 人工）。
    pub async fn ingest(
        &self,
        user_id: &str,
        draft: PairDraft,
    ) -> Result<IngestOutcome, PreferenceError> {
        let now = Utc::now();
        let cleaned =
            clean_pair(&draft, user_id, self.config, now).map_err(PreferenceError::Invalid)?;

        let pair = PreferencePair {
            id: new_id("PREF"),
            pair_key: cleaned.pair_key,
            content_hash: cleaned.content_hash,
            prompt: cleaned.prompt,
            chosen_html: cleaned.chosen_html,
            rejected_html: cleaned.rejected_html,
            source: draft.source,
            scene_code: draft.scene_code,
            user_id: user_id.to_string(),
            session_id: draft.session_id,
            request_id: draft.request_id,
            survey_id: draft.survey_id,
            prompt_template_version: draft.prompt_template_version,
            chosen_model: draft.chosen_model,
            rejected_model: draft.rejected_model,
            chosen_style_id: draft.chosen_style_id,
            rejected_style_id: draft.rejected_style_id,
            pipeline_version: draft.pipeline_version,
            preference_strength: draft.preference_strength,
            quality_score: cleaned.quality_score,
            quality_flags: cleaned.quality_flags,
            metadata: cleaned.metadata,
            status: cleaned.status,
            review_note: cleaned.review_note,
            reviewed_by: None,
            reviewed_at: None,
            exported_at: None,
            occurred_at: cleaned.occurred_at,
            created_at: now,
            updated_at: now,
        };

        self.repository
            .insert_preference_pair(&pair)
            .await
            .map_err(Into::into)
    }

    /// 由显式调研生成偏好对：溯源字段从调研记录读取。
    pub async fn from_survey(
        &self,
        user_id: &str,
        input: FromSurveyInput,
    ) -> Result<IngestOutcome, PreferenceError> {
        let survey = self
            .repository
            .find_telemetry_survey(user_id, &input.survey_id)
            .await?
            .ok_or_else(|| {
                PreferenceError::NotFound(format!("survey not found: {}", input.survey_id))
            })?;

        let metadata = merge_survey_metadata(input.metadata, &survey);
        let draft = PairDraft {
            prompt: input.prompt,
            chosen_html: input.chosen_html,
            rejected_html: input.rejected_html,
            source: "EXPLICIT_SURVEY".to_string(),
            scene_code: survey.scene_code.clone(),
            session_id: survey.session_id.clone(),
            request_id: None,
            survey_id: Some(survey.survey_id.clone()),
            prompt_template_version: input
                .prompt_template_version
                .unwrap_or_else(|| "unknown".to_string()),
            chosen_model: input.chosen_model,
            rejected_model: input.rejected_model,
            chosen_style_id: Some(survey.selected_style_id.clone()),
            rejected_style_id: Some(survey.rejected_style_id.clone()),
            pipeline_version: input.pipeline_version,
            preference_strength: input.preference_strength.unwrap_or(1.0),
            metadata,
            occurred_at: Some(survey.timestamp),
        };
        self.ingest(user_id, draft).await
    }

    /// 查询单条。
    pub async fn get(&self, id: &str) -> Result<PreferencePair, PreferenceError> {
        self.repository
            .get_preference_pair(id)
            .await?
            .ok_or_else(|| PreferenceError::NotFound(format!("preference pair not found: {id}")))
    }

    /// 分页查询。
    pub async fn list(
        &self,
        filter: &PairFilter,
        limit: i64,
        offset: i64,
    ) -> Result<PairPage, PreferenceError> {
        let (items, total) = self
            .repository
            .list_preference_pairs(filter, limit, offset)
            .await?;
        Ok(PairPage {
            items,
            total,
            limit,
            offset,
        })
    }

    /// 人工审核。
    pub async fn review(
        &self,
        id: &str,
        reviewer: &str,
        decision: ReviewDecision,
        note: Option<String>,
    ) -> Result<PreferencePair, PreferenceError> {
        let pair = self.get(id).await?;
        if pair.status == PairStatus::Exported.as_str() {
            return Err(PreferenceError::Conflict(
                "exported preference pair cannot be reviewed".to_string(),
            ));
        }
        let status = decision.target_status().as_str();
        self.repository
            .update_preference_pair_review(id, status, note.as_deref(), reviewer, Utc::now())
            .await?
            .ok_or_else(|| PreferenceError::NotFound(format!("preference pair not found: {id}")))
    }

    /// 统计。
    pub async fn stats(&self) -> Result<PreferenceStats, PreferenceError> {
        Ok(self.repository.preference_stats().await?)
    }

    /// 确定性分层抽样：先按 `sha256(seed:id)` 排序，再按场景轮转取数。
    pub async fn sample(
        &self,
        filter: &PairFilter,
        limit: usize,
        seed: &str,
    ) -> Result<SampleResult, PreferenceError> {
        let limit = limit.clamp(1, 200);
        let (candidates, _) = self
            .repository
            .list_preference_pairs(filter, SAMPLE_SCAN_LIMIT, 0)
            .await?;
        let candidate_count = candidates.len();
        let items = stratified_sample(candidates, limit, seed);
        Ok(SampleResult {
            seed: seed.to_string(),
            limit,
            candidate_count,
            items,
        })
    }

    /// 构建并（默认）落盘导出记录：仅 ACCEPTED（可选 PENDING），按内容去重并执行配额。
    pub async fn export(&self, options: &ExportOptions) -> Result<ExportBundle, PreferenceError> {
        let limit = options.limit.min(self.config.export_max_records);
        let filter = PairFilter {
            statuses: if options.include_pending {
                vec![
                    PairStatus::Pending.as_str().to_string(),
                    PairStatus::Accepted.as_str().to_string(),
                ]
            } else {
                vec![PairStatus::Accepted.as_str().to_string()]
            },
            source: options.source.clone(),
            scene_code: options.scene_code.clone(),
            min_quality: Some(options.min_quality),
            ..PairFilter::default()
        };
        let scan_limit = (limit.saturating_mul(20)).clamp(1_000, EXPORT_SCAN_LIMIT) as i64;
        let (candidates, _) = self
            .repository
            .list_preference_pairs(&filter, scan_limit, 0)
            .await?;

        let now = Utc::now();
        let mut selection = Selection::default();
        let mut records = Vec::new();
        let mut jsonl = String::new();
        for pair in candidates {
            if records.len() >= limit {
                break;
            }
            if !selection.content_hashes.insert(pair.content_hash.clone()) {
                selection.skipped.duplicate_content += 1;
                continue;
            }
            let user_count = selection
                .user_counts
                .entry(pair.user_id.clone())
                .or_insert(0);
            if *user_count >= options.max_per_user {
                selection.skipped.user_quota += 1;
                continue;
            }
            let scene_count = selection
                .scene_counts
                .entry(pair.scene_code.clone())
                .or_insert(0);
            if *scene_count >= options.max_per_scene {
                selection.skipped.scene_quota += 1;
                continue;
            }
            *user_count += 1;
            *scene_count += 1;

            let record = build_dpo_record(&pair, &self.config.user_hash_salt);
            let line = serde_json::to_string(&record).map_err(|error| {
                PreferenceError::Internal(format!("failed to serialize record: {error}"))
            })?;
            if jsonl.len() + line.len() + 1 > EXPORT_BYTE_BUDGET {
                selection.skipped.byte_budget += 1;
                continue;
            }
            jsonl.push_str(&line);
            jsonl.push('\n');
            selection
                .unique_users
                .insert(record.metadata.user_hash.clone());
            *selection
                .source_counts
                .entry(pair.source.clone())
                .or_insert(0) += 1;
            *selection
                .scene_counts_export
                .entry(pair.scene_code.clone())
                .or_insert(0) += 1;
            records.push(record);
        }

        let mut hasher = sha2::Sha256::new();
        hasher.update(jsonl.as_bytes());
        let sha256 = hex::encode(hasher.finalize());
        let dataset_id = new_id("DS");
        let manifest = ExportManifest {
            dataset_id: dataset_id.clone(),
            format: "dpo-jsonl-v1".to_string(),
            created_at: now,
            record_count: records.len(),
            source_counts: selection.source_counts,
            scene_counts: selection.scene_counts_export,
            unique_user_count: selection.unique_users.len(),
            min_quality: options.min_quality,
            max_per_user: options.max_per_user,
            max_per_scene: options.max_per_scene,
            sha256,
            user_hash_fingerprint: salted_hash(&self.config.user_hash_salt, "v1")[..12].to_string(),
            skipped: selection.skipped,
            filters: json!({
                "scene_code": options.scene_code,
                "source": options.source,
                "include_pending": options.include_pending,
                "limit": limit,
            }),
        };

        let bundle = ExportBundle { manifest, records };
        if !options.dry_run {
            self.commit_export(&bundle).await?;
        }
        Ok(bundle)
    }

    /// 落库导出批次并把记录标记为 `EXPORTED`。
    ///
    /// CLI 先以 `dry_run` 构建并写盘，成功后再调用本方法，避免「已标记导出但
    /// 文件写入失败」。
    pub async fn commit_export(&self, bundle: &ExportBundle) -> Result<(), PreferenceError> {
        let manifest_json = serde_json::to_string(&bundle.manifest).map_err(|error| {
            PreferenceError::Internal(format!("failed to serialize manifest: {error}"))
        })?;
        let export = PreferenceExportRecord {
            id: new_id("EXP"),
            dataset_id: bundle.manifest.dataset_id.clone(),
            format: bundle.manifest.format.clone(),
            record_count: bundle.manifest.record_count as i64,
            sha256: bundle.manifest.sha256.clone(),
            filters_json: bundle.manifest.filters.to_string(),
            manifest_json,
            created_at: bundle.manifest.created_at,
        };
        self.repository.insert_preference_export(&export).await?;
        let ids: Vec<String> = bundle
            .records
            .iter()
            .map(|record| record.id.clone())
            .collect();
        self.repository
            .mark_preference_pairs_exported(&ids, bundle.manifest.created_at)
            .await?;
        Ok(())
    }
}

/// 导出选择过程的状态。
#[derive(Default)]
struct Selection {
    content_hashes: HashSet<String>,
    user_counts: HashMap<String, usize>,
    scene_counts: HashMap<String, usize>,
    unique_users: HashSet<String>,
    source_counts: BTreeMap<String, usize>,
    scene_counts_export: BTreeMap<String, usize>,
    skipped: ExportSkipStats,
}

/// 合并调研溯源信息到元数据。
fn merge_survey_metadata(
    metadata: Option<serde_json::Value>,
    survey: &crate::telemetry_survey::TelemetrySurvey,
) -> serde_json::Value {
    let mut value = metadata.unwrap_or_else(|| json!({}));
    if let serde_json::Value::Object(map) = &mut value {
        map.insert("survey_id".to_string(), json!(survey.survey_id));
        map.insert(
            "selected_style_id".to_string(),
            json!(survey.selected_style_id),
        );
        map.insert(
            "rejected_style_id".to_string(),
            json!(survey.rejected_style_id),
        );
        map.insert("survey_timestamp".to_string(), json!(survey.timestamp));
    }
    value
}

/// 构造单条 DPO 导出记录（用户 / 会话均假名化）。
fn build_dpo_record(pair: &PreferencePair, salt: &str) -> DpoRecord {
    let user_hash = format!("u_{}", &salted_hash(salt, &pair.user_id)[..16]);
    let session_hash = pair
        .session_id
        .as_deref()
        .map(|session| format!("s_{}", &salted_hash(salt, session)[..16]));
    DpoRecord {
        id: pair.id.clone(),
        prompt: pair.prompt.clone(),
        chosen: pair.chosen_html.clone(),
        rejected: pair.rejected_html.clone(),
        metadata: DpoMetadata {
            source: pair.source.clone(),
            scene_code: pair.scene_code.clone(),
            user_hash,
            session_hash,
            request_id: pair.request_id.clone(),
            survey_id: pair.survey_id.clone(),
            prompt_template_version: pair.prompt_template_version.clone(),
            chosen_model: pair.chosen_model.clone(),
            rejected_model: pair.rejected_model.clone(),
            chosen_style_id: pair.chosen_style_id.clone(),
            rejected_style_id: pair.rejected_style_id.clone(),
            pipeline_version: pair.pipeline_version.clone(),
            preference_strength: pair.preference_strength,
            quality_score: pair.quality_score,
            quality_flags: pair.quality_flags.clone(),
            occurred_at: pair.occurred_at,
            created_at: pair.created_at,
        },
    }
}

/// 确定性分层抽样：场景轮转，保证小场景也有样本。
pub fn stratified_sample(
    pairs: Vec<PreferencePair>,
    limit: usize,
    seed: &str,
) -> Vec<PreferencePair> {
    let mut sorted = pairs;
    sorted.sort_by_key(|pair| sample_key(seed, &pair.id));

    let mut groups: BTreeMap<String, VecDeque<PreferencePair>> = BTreeMap::new();
    for pair in sorted {
        groups
            .entry(pair.scene_code.clone())
            .or_default()
            .push_back(pair);
    }
    let scenes: Vec<String> = groups.keys().cloned().collect();
    let mut selected = Vec::new();
    while selected.len() < limit {
        let mut progressed = false;
        for scene in &scenes {
            if let Some(pair) = groups.get_mut(scene).and_then(VecDeque::pop_front) {
                selected.push(pair);
                progressed = true;
                if selected.len() == limit {
                    break;
                }
            }
        }
        if !progressed {
            break;
        }
    }
    selected
}

/// 抽样排序键：`sha256(seed:id)`。
fn sample_key(seed: &str, id: &str) -> String {
    salted_hash(seed, id)
}

/// 解析 ISO 时间（供路由与 CLI 共用）。
pub fn parse_timestamp(value: &str) -> Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(value)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(|_| "must be an RFC3339 timestamp".to_string())
}
