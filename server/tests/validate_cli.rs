//! `intimeui-validate` CLI 的端到端测试。

use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::Value;

fn run(input: &str) -> (i32, Value) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_intimeui-validate"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("启动 CLI 失败");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(input.as_bytes())
        .expect("写入 stdin 失败");
    let output = child.wait_with_output().expect("等待 CLI 失败");
    let json = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
    (output.status.code().unwrap_or(-1), json)
}

const VALID_PAGE: &str =
    "<!DOCTYPE html><html><head><title>t</title></head><body><p>hi</p></body></html>";

#[test]
fn valid_document_exits_zero() {
    let (code, json) = run(VALID_PAGE);
    assert_eq!(code, 0);
    assert_eq!(json["valid"], true);
    assert_eq!(json["issues"].as_array().map(Vec::len), Some(0));
}

#[test]
fn invalid_document_exits_one_and_reports_issues() {
    let (code, json) = run("<!DOCTYPE html><html><head></head><body><div>");
    assert_eq!(code, 1);
    assert_eq!(json["valid"], false);
    assert!(json["issues"]
        .as_array()
        .is_some_and(|issues| !issues.is_empty()));
}
