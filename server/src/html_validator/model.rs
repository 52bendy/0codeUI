//! 校验结果的数据模型。

use serde::Serialize;

/// 问题严重级别。`Error` 会使校验不通过，`Warning` 仅提示。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// 错误：输出不可下发。
    Error,
    /// 警告：可下发，但建议关注。
    Warning,
}

/// 问题类型。新增变体时必须同步 [`IssueKind::ALL`] 并补充测试。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueKind {
    /// 文档为空（或仅含空白）。
    EmptyDocument,
    /// 文档超过大小上限。
    TooLarge,
    /// 缺少 DOCTYPE。
    MissingDoctype,
    /// DOCTYPE 不是 `html`。
    InvalidDoctype,
    /// DOCTYPE 未出现在文档最前。
    DoctypeNotFirst,
    /// 缺少 `html` / `head` / `body` 结构元素。
    MissingStructureElement,
    /// 结构元素重复（如两个 `body`）。
    DuplicateStructureElement,
    /// 结构元素位置错误（如 `head` 出现在 `body` 之后）。
    MisplacedStructureElement,
    /// 根元素 `html` 之外出现内容。
    ContentOutsideRoot,
    /// 不允许出现文本内容的位置（如 `head` 直接文本）。
    MisplacedContent,
    /// 标签未闭合。
    UnclosedTag,
    /// 多余的结束标签。
    UnexpectedClosingTag,
    /// 结束标签与当前打开的标签不匹配。
    MismatchedClosingTag,
    /// 非空元素使用了自闭合语法（如 `<div/>`）。
    SelfClosingNonVoid,
    /// 标签在文档结束前未闭合（语法中断）。
    UnterminatedTag,
    /// 注释未闭合。
    UnterminatedComment,
    /// `script` / `style` 等原始文本元素未闭合。
    UnterminatedRawText,
    /// 标签语法非法（如 `</>`、`<!foo>`）。
    InvalidTagSyntax,
    /// 标签名非法。
    InvalidTagName,
    /// 同一标签内属性重复。
    DuplicateAttribute,
    /// 属性名非法。
    InvalidAttributeName,
    /// 嵌套层级超过上限。
    MaxDepthExceeded,
    /// 外部脚本（`<script src>`）——单文件要求 JS 内联。
    ExternalScript,
    /// 外部样式表（`<link rel="stylesheet">`）。
    ExternalStylesheet,
    /// 外部资源（图片 / 字体 / 媒体 / iframe 等）。
    ExternalResource,
    /// CSS 中的外部资源（`url()` / `@import`）。
    ExternalCssResource,
    /// 危险 URL 协议（`javascript:` / `vbscript:` 等）。
    DisallowedUrlScheme,
    /// 事件属性不在白名单内。
    EventHandlerNotAllowed,
    /// 禁用元素（`iframe` / `object` / `base` 等）。
    ForbiddenElement,
    /// `meta http-equiv="refresh"` 跳转。
    MetaRefresh,
    /// 危险 CSS 写法（`expression()` / `-moz-binding` 等）。
    UnsafeCss,
    /// CSS 语法不完整（如 `url(` 未闭合）。
    InvalidCss,
    /// 问题数量超过上限，后续问题被截断。
    TooManyIssues,
}

impl IssueKind {
    /// 全部问题类型（用于测试与文档完整性检查）。
    pub const ALL: &'static [IssueKind] = &[
        Self::EmptyDocument,
        Self::TooLarge,
        Self::MissingDoctype,
        Self::InvalidDoctype,
        Self::DoctypeNotFirst,
        Self::MissingStructureElement,
        Self::DuplicateStructureElement,
        Self::MisplacedStructureElement,
        Self::ContentOutsideRoot,
        Self::MisplacedContent,
        Self::UnclosedTag,
        Self::UnexpectedClosingTag,
        Self::MismatchedClosingTag,
        Self::SelfClosingNonVoid,
        Self::UnterminatedTag,
        Self::UnterminatedComment,
        Self::UnterminatedRawText,
        Self::InvalidTagSyntax,
        Self::InvalidTagName,
        Self::DuplicateAttribute,
        Self::InvalidAttributeName,
        Self::MaxDepthExceeded,
        Self::ExternalScript,
        Self::ExternalStylesheet,
        Self::ExternalResource,
        Self::ExternalCssResource,
        Self::DisallowedUrlScheme,
        Self::EventHandlerNotAllowed,
        Self::ForbiddenElement,
        Self::MetaRefresh,
        Self::UnsafeCss,
        Self::InvalidCss,
        Self::TooManyIssues,
    ];

    /// 默认严重级别。
    pub const fn default_severity(self) -> Severity {
        match self {
            Self::TooManyIssues => Severity::Warning,
            _ => Severity::Error,
        }
    }
}

/// 源码位置（1-based 行列，`offset` 为字节偏移）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Position {
    /// 字节偏移（从 0 开始）。
    pub offset: usize,
    /// 行号（从 1 开始）。
    pub line: usize,
    /// 列号（从 1 开始，按字符计数）。
    pub column: usize,
}

impl Position {
    /// 文档起点。
    pub const START: Self = Self {
        offset: 0,
        line: 1,
        column: 1,
    };
}

/// 一条结构化校验问题。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValidationIssue {
    /// 问题类型。
    pub kind: IssueKind,
    /// 严重级别。
    pub severity: Severity,
    /// 人类可读描述。
    pub message: String,
    /// 问题位置。
    pub position: Position,
    /// 问题附近的源码片段（单行、已截断）。
    pub snippet: String,
}

/// 文档统计信息，供日志与性能分析使用。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ValidationStats {
    /// 输入字节数。
    pub bytes: usize,
    /// 元素数量（含自闭合与空元素）。
    pub elements: usize,
    /// `script` 元素数量。
    pub scripts: usize,
    /// `style` 元素数量。
    pub styles: usize,
    /// 最大嵌套深度。
    pub max_depth: usize,
}

/// 校验结果。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ValidationResult {
    /// 是否通过（不存在 `Error` 级问题）。
    pub valid: bool,
    /// 结构化问题列表。
    pub issues: Vec<ValidationIssue>,
    /// 文档统计。
    pub stats: ValidationStats,
}

impl ValidationResult {
    /// 是否存在错误级问题。
    pub fn has_errors(&self) -> bool {
        self.issues
            .iter()
            .any(|issue| issue.severity == Severity::Error)
    }

    /// 遍历全部错误级问题。
    pub fn errors(&self) -> impl Iterator<Item = &ValidationIssue> {
        self.issues
            .iter()
            .filter(|issue| issue.severity == Severity::Error)
    }

    /// 第一条错误级问题。
    pub fn first_error(&self) -> Option<&ValidationIssue> {
        self.errors().next()
    }
}
