//! 校验器配置与默认安全策略。

use std::collections::BTreeSet;

/// 默认允许的内联事件属性白名单。
///
/// 只放行常见用户交互事件；`onerror` / `onload` / `onmessage` 等
/// 生命周期与错误事件默认禁止，避免被用作注入执行入口。
pub const DEFAULT_EVENT_ATTRIBUTES: &[&str] = &[
    "onclick",
    "ondblclick",
    "onchange",
    "oninput",
    "onsubmit",
    "onreset",
    "onfocus",
    "onblur",
    "onkeydown",
    "onkeyup",
    "onkeypress",
    "onmousedown",
    "onmouseup",
    "onmouseover",
    "onmouseout",
    "onmousemove",
    "ontouchstart",
    "ontouchend",
    "ontouchmove",
    "onpointerdown",
    "onpointerup",
    "onpointermove",
    "onpointercancel",
    "onscroll",
    "onwheel",
    "ondragstart",
    "ondragover",
    "ondrop",
    "ondragend",
];

/// 默认禁用元素：可加载外部内容或改变文档基址 / 执行插件的元素。
pub const DEFAULT_FORBIDDEN_ELEMENTS: &[&str] = &[
    "iframe", "frame", "frameset", "object", "embed", "applet", "base", "portal",
];

/// 校验器配置。
#[derive(Debug, Clone)]
pub struct ValidatorConfig {
    /// 最大输入字节数，默认 2 MiB。
    pub max_bytes: usize,
    /// 最大嵌套深度，默认 128。
    pub max_depth: usize,
    /// 最大问题条数（超出后截断并给出警告），默认 100。
    pub max_issues: usize,
    /// 是否要求 DOCTYPE，默认 `true`。
    pub require_doctype: bool,
    /// 是否允许 `data:` URI（内联资源），默认 `true`。
    pub allow_data_uris: bool,
    /// 外部资源主机白名单，支持 `*.example.com` 通配，默认空。
    pub allowed_external_hosts: Vec<String>,
    /// 允许的内联事件属性白名单。
    pub allowed_event_attributes: BTreeSet<String>,
    /// 禁用元素列表。
    pub forbidden_elements: BTreeSet<String>,
}

impl Default for ValidatorConfig {
    fn default() -> Self {
        Self {
            max_bytes: 2 * 1024 * 1024,
            max_depth: 128,
            max_issues: 100,
            require_doctype: true,
            allow_data_uris: true,
            allowed_external_hosts: Vec::new(),
            allowed_event_attributes: DEFAULT_EVENT_ATTRIBUTES
                .iter()
                .map(|name| (*name).to_string())
                .collect(),
            forbidden_elements: DEFAULT_FORBIDDEN_ELEMENTS
                .iter()
                .map(|name| (*name).to_string())
                .collect(),
        }
    }
}

impl ValidatorConfig {
    /// 判断事件属性是否在白名单内（大小写不敏感）。
    pub fn is_event_allowed(&self, attribute: &str) -> bool {
        self.allowed_event_attributes.contains(attribute)
    }

    /// 判断元素是否被禁用（大小写不敏感）。
    pub fn is_element_forbidden(&self, element: &str) -> bool {
        self.forbidden_elements.contains(element)
    }
}
