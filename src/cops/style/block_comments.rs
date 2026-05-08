use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Location, Offense, Severity};

const MSG: &str = "Do not use block comments.";

#[derive(Default)]
pub struct BlockComments;

impl BlockComments {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for BlockComments {
    fn name(&self) -> &'static str {
        "Style/BlockComments"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut offenses = Vec::new();
        let source = ctx.source;
        let bytes = source.as_bytes();

        // Scan for =begin at start of line
        let mut i = 0;
        while i < bytes.len() {
            // Check if we're at start of line (or start of file)
            let at_line_start = i == 0 || bytes[i - 1] == b'\n';
            if at_line_start
                && bytes.len() >= i + 6
                && &bytes[i..i + 6] == b"=begin"
                && (bytes.len() == i + 6 || bytes[i + 6] == b'\n' || bytes[i + 6] == b' ')
            {
                let block_start = i;
                // Found =begin — offense is just `=begin` (6 chars)
                let offense_end = i + 6;
                let loc = Location::from_offsets(source, i, offense_end);
                // Skip to =end to find full block range
                i += 6;
                // Skip rest of =begin line
                while i < bytes.len() && bytes[i] != b'\n' { i += 1; }
                let content_start = if i < bytes.len() { i + 1 } else { i }; // after =begin\n

                let mut content_end = content_start;
                let mut block_end = i; // position of \n before =end or end of file
                let mut found_end = false;
                let mut j = content_start;
                while j < bytes.len() {
                    // Check for =end at line start
                    if (j == 0 || bytes[j - 1] == b'\n')
                        && bytes.len() >= j + 4
                        && &bytes[j..j + 4] == b"=end"
                        && (bytes.len() == j + 4 || bytes[j + 4] == b'\n' || bytes[j + 4] == b'\r')
                    {
                        content_end = j; // content ends before =end line
                        // block_end includes =end line + trailing newline
                        block_end = j + 4;
                        if block_end < bytes.len() && bytes[block_end] == b'\n' {
                            block_end += 1;
                        }
                        i = block_end;
                        found_end = true;
                        break;
                    }
                    j += 1;
                }
                if !found_end {
                    i = bytes.len();
                    content_end = i;
                    block_end = i;
                }

                // Build correction: convert content lines to # comments
                let content = &source[content_start..content_end];
                let replacement = build_block_comment_replacement(content);

                let correction = Correction {
                    edits: vec![Edit {
                        start_offset: block_start,
                        end_offset: block_end,
                        replacement,
                    }],
                };

                let offense = Offense::new(self.name(), MSG, self.severity(), loc, ctx.filename)
                    .with_correction(correction);
                offenses.push(offense);
                continue;
            }
            // Skip to next line
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            if i < bytes.len() {
                i += 1; // skip \n
            }
        }

        offenses
    }
}

fn build_block_comment_replacement(content: &str) -> String {
    // Convert =begin...=end body to # comment lines
    // Trailing newline from =begin line is already excluded from content
    let mut result = String::new();
    for line in content.lines() {
        if line.is_empty() {
            result.push_str("#\n");
        } else {
            result.push_str("# ");
            result.push_str(line);
            result.push('\n');
        }
    }
    // If content is empty (=begin\n=end), return empty string
    result
}

crate::register_cop!("Style/BlockComments", |_cfg| Some(Box::new(BlockComments::new())));
