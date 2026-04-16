//! Integration tests for the `ast` binary (Prism AST explorer).
//!
//! Invokes the compiled binary via CARGO_BIN_EXE_ast and asserts on stdout/stderr.

use std::io::Write;
use std::process::{Command, Stdio};

fn ast_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ast"))
}

fn run(args: &[&str]) -> (String, String, i32) {
    let out = ast_bin().args(args).output().expect("run ast");
    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
        out.status.code().unwrap_or(-1),
    )
}

fn run_stdin(args: &[&str], input: &str) -> (String, String, i32) {
    let mut child = ast_bin()
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn ast");
    child.stdin.as_mut().unwrap().write_all(input.as_bytes()).unwrap();
    let out = child.wait_with_output().expect("wait ast");
    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
        out.status.code().unwrap_or(-1),
    )
}

#[test]
fn dumps_basic_tree_with_source_snippets() {
    let (stdout, _, code) = run(&["foo.bar(1)"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("(program"), "stdout:\n{stdout}");
    assert!(stdout.contains("(call"));
    assert!(stdout.contains("(integer"));
    assert!(stdout.contains("`foo.bar(1)`"));
    assert!(stdout.contains("`1`"));
}

#[test]
fn dumps_nested_call_hierarchy() {
    let (stdout, _, code) = run(&["foo.bar.baz"]);
    assert_eq!(code, 0);
    // Three nested calls: foo.bar.baz wraps foo.bar wraps foo
    assert_eq!(stdout.matches("(call").count(), 3, "stdout:\n{stdout}");
}

#[test]
fn loc_flag_includes_line_col_and_byte_offsets() {
    let (stdout, _, code) = run(&["--loc", "foo.bar"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("@ 1:0..7"), "stdout:\n{stdout}");
    assert!(stdout.contains("[0..7]"));
}

#[test]
fn loc_flag_spans_multiline_as_line_col_range() {
    // `def x\n  y\nend` — def spans lines 1..3
    let (stdout, _, code) = run(&["--loc", "--no-source", "def x\n  y\nend"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("(def @ 1:0..3:3"), "stdout:\n{stdout}");
}

#[test]
fn no_source_flag_strips_snippets() {
    let (stdout, _, code) = run(&["--no-source", "foo.bar"]);
    assert_eq!(code, 0);
    assert!(!stdout.contains('`'), "snippets should be stripped; stdout:\n{stdout}");
    assert!(stdout.contains("(program"));
    assert!(stdout.contains("(call"));
}

#[test]
fn stdin_flag_reads_from_stdin() {
    let (stdout, _, code) = run_stdin(&["--stdin", "--no-source"], "a + b");
    assert_eq!(code, 0);
    // `a + b` parses to a call on `a` with `:+` and arg `b`
    assert!(stdout.contains("(program"), "stdout:\n{stdout}");
    assert!(stdout.contains("(call"));
    assert!(stdout.matches("(local_variable_read").count() + stdout.matches("(call").count() >= 2);
}

#[test]
fn parse_errors_reported_on_stderr() {
    let (_, stderr, _) = run(&["def foo"]);
    assert!(stderr.contains("parse error"), "stderr:\n{stderr}");
}

#[test]
fn help_flag_prints_usage_and_exits_zero() {
    let (_, stderr, code) = run(&["--help"]);
    assert_eq!(code, 0);
    assert!(stderr.contains("ast"), "stderr:\n{stderr}");
    assert!(stderr.contains("Usage"));
}

#[test]
fn unknown_flag_fails_with_exit_2() {
    let (_, stderr, code) = run(&["--wat"]);
    assert_eq!(code, 2);
    assert!(stderr.contains("unknown flag"), "stderr:\n{stderr}");
}

#[test]
fn no_input_prints_help_and_exits_2() {
    let (_, stderr, code) = run(&[]);
    assert_eq!(code, 2);
    assert!(stderr.contains("Usage"), "stderr:\n{stderr}");
}

#[test]
fn snake_case_kind_names_drop_node_suffix() {
    let (stdout, _, code) = run(&["--no-source", "@foo = 1"]);
    assert_eq!(code, 0);
    // instance_variable_write_node → instance_variable_write
    assert!(
        stdout.contains("(instance_variable_write"),
        "stdout:\n{stdout}"
    );
    assert!(!stdout.contains("_node"), "should strip '_node' suffix");
}

#[test]
fn leaf_nodes_render_on_single_line() {
    let (stdout, _, _) = run(&["42"]);
    // Integer leaf → single-line "(integer `42`)"
    assert!(
        stdout.lines().any(|l| l.contains("(integer") && l.trim_end().ends_with(')')),
        "integer leaf should render on a single line; stdout:\n{stdout}"
    );
}
