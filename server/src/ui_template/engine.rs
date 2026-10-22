//! 无依赖的轻量模板引擎：`{{path}}` 占位符、`{{#section}}` 迭代与 `{{^section}}` 反向区块。
//!
//! 设计约束：
//! - 只做「文本填充」，不执行任意表达式 / 不引入模板语言运行时；
//! - 所有插值默认做 HTML 转义，模板作者无需（也无法）关闭；
//! - 缺失变量、未闭合区块等错误显式返回，避免静默产出半成品页面。

use serde_json::Value;

use crate::ui_template::TemplateError;

/// 渲染一段模板字符串。
///
/// `data` 为业务 JSON；区块内可直接引用数组元素字段，也支持：
/// - `{{@index}}`：当前循环下标（从 0 开始）；
/// - `{{index}}`：当前循环序号（从 1 开始，元素自身含 `index` 字段时以元素字段优先）；
/// - `{{.}}`：当前上下文本身。
pub fn render_str(template: &str, data: &Value) -> Result<String, TemplateError> {
    let nodes = parse(template)?;
    let mut frames = vec![Frame {
        value: data.clone(),
        index: None,
    }];
    let mut output = String::with_capacity(template.len() + 64);
    render_nodes(&nodes, &mut frames, &mut output)?;
    Ok(output)
}

/// HTML 转义（覆盖 `& < > " '`；斜杠不转义，避免破坏 URL / 内联脚本中的路径）。
pub fn escape_html(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#x27;"),
            other => escaped.push(other),
        }
    }
    escaped
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Node {
    Text(String),
    Variable(String),
    Section {
        name: String,
        inverted: bool,
        children: Vec<Node>,
    },
}

#[derive(Debug)]
struct Frame {
    value: Value,
    index: Option<usize>,
}

fn parse(template: &str) -> Result<Vec<Node>, TemplateError> {
    let mut root: Vec<Node> = Vec::new();
    let mut stack: Vec<(String, bool, Vec<Node>)> = Vec::new();
    let mut rest = template;

    while let Some(start) = rest.find("{{") {
        push_text(&mut root, &mut stack, &rest[..start]);

        let after_open = &rest[start + 2..];
        let end = after_open
            .find("}}")
            .ok_or_else(|| TemplateError::Render("unclosed `{{` tag".to_string()))?;
        let tag = after_open[..end].trim();
        rest = &after_open[end + 2..];

        if let Some(name) = tag.strip_prefix('#') {
            stack.push((validate_name(name)?, false, Vec::new()));
        } else if let Some(name) = tag.strip_prefix('^') {
            stack.push((validate_name(name)?, true, Vec::new()));
        } else if let Some(name) = tag.strip_prefix('/') {
            let name = validate_name(name)?;
            let (open_name, inverted, children) = stack.pop().ok_or_else(|| {
                TemplateError::Render(format!("unexpected closing tag `/{name}`"))
            })?;
            if open_name != name {
                return Err(TemplateError::Render(format!(
                    "mismatched closing tag `/{name}`, expected `/{open_name}`"
                )));
            }
            push_node(
                &mut root,
                &mut stack,
                Node::Section {
                    name,
                    inverted,
                    children,
                },
            );
        } else if tag.starts_with('!') {
            // 注释：忽略。
        } else {
            push_node(&mut root, &mut stack, Node::Variable(validate_name(tag)?));
        }
    }

    push_text(&mut root, &mut stack, rest);

    if let Some((name, _, _)) = stack.last() {
        return Err(TemplateError::Render(format!(
            "unclosed section `{{{{#{name}}}}}`"
        )));
    }
    Ok(root)
}

fn validate_name(name: &str) -> Result<String, TemplateError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(TemplateError::Render("empty tag name".to_string()));
    }
    let valid = name == "."
        || name == "@index"
        || name
            .split('.')
            .all(|segment| !segment.is_empty() && is_identifier(segment));
    if !valid {
        return Err(TemplateError::Render(format!("invalid tag name `{name}`")));
    }
    Ok(name.to_string())
}

fn is_identifier(segment: &str) -> bool {
    segment
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

fn push_text(root: &mut Vec<Node>, stack: &mut [(String, bool, Vec<Node>)], text: &str) {
    if text.is_empty() {
        return;
    }
    push_node(root, stack, Node::Text(text.to_string()));
}

fn push_node(root: &mut Vec<Node>, stack: &mut [(String, bool, Vec<Node>)], node: Node) {
    match stack.last_mut() {
        Some((_, _, children)) => children.push(node),
        None => root.push(node),
    }
}

fn render_nodes(
    nodes: &[Node],
    frames: &mut Vec<Frame>,
    output: &mut String,
) -> Result<(), TemplateError> {
    for node in nodes {
        match node {
            Node::Text(text) => output.push_str(text),
            Node::Variable(name) => render_variable(name, frames, output)?,
            Node::Section {
                name,
                inverted,
                children,
            } => {
                let value = resolve(name, frames).cloned();
                let truthy = value.as_ref().map(is_truthy).unwrap_or(false);
                if *inverted {
                    if !truthy {
                        render_nodes(children, frames, output)?;
                    }
                    continue;
                }
                match value {
                    Some(Value::Array(items)) => {
                        for (index, item) in items.into_iter().enumerate() {
                            frames.push(Frame {
                                value: item,
                                index: Some(index),
                            });
                            render_nodes(children, frames, output)?;
                            frames.pop();
                        }
                    }
                    Some(value) if is_truthy(&value) => {
                        frames.push(Frame { value, index: None });
                        render_nodes(children, frames, output)?;
                        frames.pop();
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

fn render_variable(name: &str, frames: &[Frame], output: &mut String) -> Result<(), TemplateError> {
    if name == "@index" {
        let index = frames
            .last()
            .and_then(|frame| frame.index)
            .ok_or_else(|| TemplateError::Render("`@index` used outside a loop".to_string()))?;
        output.push_str(&index.to_string());
        return Ok(());
    }
    if name == "index" {
        let has_index_field = frames
            .last()
            .and_then(|frame| lookup(&frame.value, "index"))
            .is_some();
        if !has_index_field {
            if let Some(index) = frames.last().and_then(|frame| frame.index) {
                output.push_str(&(index + 1).to_string());
                return Ok(());
            }
        }
    }

    let value = resolve(name, frames)
        .ok_or_else(|| TemplateError::Render(format!("unknown placeholder `{name}`")))?;
    match value {
        Value::String(text) => output.push_str(&escape_html(text)),
        Value::Number(number) => output.push_str(&number.to_string()),
        Value::Bool(flag) => output.push_str(if *flag { "true" } else { "false" }),
        Value::Null => {
            return Err(TemplateError::Render(format!(
                "placeholder `{name}` resolved to null"
            )))
        }
        Value::Array(_) | Value::Object(_) => {
            return Err(TemplateError::Render(format!(
                "placeholder `{name}` must be a scalar value"
            )))
        }
    }
    Ok(())
}

fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
        Value::Number(number) => number.as_f64().map(|n| n != 0.0).unwrap_or(true),
    }
}

fn resolve<'a>(path: &str, frames: &'a [Frame]) -> Option<&'a Value> {
    let current = frames.last()?;
    if path == "." {
        return Some(&current.value);
    }

    let direct = lookup(&current.value, path);
    if direct.is_some() {
        return direct;
    }
    for frame in frames.iter().rev().skip(1) {
        if let Some(value) = lookup(&frame.value, path) {
            return Some(value);
        }
    }
    None
}

fn lookup<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;
    for segment in path.split('.') {
        current = match current {
            Value::Object(map) => map.get(segment)?,
            _ => return None,
        };
    }
    Some(current)
}
