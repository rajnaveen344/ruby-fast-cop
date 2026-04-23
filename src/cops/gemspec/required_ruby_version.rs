//! Gemspec/RequiredRubyVersion cop
//! Checks that required_ruby_version is specified and matches TargetRubyVersion.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/gemspec/required_ruby_version.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Default)]
pub struct RequiredRubyVersion;

impl RequiredRubyVersion {
    pub fn new() -> Self { Self }
}

impl Cop for RequiredRubyVersion {
    fn name(&self) -> &'static str { "Gemspec/RequiredRubyVersion" }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        // Only run on .gemspec files (or when no specific file is given)
        let filename = ctx.filename;
        if filename != "(string)" && !filename.ends_with(".gemspec") {
            return vec![];
        }

        let source = ctx.source;
        let target_version = ctx.target_ruby_version;
        let target_str = format_version(target_version);

        let result = ruby_prism::parse(source.as_bytes());
        let tree = result.node();

        let mut visitor = RequiredRubyVersionVisitor {
            source,
            target_str: &target_str,
            offenses: Vec::new(),
            ctx,
            found_assignment: false,
        };
        visitor.visit(&tree);

        // If no assignment found, report offense on first char
        if !visitor.found_assignment {
            let msg = "`required_ruby_version` should be specified.";
            visitor.offenses.push(ctx.offense_with_range(
                "Gemspec/RequiredRubyVersion", msg, Severity::Convention,
                0, 1,
            ));
        }

        visitor.offenses
    }
}

struct RequiredRubyVersionVisitor<'a> {
    source: &'a str,
    target_str: &'a str,
    offenses: Vec<Offense>,
    ctx: &'a CheckContext<'a>,
    found_assignment: bool,
}

impl RequiredRubyVersionVisitor<'_> {
    fn not_equal_msg(&self) -> String {
        format!(
            "`required_ruby_version` and `TargetRubyVersion` ({}, which may be specified in .rubocop.yml) should be equal.",
            self.target_str
        )
    }

    fn emit_offense(&mut self, start: usize, end: usize) {
        let msg = self.not_equal_msg();
        self.offenses.push(self.ctx.offense_with_range(
            "Gemspec/RequiredRubyVersion", &msg, Severity::Convention,
            start, end,
        ));
    }

    /// Extract ruby version (major.minor) from a version requirement string like ">= 3.3.0"
    /// Mimics RuboCop's: required_ruby_version.str_content.scan(/\d/).first(2).join('.')
    fn extract_version(req: &str) -> Option<String> {
        let digits: Vec<char> = req.chars().filter(|c| c.is_ascii_digit()).collect();
        if digits.len() >= 2 {
            Some(format!("{}.{}", digits[0], digits[1]))
        } else if digits.len() == 1 {
            Some(format!("{}.0", digits[0]))
        } else {
            None
        }
    }

    /// Check a string version requirement against target
    /// Returns true if there is a version mismatch (offense needed)
    fn check_version_str(&self, version_str: &str) -> bool {
        let extracted = Self::extract_version(version_str);
        match extracted {
            Some(v) => v != self.target_str,
            None => true, // empty or non-parseable → offense
        }
    }

    fn process_arg(&mut self, arg: &Node, start: usize, end: usize) {
        match arg {
            Node::StringNode { .. } => {
                let s = arg.as_string_node().unwrap();
                let content = String::from_utf8_lossy(s.unescaped()).to_string();
                if self.check_version_str(&content) {
                    self.emit_offense(start, end);
                }
            }
            Node::ArrayNode { .. } => {
                let arr = arg.as_array_node().unwrap();
                let elements: Vec<Node> = arr.elements().iter().collect();

                if elements.is_empty() {
                    self.emit_offense(start, end);
                    return;
                }

                // Check if any string element has a >= or ~>
                // Find the first element with >= or ~> operator
                let lower_bound = elements.iter().find_map(|e| {
                    if let Node::StringNode { .. } = e {
                        let s = e.as_string_node().unwrap();
                        let content = String::from_utf8_lossy(s.unescaped()).to_string();
                        let t = content.trim();
                        if t.contains('=') || t.starts_with('>') {
                            return Some(content);
                        }
                    }
                    None
                });

                match lower_bound {
                    Some(lb) => {
                        if self.check_version_str(&lb) {
                            self.emit_offense(start, end);
                        }
                    }
                    None => {
                        // No lower bound string found → not pure strings → skip (false negative)
                    }
                }
            }
            Node::CallNode { .. } => {
                // Gem::Requirement.new(str+) pattern
                let call = arg.as_call_node().unwrap();
                let method = String::from_utf8_lossy(call.name().as_slice()).to_string();
                if method != "new" {
                    return; // dynamic → skip
                }

                if let Some(req_args) = call.arguments() {
                    let req_arg_list: Vec<Node> = req_args.arguments().iter().collect();

                    // Collect all string requirements
                    let mut reqs: Vec<String> = Vec::new();
                    let mut all_strings = true;
                    for ra in &req_arg_list {
                        if let Node::StringNode { .. } = ra {
                            let s = ra.as_string_node().unwrap();
                            reqs.push(String::from_utf8_lossy(s.unescaped()).to_string());
                        } else {
                            all_strings = false;
                            break;
                        }
                    }

                    if !all_strings { return; }

                    // Find lower bound (>= or ~> or =)
                    let lower_bound = reqs.iter().find(|r| {
                        let t = r.trim();
                        t.contains('=') || t.starts_with('>')
                    });

                    match lower_bound {
                        Some(lb) => {
                            // If there's also an upper bound and the lower bound matches, skip
                            let has_upper = reqs.iter().any(|r| r.trim().starts_with('<'));
                            if has_upper {
                                // Multi-requirement with both bounds: check lower bound matches
                                if self.check_version_str(lb) {
                                    self.emit_offense(start, end);
                                }
                            } else {
                                if self.check_version_str(lb) {
                                    self.emit_offense(start, end);
                                }
                            }
                        }
                        None => {
                            // No lower bound → offense
                            self.emit_offense(start, end);
                        }
                    }
                }
            }
            _ => {
                // Variable or other dynamic expression: skip (false negative)
            }
        }
    }
}

fn format_version(v: f64) -> String {
    // 3.4 → "3.4", 3.0 → "3.0"
    format!("{:.1}", v)
}

impl Visit<'_> for RequiredRubyVersionVisitor<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let method = String::from_utf8_lossy(node.name().as_slice()).to_string();

        if method == "required_ruby_version=" {
            self.found_assignment = true;

            // Get the first argument (the version definition)
            if let Some(args) = node.arguments() {
                let arg_list: Vec<Node> = args.arguments().iter().collect();
                if !arg_list.is_empty() {
                    let arg = &arg_list[0];
                    let loc = arg.location();
                    let start = loc.start_offset();
                    let end = loc.end_offset();

                    // Check if the version is dynamic (variable, etc.)
                    let is_dynamic = match arg {
                        Node::LocalVariableReadNode { .. }
                        | Node::InstanceVariableReadNode { .. }
                        | Node::ClassVariableReadNode { .. }
                        | Node::GlobalVariableReadNode { .. } => true,
                        _ => false,
                    };

                    if !is_dynamic {
                        self.process_arg(arg, start, end);
                    }
                }
            }
        }

        ruby_prism::visit_call_node(self, node);
    }
}

crate::register_cop!("Gemspec/RequiredRubyVersion", |_cfg| Some(Box::new(RequiredRubyVersion::new())));
