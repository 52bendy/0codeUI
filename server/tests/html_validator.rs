//! 集成测试：单文件 HTML 输出校验器（REQ-202610-1173）。
//!
//! 覆盖全部错误类型：结构完整性、单文件约束、安全基线、定位信息与配置能力。

use intimeui_server::html_validator::{
    validate_single_file_html, validate_single_file_html_with_config, IssueKind, Severity,
    ValidatorConfig,
};

/// 合法样例：结构完整、CSS/JS 内联、事件属性在白名单内、图片使用 data URI。
const VALID_PAGE: &str = r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>订单详情</title>
  <style>
    :root { --bg: #121212; --fg: #ffffff; }
    body { margin: 0; background: var(--bg); color: var(--fg); }
    .card { border-radius: 12px; padding: 16px; }
    .card > h1 { font-size: 20px; }
  </style>
</head>
<body>
  <main class="card">
    <h1>订单详情</h1>
    <p>金额：299.00 元</p>
    <button type="button" onclick="confirmOrder()">确认</button>
    <img src="data:image/png;base64,iVBORw0KGgo=" alt="图标">
  </main>
  <script>
    function confirmOrder() {
      if (1 < 2) { document.body.dataset.confirmed = '1'; }
      fetch('/api/v1/orders/ORD_20261014_001/business-data');
    }
  </script>
</body>
</html>
"#;

fn assert_invalid_with(html: &str, kind: IssueKind) {
    let result = validate_single_file_html(html);
    assert!(!result.valid, "expected invalid, got: {result:#?}");
    assert!(
        result.issues.iter().any(|issue| issue.kind == kind),
        "expected issue {kind:?}, got {:?}",
        result
            .issues
            .iter()
            .map(|issue| issue.kind)
            .collect::<Vec<_>>()
    );
}

fn kinds(result: &intimeui_server::html_validator::ValidationResult) -> Vec<IssueKind> {
    result.issues.iter().map(|issue| issue.kind).collect()
}

#[test]
fn valid_single_file_page_passes() {
    let result = validate_single_file_html(VALID_PAGE);
    assert!(result.valid, "valid page rejected: {:#?}", result.issues);
    assert!(result.issues.is_empty(), "unexpected warnings: {result:#?}");
    assert_eq!(result.stats.elements, 13);
    assert_eq!(result.stats.scripts, 1);
    assert_eq!(result.stats.styles, 1);
}

#[test]
fn empty_document_is_rejected() {
    assert_invalid_with("", IssueKind::EmptyDocument);
    assert_invalid_with("   \n\t ", IssueKind::EmptyDocument);
}

#[test]
fn missing_doctype_is_rejected() {
    assert_invalid_with(
        "<html><head></head><body></body></html>",
        IssueKind::MissingDoctype,
    );
}

#[test]
fn invalid_doctype_is_rejected() {
    assert_invalid_with(
        "<!DOCTYPE xml><html><head></head><body></body></html>",
        IssueKind::InvalidDoctype,
    );
}

#[test]
fn doctype_after_content_is_rejected() {
    assert_invalid_with(
        "<html><head></head><body></body></html><!DOCTYPE html>",
        IssueKind::DoctypeNotFirst,
    );
}

#[test]
fn missing_head_is_rejected() {
    assert_invalid_with(
        "<!DOCTYPE html><html><body></body></html>",
        IssueKind::MissingStructureElement,
    );
}

#[test]
fn missing_body_is_rejected() {
    assert_invalid_with(
        "<!DOCTYPE html><html><head></head></html>",
        IssueKind::MissingStructureElement,
    );
}

#[test]
fn missing_html_is_rejected() {
    assert_invalid_with(
        "<!DOCTYPE html><head></head><body></body>",
        IssueKind::MissingStructureElement,
    );
}

#[test]
fn duplicate_head_is_rejected() {
    assert_invalid_with(
        "<!DOCTYPE html><html><head></head><head></head><body></body></html>",
        IssueKind::DuplicateStructureElement,
    );
}

#[test]
fn duplicate_body_is_rejected() {
    assert_invalid_with(
        "<!DOCTYPE html><html><head></head><body></body><body></body></html>",
        IssueKind::DuplicateStructureElement,
    );
}

#[test]
fn head_after_body_is_rejected() {
    assert_invalid_with(
        "<!DOCTYPE html><html><body></body><head></head></html>",
        IssueKind::MisplacedStructureElement,
    );
}

#[test]
fn element_directly_under_html_is_rejected() {
    assert_invalid_with(
        "<!DOCTYPE html><html><div></div><head></head><body></body></html>",
        IssueKind::MisplacedStructureElement,
    );
}

#[test]
fn content_after_html_root_is_rejected() {
    assert_invalid_with(
        "<!DOCTYPE html><html><head></head><body></body></html>trailing",
        IssueKind::ContentOutsideRoot,
    );
}

#[test]
fn element_after_html_root_is_rejected() {
    assert_invalid_with(
        "<!DOCTYPE html><html><head></head><body></body></html><div></div>",
        IssueKind::ContentOutsideRoot,
    );
}

#[test]
fn text_directly_in_head_is_rejected() {
    assert_invalid_with(
        "<!DOCTYPE html><html><head>oops</head><body></body></html>",
        IssueKind::MisplacedContent,
    );
}

#[test]
fn unclosed_tag_is_rejected_with_position() {
    let html = "<!DOCTYPE html>\n<html>\n<head></head>\n<body>\n  <div>\n</body>\n</html>";
    let result = validate_single_file_html(html);
    assert!(!result.valid);
    let issue = result
        .issues
        .iter()
        .find(|issue| issue.kind == IssueKind::UnclosedTag && issue.message.contains("div"))
        .expect("expected unclosed div issue");
    assert_eq!(issue.position.line, 5);
    assert_eq!(issue.position.column, 3);
    assert!(
        issue.snippet.contains("<div>"),
        "snippet: {}",
        issue.snippet
    );
}

#[test]
fn truncated_document_is_rejected() {
    assert_invalid_with(
        "<!DOCTYPE html><html><head></head><body><div class=\"x\">",
        IssueKind::UnclosedTag,
    );
}

#[test]
fn unexpected_closing_tag_is_rejected() {
    assert_invalid_with(
        "<!DOCTYPE html><html><head></head><body></div></body></html>",
        IssueKind::UnexpectedClosingTag,
    );
}

#[test]
fn mismatched_closing_tag_is_rejected() {
    assert_invalid_with(
        "<!DOCTYPE html><html><head></head><body><div><span></div></body></html>",
        IssueKind::MismatchedClosingTag,
    );
}

#[test]
fn self_closing_non_void_element_is_rejected() {
    assert_invalid_with(
        "<!DOCTYPE html><html><head></head><body><div/></body></html>",
        IssueKind::SelfClosingNonVoid,
    );
}

#[test]
fn self_closing_svg_element_is_allowed() {
    let html = r#"<!DOCTYPE html><html><head></head><body>
      <svg viewBox="0 0 10 10"><circle cx="5" cy="5" r="4"/></svg>
    </body></html>"#;
    let result = validate_single_file_html(html);
    assert!(result.valid, "svg rejected: {:#?}", result.issues);
}

#[test]
fn unterminated_comment_is_rejected() {
    assert_invalid_with(
        "<!DOCTYPE html><html><head></head><body><!-- oops</body></html>",
        IssueKind::UnterminatedComment,
    );
}

#[test]
fn unterminated_tag_is_rejected() {
    assert_invalid_with(
        "<!DOCTYPE html><html><head></head><body><div class=\"x",
        IssueKind::UnterminatedTag,
    );
}

#[test]
fn unterminated_script_is_rejected() {
    assert_invalid_with(
        "<!DOCTYPE html><html><head></head><body><script>var a = 1;",
        IssueKind::UnterminatedRawText,
    );
}

#[test]
fn unterminated_style_is_rejected() {
    assert_invalid_with(
        "<!DOCTYPE html><html><head><style>body{color:red}",
        IssueKind::UnterminatedRawText,
    );
}

#[test]
fn duplicate_attribute_is_rejected() {
    assert_invalid_with(
        "<!DOCTYPE html><html><head></head><body><div class=\"a\" class=\"b\"></div></body></html>",
        IssueKind::DuplicateAttribute,
    );
}

#[test]
fn invalid_tag_syntax_is_rejected() {
    assert_invalid_with(
        "<!DOCTYPE html><html><head></head><body></></body></html>",
        IssueKind::InvalidTagSyntax,
    );
}

#[test]
fn invalid_tag_name_is_rejected() {
    assert_invalid_with(
        "<!DOCTYPE html><html><head></head><body><div@class=\"x\"></div></body></html>",
        IssueKind::InvalidTagName,
    );
}

#[test]
fn invalid_attribute_name_is_rejected() {
    assert_invalid_with(
        "<!DOCTYPE html><html><head></head><body><div \"foo\"=1></div></body></html>",
        IssueKind::InvalidAttributeName,
    );
}

#[test]
fn max_depth_is_enforced() {
    let html = "<!DOCTYPE html><html><head></head><body><div><div><div><div></div></div></div></div></body></html>";
    let config = ValidatorConfig {
        max_depth: 3,
        ..ValidatorConfig::default()
    };
    let result = validate_single_file_html_with_config(html, &config);
    assert!(result
        .issues
        .iter()
        .any(|issue| issue.kind == IssueKind::MaxDepthExceeded));
}

#[test]
fn oversized_document_is_rejected() {
    let html = format!(
        "<!DOCTYPE html><html><head></head><body>{}</body></html>",
        "x".repeat(100)
    );
    let config = ValidatorConfig {
        max_bytes: 64,
        ..ValidatorConfig::default()
    };
    let result = validate_single_file_html_with_config(&html, &config);
    assert!(!result.valid);
    assert!(result
        .issues
        .iter()
        .any(|issue| issue.kind == IssueKind::TooLarge));
}

#[test]
fn issue_cap_emits_warning() {
    let html = "<!DOCTYPE html><html><head></head><body></div></div></div></body></html>";
    let config = ValidatorConfig {
        max_issues: 2,
        ..ValidatorConfig::default()
    };
    let result = validate_single_file_html_with_config(html, &config);
    assert!(result
        .issues
        .iter()
        .any(|issue| issue.kind == IssueKind::TooManyIssues));
    assert!(result
        .issues
        .iter()
        .any(|issue| issue.severity == Severity::Warning));
}

#[test]
fn external_script_is_rejected() {
    assert_invalid_with(
        "<!DOCTYPE html><html><head></head><body><script src=\"https://cdn.example.com/app.js\"></script></body></html>",
        IssueKind::ExternalScript,
    );
}

#[test]
fn external_stylesheet_is_rejected() {
    assert_invalid_with(
        "<!DOCTYPE html><html><head><link rel=\"stylesheet\" href=\"https://cdn.example.com/app.css\"></head><body></body></html>",
        IssueKind::ExternalStylesheet,
    );
}

#[test]
fn external_image_is_rejected() {
    assert_invalid_with(
        "<!DOCTYPE html><html><head></head><body><img src=\"https://cdn.example.com/a.png\"></body></html>",
        IssueKind::ExternalResource,
    );
    assert_invalid_with(
        "<!DOCTYPE html><html><head></head><body><img src=\"//cdn.example.com/a.png\"></body></html>",
        IssueKind::ExternalResource,
    );
    assert_invalid_with(
        "<!DOCTYPE html><html><head></head><body><img src=\"images/a.png\"></body></html>",
        IssueKind::ExternalResource,
    );
}

#[test]
fn external_css_url_is_rejected() {
    assert_invalid_with(
        "<!DOCTYPE html><html><head><style>body{background:url(https://cdn.example.com/bg.png)}</style></head><body></body></html>",
        IssueKind::ExternalCssResource,
    );
}

#[test]
fn css_import_is_rejected() {
    assert_invalid_with(
        "<!DOCTYPE html><html><head><style>@import \"https://cdn.example.com/a.css\";</style></head><body></body></html>",
        IssueKind::ExternalCssResource,
    );
}

#[test]
fn inline_style_attribute_with_external_url_is_rejected() {
    assert_invalid_with(
        "<!DOCTYPE html><html><head></head><body><div style=\"background:url(//cdn.example.com/a.png)\"></div></body></html>",
        IssueKind::ExternalCssResource,
    );
}

#[test]
fn inline_style_issue_position_points_at_url() {
    let html = "<!DOCTYPE html><html><head></head><body><div style=\"background:url(//cdn.example.com/a.png)\"></div></body></html>";
    let result = validate_single_file_html(html);
    let issue = result
        .issues
        .iter()
        .find(|issue| issue.kind == IssueKind::ExternalCssResource)
        .expect("expected external css resource issue");
    assert_eq!(
        issue.position.offset,
        html.find("url(").expect("url offset")
    );
}

#[test]
fn data_uri_is_allowed() {
    let html = "<!DOCTYPE html><html><head></head><body><img src=\"data:image/png;base64,iVBORw0KGgo=\"></body></html>";
    let result = validate_single_file_html(html);
    assert!(result.valid, "data uri rejected: {:#?}", result.issues);
}

#[test]
fn whitelisted_external_host_is_allowed_for_resources() {
    let html = "<!DOCTYPE html><html><head></head><body><img src=\"https://cdn.example.com/a.png\"></body></html>";
    let config = ValidatorConfig {
        allowed_external_hosts: vec!["cdn.example.com".to_string()],
        ..ValidatorConfig::default()
    };
    let result = validate_single_file_html_with_config(html, &config);
    assert!(
        result.valid,
        "whitelisted host rejected: {:#?}",
        result.issues
    );
}

#[test]
fn event_handler_outside_whitelist_is_rejected() {
    assert_invalid_with(
        "<!DOCTYPE html><html><head></head><body><div onerror=\"alert(1)\"></div></body></html>",
        IssueKind::EventHandlerNotAllowed,
    );
}

#[test]
fn event_handler_inside_whitelist_is_allowed() {
    let html = "<!DOCTYPE html><html><head></head><body><button onclick=\"go()\">go</button></body></html>";
    let result = validate_single_file_html(html);
    assert!(result.valid, "onclick rejected: {:#?}", result.issues);
}

#[test]
fn javascript_url_is_rejected() {
    assert_invalid_with(
        "<!DOCTYPE html><html><head></head><body><a href=\"javascript:alert(1)\">x</a></body></html>",
        IssueKind::DisallowedUrlScheme,
    );
}

#[test]
fn obfuscated_javascript_url_is_rejected() {
    assert_invalid_with(
        "<!DOCTYPE html><html><head></head><body><a href=\"java&#115;cript:alert(1)\">x</a></body></html>",
        IssueKind::DisallowedUrlScheme,
    );
    assert_invalid_with(
        "<!DOCTYPE html><html><head></head><body><a href=\"java\nscript:alert(1)\">x</a></body></html>",
        IssueKind::DisallowedUrlScheme,
    );
}

#[test]
fn forbidden_element_is_rejected() {
    assert_invalid_with(
        "<!DOCTYPE html><html><head></head><body><iframe src=\"https://evil.example\"></iframe></body></html>",
        IssueKind::ForbiddenElement,
    );
    assert_invalid_with(
        "<!DOCTYPE html><html><head></head><body><base href=\"https://evil.example/\"></body></html>",
        IssueKind::ForbiddenElement,
    );
}

#[test]
fn meta_refresh_is_rejected() {
    assert_invalid_with(
        "<!DOCTYPE html><html><head><meta http-equiv=\"refresh\" content=\"0;url=https://evil.example\"></head><body></body></html>",
        IssueKind::MetaRefresh,
    );
}

#[test]
fn css_expression_is_rejected() {
    assert_invalid_with(
        "<!DOCTYPE html><html><head><style>div{width:expression(alert(1))}</style></head><body></body></html>",
        IssueKind::UnsafeCss,
    );
}

#[test]
fn malformed_css_is_rejected() {
    assert_invalid_with(
        "<!DOCTYPE html><html><head><style>div{background:url(</style></head><body></body></html>",
        IssueKind::InvalidCss,
    );
}

#[test]
fn issues_serialize_with_kind_position_and_snippet() {
    let html = "<!DOCTYPE html><html><head></head><body><script src=\"https://cdn.example.com/a.js\"></script></body></html>";
    let result = validate_single_file_html(html);
    let value = serde_json::to_value(&result).expect("result must serialize");
    assert_eq!(value["valid"], false);
    let issue = value["issues"]
        .as_array()
        .and_then(|issues| issues.first())
        .expect("expected at least one issue");
    assert_eq!(issue["kind"], "external_script");
    assert!(issue["position"]["line"].is_number());
    assert!(issue["position"]["column"].is_number());
    assert!(issue["snippet"]
        .as_str()
        .unwrap()
        .contains("cdn.example.com"));
}

#[test]
fn all_error_kinds_are_covered_by_tests() {
    // 该断言把「新增错误类型必须补测试」固化下来：新增枚举变体时此处会失败。
    let expected = [
        IssueKind::EmptyDocument,
        IssueKind::TooLarge,
        IssueKind::MissingDoctype,
        IssueKind::InvalidDoctype,
        IssueKind::DoctypeNotFirst,
        IssueKind::MissingStructureElement,
        IssueKind::DuplicateStructureElement,
        IssueKind::MisplacedStructureElement,
        IssueKind::ContentOutsideRoot,
        IssueKind::MisplacedContent,
        IssueKind::UnclosedTag,
        IssueKind::UnexpectedClosingTag,
        IssueKind::MismatchedClosingTag,
        IssueKind::SelfClosingNonVoid,
        IssueKind::UnterminatedTag,
        IssueKind::UnterminatedComment,
        IssueKind::UnterminatedRawText,
        IssueKind::InvalidTagSyntax,
        IssueKind::InvalidTagName,
        IssueKind::DuplicateAttribute,
        IssueKind::InvalidAttributeName,
        IssueKind::MaxDepthExceeded,
        IssueKind::ExternalScript,
        IssueKind::ExternalStylesheet,
        IssueKind::ExternalResource,
        IssueKind::ExternalCssResource,
        IssueKind::DisallowedUrlScheme,
        IssueKind::EventHandlerNotAllowed,
        IssueKind::ForbiddenElement,
        IssueKind::MetaRefresh,
        IssueKind::UnsafeCss,
        IssueKind::InvalidCss,
        IssueKind::TooManyIssues,
    ];
    assert_eq!(IssueKind::ALL.len(), expected.len());
    for kind in expected {
        assert!(IssueKind::ALL.contains(&kind), "missing {kind:?}");
    }
}

#[test]
fn stats_count_elements_and_depth() {
    let result = validate_single_file_html(VALID_PAGE);
    assert_eq!(result.stats.bytes, VALID_PAGE.len());
    assert!(result.stats.max_depth >= 3, "stats: {:#?}", result.stats);
}

#[test]
fn result_helper_methods_work() {
    let result = validate_single_file_html("<html><head></head><body></body></html>");
    assert!(!result.valid);
    assert!(result.has_errors());
    assert!(result.first_error().is_some());
    assert_eq!(result.errors().count(), result.issues.len());
    assert_eq!(kinds(&result).len(), result.issues.len());
}
