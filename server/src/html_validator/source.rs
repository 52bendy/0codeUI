//! 源码位置映射与片段提取。

use crate::html_validator::model::Position;

/// 输入源码的行索引，用于把字节偏移换算为行列号。
pub(crate) struct SourceMap<'a> {
    input: &'a str,
    line_starts: Vec<usize>,
}

impl<'a> SourceMap<'a> {
    /// 建立行索引。
    pub fn new(input: &'a str) -> Self {
        let mut line_starts = vec![0];
        for (index, byte) in input.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(index + 1);
            }
        }
        Self { input, line_starts }
    }

    /// 字节偏移转行列号（1-based）。
    pub fn position(&self, offset: usize) -> Position {
        let offset = self.clamp_boundary(offset);
        let line_index = match self.line_starts.binary_search(&offset) {
            Ok(index) => index,
            Err(index) => index.saturating_sub(1),
        };
        let line_start = self.line_starts[line_index];
        let column = self.input[line_start..offset].chars().count() + 1;
        Position {
            offset,
            line: line_index + 1,
            column,
        }
    }

    /// 提取问题附近的单行片段（最多 160 个字符）。
    pub fn snippet(&self, offset: usize) -> String {
        let offset = self.clamp_boundary(offset);
        let mut line_start = self.input[..offset].rfind('\n').map_or(0, |i| i + 1);
        let mut line_end = self.input[offset..]
            .find('\n')
            .map_or(self.input.len(), |i| offset + i);

        // 偏移落在空行（常见于 EOF）时，向前找最近的非空行。
        if self.input[line_start..line_end].trim().is_empty() {
            let before = &self.input[..line_start];
            if let Some(previous_end) = before.rfind('\n') {
                let previous_start = before[..previous_end].rfind('\n').map_or(0, |i| i + 1);
                line_start = previous_start;
                line_end = previous_end;
            }
        }

        self.input[line_start..line_end]
            .trim()
            .chars()
            .take(160)
            .collect()
    }

    fn clamp_boundary(&self, offset: usize) -> usize {
        let mut offset = offset.min(self.input.len());
        while offset > 0 && !self.input.is_char_boundary(offset) {
            offset -= 1;
        }
        offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_counts_lines_and_columns() {
        let map = SourceMap::new("abc\ndef\n");
        assert_eq!(map.position(0).line, 1);
        assert_eq!(map.position(0).column, 1);
        assert_eq!(map.position(4).line, 2);
        assert_eq!(map.position(4).column, 1);
        assert_eq!(map.position(6).line, 2);
        assert_eq!(map.position(6).column, 3);
    }

    #[test]
    fn snippet_returns_trimmed_line() {
        let map = SourceMap::new("first\n  <div class=\"x\">  \nlast");
        assert_eq!(map.snippet(8), "<div class=\"x\">");
    }

    #[test]
    fn position_is_char_boundary_safe() {
        let map = SourceMap::new("中文\nabc");
        let position = map.position(1);
        assert_eq!(position.line, 1);
        assert_eq!(position.column, 1);
    }
}
