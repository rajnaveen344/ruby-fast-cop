//! Gemspec/OrderedDependencies cop
//! Dependencies in gemspec should be ordered alphabetically within each group
//! of the same type (add_dependency, add_runtime_dependency, add_development_dependency).
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/gemspec/ordered_dependencies.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Cfg {
    #[serde(default)]
    consider_punctuation: bool,
    #[serde(default)]
    treat_comments_as_group_separators: bool,
}

impl Default for Cfg {
    fn default() -> Self {
        Cfg {
            consider_punctuation: false,
            treat_comments_as_group_separators: true,
        }
    }
}

pub struct OrderedDependencies {
    consider_punctuation: bool,
    treat_comments_as_group_separators: bool,
}

impl OrderedDependencies {
    pub fn new(consider_punctuation: bool, treat_comments_as_group_separators: bool) -> Self {
        Self { consider_punctuation, treat_comments_as_group_separators }
    }

    fn gem_sort_key(&self, name: &str) -> String {
        if self.consider_punctuation {
            name.to_lowercase()
        } else {
            name.chars()
                .filter(|c| c.is_alphanumeric())
                .collect::<String>()
                .to_lowercase()
        }
    }

    fn is_sorted(&self, a: &str, b: &str) -> bool {
        self.gem_sort_key(a) <= self.gem_sort_key(b)
    }
}

const DEP_METHODS: &[&str] = &[
    "add_dependency",
    "add_runtime_dependency",
    "add_development_dependency",
];

impl Cop for OrderedDependencies {
    fn name(&self) -> &'static str { "Gemspec/OrderedDependencies" }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let source = ctx.source;

        let result = ruby_prism::parse(source.as_bytes());
        let tree = result.node();

        let mut collector = DepCollector {
            source,
            deps: Vec::new(),
        };
        collector.visit(&tree);

        if collector.deps.len() < 2 {
            return vec![];
        }

        // Group consecutive deps of same type (separated by blank lines or different type)
        let mut groups: Vec<Vec<DepEntry>> = Vec::new();
        let mut current_group: Vec<DepEntry> = Vec::new();

        for dep in collector.deps {
            if current_group.is_empty() {
                current_group.push(dep);
                continue;
            }

            let prev = &current_group[current_group.len() - 1];
            let prev_end_line = prev.end_line;
            let cur_start_line = dep.start_line;
            let cur_method = dep.method.clone();
            let prev_method = prev.method.clone();

            // Different dep method type → new group
            let diff_type = cur_method != prev_method;
            let has_blank = lines_between_have_blank(source, prev_end_line, cur_start_line);
            let has_comment = self.treat_comments_as_group_separators
                && lines_between_have_comment(source, prev_end_line, cur_start_line);
            let no_name = dep.name.is_none();

            if diff_type || has_blank || has_comment || no_name
                || current_group.last().map_or(false, |g| g.name.is_none()) {
                if !current_group.is_empty() {
                    groups.push(std::mem::take(&mut current_group));
                }
                if !no_name {
                    current_group.push(dep);
                }
            } else {
                current_group.push(dep);
            }
        }
        if !current_group.is_empty() {
            groups.push(current_group);
        }

        let mut offenses = Vec::new();

        for group in &groups {
            for i in 1..group.len() {
                let prev = &group[i-1];
                let curr = &group[i];
                let prev_name = match &prev.name { Some(n) => n.clone(), None => continue };
                let curr_name = match &curr.name { Some(n) => n.clone(), None => continue };

                if !self.is_sorted(&prev_name, &curr_name) {
                    let msg = format!(
                        "Dependencies should be sorted in an alphabetical order within their section of the gemspec. Dependency `{}` should appear before `{}`.",
                        curr_name, prev_name
                    );

                    let correction = self.build_swap_correction(source, prev, curr);

                    let offense = ctx.offense_with_range(
                        "Gemspec/OrderedDependencies", &msg, Severity::Convention,
                        curr.node_start,
                        curr.node_end,
                    ).with_correction(correction);
                    offenses.push(offense);
                }
            }
        }

        offenses
    }
}

impl OrderedDependencies {
    fn build_swap_correction(&self, source: &str, prev: &DepEntry, curr: &DepEntry) -> Correction {
        let prev_with_comments = get_entry_with_comments(source, prev);
        let curr_with_comments = get_entry_with_comments(source, curr);

        let region_start = line_start_offset(source, prev.comment_start_line.unwrap_or(prev.start_line));
        let region_end = line_end_offset(source, curr.end_line);

        let swapped = format!("{}{}", curr_with_comments, prev_with_comments);
        Correction::replace(region_start, region_end, swapped)
    }
}

#[derive(Debug, Clone)]
struct DepEntry {
    name: Option<String>,
    method: String,
    start_line: usize,
    end_line: usize,
    node_start: usize,
    node_end: usize,
    comment_start_line: Option<usize>,
}

struct DepCollector<'a> {
    source: &'a str,
    deps: Vec<DepEntry>,
}

impl Visit<'_> for DepCollector<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let method = String::from_utf8_lossy(node.name().as_slice()).to_string();
        if DEP_METHODS.contains(&method.as_str()) {
            let loc = node.location();
            let start_line = 1 + self.source[..loc.start_offset()].bytes().filter(|&b| b == b'\n').count();
            let end_line = 1 + self.source[..loc.end_offset().saturating_sub(1)].bytes().filter(|&b| b == b'\n').count();

            // Get gem name from first arg - must be a string literal (not method call)
            let name = node.arguments()
                .and_then(|args| args.arguments().iter().next())
                .and_then(|arg| {
                    if let Node::StringNode { .. } = arg {
                        let s = arg.as_string_node().unwrap();
                        Some(String::from_utf8_lossy(s.unescaped()).to_string())
                    } else {
                        None
                    }
                });

            let comment_start_line = find_preceding_comment_line(self.source, start_line);

            self.deps.push(DepEntry {
                name,
                method,
                start_line,
                end_line,
                node_start: loc.start_offset(),
                node_end: loc.end_offset(),
                comment_start_line,
            });
        }
        ruby_prism::visit_call_node(self, node);
    }
}

fn lines_between_have_blank(source: &str, prev_end: usize, curr_start: usize) -> bool {
    if curr_start <= prev_end + 1 {
        return false;
    }
    for (i, line) in source.lines().enumerate() {
        let line_num = i + 1;
        if line_num > prev_end && line_num < curr_start && line.trim().is_empty() {
            return true;
        }
    }
    false
}

fn lines_between_have_comment(source: &str, prev_end: usize, curr_start: usize) -> bool {
    for (i, line) in source.lines().enumerate() {
        let line_num = i + 1;
        if line_num > prev_end && line_num < curr_start && line.trim().starts_with('#') {
            return true;
        }
    }
    false
}

fn line_start_offset(source: &str, line_num: usize) -> usize {
    let mut offset = 0;
    for (i, line) in source.lines().enumerate() {
        if i + 1 == line_num { return offset; }
        offset += line.len() + 1;
    }
    offset
}

fn line_end_offset(source: &str, line_num: usize) -> usize {
    let start = line_start_offset(source, line_num);
    let line_text = source[start..].lines().next().unwrap_or("");
    start + line_text.len() + 1
}

fn find_preceding_comment_line(source: &str, gem_line: usize) -> Option<usize> {
    if gem_line <= 1 { return None; }
    let lines: Vec<&str> = source.lines().collect();
    let prev_line = lines.get(gem_line - 2)?;
    if prev_line.trim().starts_with('#') {
        let mut comment_start = gem_line - 1;
        while comment_start > 1 {
            if let Some(candidate) = lines.get(comment_start - 2) {
                if candidate.trim().starts_with('#') {
                    comment_start -= 1;
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        Some(comment_start)
    } else {
        None
    }
}

fn get_entry_with_comments<'a>(source: &'a str, entry: &DepEntry) -> String {
    let first_line = entry.comment_start_line.unwrap_or(entry.start_line);
    let start = line_start_offset(source, first_line);
    let end = line_end_offset(source, entry.end_line);
    source[start..end.min(source.len())].to_string()
}

crate::register_cop!("Gemspec/OrderedDependencies", |cfg| {
    let c: Cfg = cfg.typed("Gemspec/OrderedDependencies");
    Some(Box::new(OrderedDependencies::new(c.consider_punctuation, c.treat_comments_as_group_separators)))
});
