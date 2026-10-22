//! 单文件 HTML 校验 CLI。
//!
//! 用法：
//! ```bash
//! intimeui-validate page.html
//! cat page.html | intimeui-validate -
//! ```
//!
//! 输出结构化 JSON（`valid` / `issues` / `stats`），退出码 0 通过、1 不通过、2 读取失败。

use std::io::{self, Read};
use std::process::ExitCode;

use intimeui_server::html_validator::validate_single_file_html;

fn main() -> ExitCode {
    let html = match read_input() {
        Ok(html) => html,
        Err(message) => {
            eprintln!("intimeui-validate: {message}");
            return ExitCode::from(2);
        }
    };

    let result = validate_single_file_html(&html);
    match serde_json::to_string_pretty(&result) {
        Ok(json) => println!("{json}"),
        Err(error) => {
            eprintln!("intimeui-validate: 序列化结果失败：{error}");
            return ExitCode::from(2);
        }
    }

    if result.valid {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn read_input() -> Result<String, String> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match arguments.as_slice() {
        [] => read_stdin(),
        [flag, path] if flag == "--file" => read_file(path),
        [path] => read_file(path),
        _ => Err("用法：intimeui-validate [--file] <path|->".to_string()),
    }
}

fn read_file(path: &str) -> Result<String, String> {
    if path == "-" {
        return read_stdin();
    }
    std::fs::read_to_string(path).map_err(|error| format!("读取 {path} 失败：{error}"))
}

fn read_stdin() -> Result<String, String> {
    let mut buffer = String::new();
    io::stdin()
        .read_to_string(&mut buffer)
        .map_err(|error| format!("读取标准输入失败：{error}"))?;
    Ok(buffer)
}
