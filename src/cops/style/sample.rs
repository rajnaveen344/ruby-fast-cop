//! Style/Sample - Prefer `sample` over `shuffle.first`, `shuffle.last`, `shuffle[]`.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/sample.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::CallNode;

pub struct Sample;

impl Sample {
    pub fn new() -> Self {
        Self
    }

    fn src<'a>(source: &'a str, node: &ruby_prism::Node) -> &'a str {
        let loc = node.location();
        &source[loc.start_offset()..loc.end_offset()]
    }

    /// Parse an integer literal from its source text.
    fn parse_int(source: &str, node: &ruby_prism::Node) -> Option<i64> {
        if !matches!(node, ruby_prism::Node::IntegerNode { .. }) {
            return None;
        }
        Self::src(source, node)
            .chars()
            .filter(|c| *c != '_')
            .collect::<String>()
            .parse::<i64>()
            .ok()
    }

    /// Check if the outer method + args constitute an offense.
    fn is_offensive(method: &str, args: &[ruby_prism::Node], source: &str) -> bool {
        match method {
            "first" | "last" => true,
            "[]" | "at" | "slice" => Self::sample_size(args, source) != SampleSize::Unknown,
            _ => false,
        }
    }

    fn sample_size(args: &[ruby_prism::Node], source: &str) -> SampleSize {
        match args.len() {
            1 => Self::sample_size_for_one_arg(&args[0], source),
            2 => Self::sample_size_for_two_args(&args[0], &args[1], source),
            _ => SampleSize::Unknown,
        }
    }

    fn sample_size_for_one_arg(arg: &ruby_prism::Node, source: &str) -> SampleSize {
        match arg {
            ruby_prism::Node::RangeNode { .. } => {
                Self::range_size(&arg.as_range_node().unwrap(), source)
            }
            ruby_prism::Node::IntegerNode { .. } => {
                if let Some(val) = Self::parse_int(source, arg) {
                    if val == 0 || val == -1 {
                        SampleSize::None
                    } else {
                        SampleSize::Unknown
                    }
                } else {
                    SampleSize::Unknown
                }
            }
            _ => SampleSize::Unknown,
        }
    }

    fn sample_size_for_two_args(
        first: &ruby_prism::Node,
        second: &ruby_prism::Node,
        source: &str,
    ) -> SampleSize {
        // first arg must be integer 0
        match Self::parse_int(source, first) {
            Some(0) => {}
            _ => return SampleSize::Unknown,
        }
        // second arg: if integer, use its value; otherwise unknown
        if let Some(val) = Self::parse_int(source, second) {
            SampleSize::Size(val)
        } else {
            SampleSize::Unknown
        }
    }

    fn range_size(range: &ruby_prism::RangeNode, source: &str) -> SampleSize {
        let low: i64 = match range.left() {
            None => 0,
            Some(ref node) => match Self::parse_int(source, node) {
                Some(v) => v,
                None => return SampleSize::Unknown,
            },
        };

        let high: i64 = match range.right() {
            None => return SampleSize::Unknown, // open-ended like `0..`
            Some(ref node) => match Self::parse_int(source, node) {
                Some(v) => v,
                None => return SampleSize::Unknown,
            },
        };

        if low != 0 || high < 0 {
            return SampleSize::Unknown;
        }

        let size = if range.is_exclude_end() {
            high // 0...N has N elements
        } else {
            high + 1 // 0..N has N+1 elements
        };

        SampleSize::Size(size)
    }

    /// Build the correction string for `sample(...)`.
    fn correction(
        shuffle_arg_src: Option<&str>,
        method: &str,
        args: &[ruby_prism::Node],
        source: &str,
    ) -> String {
        let sample_arg = Self::sample_arg(method, args, source);
        let parts: Vec<&str> = [sample_arg.as_deref(), shuffle_arg_src]
            .iter()
            .filter_map(|x| *x)
            .collect();
        if parts.is_empty() {
            "sample".to_string()
        } else {
            format!("sample({})", parts.join(", "))
        }
    }

    fn sample_arg(method: &str, args: &[ruby_prism::Node], source: &str) -> Option<String> {
        match method {
            "first" | "last" => {
                if args.is_empty() {
                    None
                } else {
                    Some(Self::src(source, &args[0]).to_string())
                }
            }
            "[]" | "slice" => match Self::sample_size(args, source) {
                SampleSize::None => None,
                SampleSize::Size(n) => Some(n.to_string()),
                SampleSize::Unknown => None,
            },
            "at" => None, // at(0) or at(-1) => no sample arg
            _ => None,
        }
    }
}

#[derive(Debug, PartialEq)]
enum SampleSize {
    None,      // No argument needed (e.g., shuffle[0] => sample)
    Size(i64), // Specific size (e.g., shuffle[0..3] => sample(4))
    Unknown,   // Cannot determine / not offensive
}

impl Cop for Sample {
    fn name(&self) -> &'static str {
        "Style/Sample"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method_name = node_name!(node);

        // Must be one of: first, last, [], at, slice
        if !matches!(
            method_name.as_ref(),
            "first" | "last" | "[]" | "at" | "slice"
        ) {
            return vec![];
        }

        // Receiver must be a call to `shuffle`
        let receiver = match node.receiver() {
            Some(r) => r,
            None => return vec![],
        };

        let shuffle_node = match &receiver {
            ruby_prism::Node::CallNode { .. } => receiver.as_call_node().unwrap(),
            _ => return vec![],
        };

        let shuffle_name = node_name!(shuffle_node);
        if shuffle_name != "shuffle" {
            return vec![];
        }

        // Collect method args (the outer call's arguments)
        let method_args: Vec<ruby_prism::Node> = if let Some(args_node) = node.arguments() {
            args_node.arguments().iter().collect()
        } else {
            vec![]
        };

        if !Self::is_offensive(&method_name, &method_args, ctx.source) {
            return vec![];
        }

        // Get shuffle's argument source (e.g., "random: Random.new")
        let shuffle_arg_src: Option<String> =
            if let Some(shuffle_args) = shuffle_node.arguments() {
                let args: Vec<_> = shuffle_args.arguments().iter().collect();
                if args.is_empty() {
                    None
                } else {
                    let first_loc = args[0].location();
                    let last_loc = args[args.len() - 1].location();
                    Some(
                        ctx.source[first_loc.start_offset()..last_loc.end_offset()].to_string(),
                    )
                }
            } else {
                None
            };

        // Offense range: from shuffle's method name start to end of outer node
        let shuffle_msg_loc = match shuffle_node.message_loc() {
            Some(loc) => loc,
            None => return vec![],
        };
        let start_offset = shuffle_msg_loc.start_offset();
        let end_offset = node.location().end_offset();

        let incorrect = &ctx.source[start_offset..end_offset];

        let correct = Self::correction(
            shuffle_arg_src.as_deref(),
            &method_name,
            &method_args,
            ctx.source,
        );

        let message = format!("Use `{}` instead of `{}`.", correct, incorrect);

        let offense = ctx.offense_with_range(
            self.name(),
            &message,
            self.severity(),
            start_offset,
            end_offset,
        );

        let correction = Correction::replace(start_offset, end_offset, &correct);
        vec![offense.with_correction(correction)]
    }
}
