//! 校验问题收集器：统一附加位置、片段与数量上限。

use crate::html_validator::model::{IssueKind, Severity, ValidationIssue};
use crate::html_validator::source::SourceMap;

/// 问题收集器。
pub(crate) struct IssueSink<'a> {
    map: &'a SourceMap<'a>,
    issues: Vec<ValidationIssue>,
    max_issues: usize,
    truncated: bool,
}

impl<'a> IssueSink<'a> {
    /// 创建收集器。
    pub fn new(map: &'a SourceMap<'a>, max_issues: usize) -> Self {
        Self {
            map,
            issues: Vec::new(),
            max_issues,
            truncated: false,
        }
    }

    /// 记录一条错误。
    pub fn error(&mut self, kind: IssueKind, offset: usize, message: impl Into<String>) {
        self.push(kind, Severity::Error, offset, message.into());
    }

    /// 结束收集，返回问题列表。
    pub fn finish(self) -> Vec<ValidationIssue> {
        self.issues
    }

    fn push(&mut self, kind: IssueKind, severity: Severity, offset: usize, message: String) {
        if self.max_issues > 0 && self.issues.len() >= self.max_issues {
            if !self.truncated {
                self.truncated = true;
                self.issues.push(ValidationIssue {
                    kind: IssueKind::TooManyIssues,
                    severity: Severity::Warning,
                    message: format!("问题数量超过上限 {}，后续问题已截断", self.max_issues),
                    position: self.map.position(offset),
                    snippet: self.map.snippet(offset),
                });
            }
            return;
        }

        self.issues.push(ValidationIssue {
            kind,
            severity,
            message,
            position: self.map.position(offset),
            snippet: self.map.snippet(offset),
        });
    }
}
