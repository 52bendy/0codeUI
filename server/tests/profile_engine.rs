//! 用户画像引擎单元级测试（REQ-202610-1176）。
//!
//! 覆盖验收标准：
//! - 构造行为序列后画像按 EWMA 收敛；
//! - 单次异常停留 / 误触不引起显著变化（限幅 + 死区）；
//! - 更新频率控制、冷启动默认画像与置信度标记；
//! - 画像 JSON 可直接注入 Prompt。

use chrono::{Duration, TimeZone, Utc};
use intimeui_server::profile::{
    Observation, ObservationSource, PartialPreferenceVector, ProfileEngine, ProfileEngineConfig,
    ProfileUpdate, Theme, UpdateSkipReason,
};

fn engine(min_update_secs: i64) -> ProfileEngine {
    ProfileEngine::new(ProfileEngineConfig {
        base_alpha: 0.15,
        max_alpha: 0.6,
        min_update_interval: Duration::seconds(min_update_secs),
    })
}

fn now() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 22, 8, 0, 0).unwrap()
}

fn telemetry_observation(
    vector: PartialPreferenceVector,
    weight: f64,
    at: chrono::DateTime<Utc>,
) -> Observation {
    Observation {
        vector,
        weight,
        source: ObservationSource::Telemetry,
        observed_at: at,
    }
}

fn target_vector() -> PartialPreferenceVector {
    PartialPreferenceVector {
        font_scale: Some(1.2),
        density: Some(0.2),
        theme_score: None,
        radius: Some(16.0),
        spacing: Some(1.2),
    }
}

#[test]
fn cold_start_uses_defaults_and_zero_confidence() {
    let profile = engine(0).cold_start("usr_cold", now());

    assert_eq!(profile.user_id, "usr_cold");
    assert_eq!(profile.preferences.font_scale, 1.0);
    assert_eq!(profile.preferences.density, 0.5);
    assert_eq!(profile.preferences.theme(), Theme::Light);
    assert_eq!(profile.preferences.radius, 8.0);
    assert_eq!(profile.preferences.spacing, 1.0);
    assert_eq!(profile.confidence, 0.0);
    assert_eq!(profile.sample_count, 0);
    assert_eq!(profile.version, 1);
    assert!(profile.cold_start);
    assert!(profile.updated_at.is_none());
}

#[test]
fn repeated_observations_converge_by_ewma() {
    let engine = engine(0);
    let mut profile = engine.cold_start("usr_converge", now());

    for step in 0..40 {
        let observation =
            telemetry_observation(target_vector(), 1.0, now() + Duration::seconds(step));
        let outcome = engine.apply_observations(
            &profile,
            &[observation],
            now() + Duration::seconds(step),
            true,
        );
        assert!(outcome.updated, "step {step} should update the profile");
        profile = outcome.profile;
    }

    assert!((profile.preferences.font_scale - 1.2).abs() < 0.01);
    assert!((profile.preferences.density - 0.2).abs() < 0.01);
    assert!((profile.preferences.radius - 16.0).abs() < 0.1);
    assert!((profile.preferences.spacing - 1.2).abs() < 0.01);
    assert!(profile.confidence > 0.8);
    assert!(!profile.cold_start);
    assert_eq!(profile.sample_count, 40);
}

#[test]
fn single_outlier_is_clamped_by_max_step() {
    let engine = engine(0);
    let profile = engine.cold_start("usr_outlier", now());

    let outlier = telemetry_observation(
        PartialPreferenceVector {
            font_scale: Some(1.4),
            density: Some(1.0),
            theme_score: Some(1.0),
            radius: Some(24.0),
            spacing: Some(1.5),
        },
        1.0,
        now(),
    );

    let outcome = engine.apply_observations(&profile, &[outlier], now(), true);

    assert!(outcome.updated);
    // 单次异常最多只能移动一个限幅步长，不能造成 UI 剧烈抖动。
    assert!((outcome.profile.preferences.font_scale - 1.0).abs() <= 0.05 + 1e-9);
    assert!((outcome.profile.preferences.density - 0.5).abs() <= 0.08 + 1e-9);
    assert!((outcome.profile.preferences.theme_score - 0.0).abs() <= 0.10 + 1e-9);
    assert!((outcome.profile.preferences.radius - 8.0).abs() <= 2.0 + 1e-9);
    assert!((outcome.profile.preferences.spacing - 1.0).abs() <= 0.06 + 1e-9);
    assert_eq!(outcome.profile.preferences.theme(), Theme::Light);
}

#[test]
fn update_frequency_guard_skips_rapid_updates() {
    let engine = engine(60);
    let profile = engine.cold_start("usr_rate", now());
    let observation = telemetry_observation(target_vector(), 1.0, now());

    let first =
        engine.apply_observations(&profile, std::slice::from_ref(&observation), now(), false);
    assert!(first.updated);

    let second = engine.apply_observations(
        &first.profile,
        &[observation],
        now() + Duration::seconds(1),
        false,
    );
    assert!(!second.updated);
    assert_eq!(second.skipped_reason, Some(UpdateSkipReason::RateLimited));

    // force = true 时允许立即重算（供运维 / 离线补算使用）。
    let forced = engine.apply_observations(
        &first.profile,
        &[telemetry_observation(target_vector(), 1.0, now())],
        now() + Duration::seconds(1),
        true,
    );
    assert!(forced.updated);
}

#[test]
fn deadband_skips_negligible_changes() {
    let engine = engine(0);
    let profile = engine.cold_start("usr_deadband", now());
    let tiny = telemetry_observation(
        PartialPreferenceVector {
            font_scale: Some(1.001),
            density: None,
            theme_score: None,
            radius: None,
            spacing: None,
        },
        1.0,
        now(),
    );

    let outcome = engine.apply_observations(&profile, &[tiny], now(), true);
    assert!(!outcome.updated);
    assert_eq!(outcome.skipped_reason, Some(UpdateSkipReason::NoChange));
    assert_eq!(outcome.profile.version, 1);
}

#[test]
fn survey_style_tokens_map_to_preference_targets() {
    let observation = Observation::from_survey(
        "style_dark_compact_large_font_rounded_wide_spacing",
        "style_light_spacious_small_font_sharp_tight_spacing",
        now(),
    );

    assert_eq!(observation.source, ObservationSource::Survey);
    assert_eq!(observation.weight, 1.0);
    assert_eq!(observation.vector.theme_score, Some(1.0));
    assert_eq!(observation.vector.density, Some(0.15));
    assert_eq!(observation.vector.font_scale, Some(1.2));
    assert_eq!(observation.vector.radius, Some(16.0));
    assert_eq!(observation.vector.spacing, Some(1.2));
}

#[test]
fn theme_flips_only_after_repeated_dark_surveys() {
    let engine = engine(0);
    let mut profile = engine.cold_start("usr_theme", now());
    let survey = Observation::from_survey("style_dark", "style_light", now());

    let first = engine.apply_observations(&profile, std::slice::from_ref(&survey), now(), true);
    assert!(first.updated);
    assert_eq!(
        first.profile.preferences.theme(),
        Theme::Light,
        "single survey must not flip the theme"
    );
    profile = first.profile;

    for step in 1..12 {
        let outcome = engine.apply_observations(
            &profile,
            std::slice::from_ref(&survey),
            now() + Duration::seconds(step),
            true,
        );
        profile = outcome.profile;
    }
    assert_eq!(profile.preferences.theme(), Theme::Dark);
}

#[test]
fn prompt_profile_is_directly_injectable() {
    let profile = engine(0).cold_start("usr_prompt", now());
    let value = serde_json::to_value(profile.prompt_profile()).unwrap();
    let keys: Vec<&str> = value
        .as_object()
        .expect("prompt profile must be a JSON object")
        .keys()
        .map(String::as_str)
        .collect();

    assert_eq!(
        keys,
        vec![
            "cold_start",
            "confidence",
            "density",
            "font_scale",
            "radius",
            "spacing",
            "theme"
        ]
    );
    assert_eq!(value["theme"], "light");
    assert_eq!(value["font_scale"], 1.0);
    assert_eq!(value["cold_start"], true);
}

#[test]
fn explicit_update_applies_values_and_bumps_version() {
    let engine = engine(0);
    let profile = engine.cold_start("usr_explicit", now());
    let update = ProfileUpdate {
        font_scale: Some(1.1),
        density: None,
        theme: Some(Theme::Dark),
        radius: None,
        spacing: None,
    };

    let updated = engine.apply_explicit(&profile, &update, now());

    assert_eq!(updated.preferences.font_scale, 1.1);
    assert_eq!(updated.preferences.theme(), Theme::Dark);
    assert_eq!(updated.preferences.density, 0.5);
    assert_eq!(updated.version, 2);
    assert!(!updated.cold_start);
    assert!(updated.confidence >= 0.5);
    assert_eq!(updated.updated_at, Some(now()));
}
