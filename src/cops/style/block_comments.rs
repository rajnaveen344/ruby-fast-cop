use crate::cops::{CheckContext, Cop};
use crate::offense::{Location, Offense, Severity};

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
                // Found =begin — offense is just `=begin` (6 chars)
                let offense_end = i + 6;
                let loc = Location::from_offsets(source, i, offense_end);
                offenses.push(Offense::new(
                    self.name(),
                    MSG,
                    self.severity(),
                    loc,
                    ctx.filename,
                ));
                // Skip to =end
                i += 6;
                while i < bytes.len() {
                    if bytes[i] == b'\n' {
                        i += 1;
                        // Check for =end
                        if bytes.len() >= i + 4
                            && &bytes[i..i + 4] == b"=end"
                            && (bytes.len() == i + 4
                                || bytes[i + 4] == b'\n'
                                || bytes[i + 4] == b'\r')
                        {
                            i += 4;
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }
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

crate::register_cop!("Style/BlockComments", |_cfg| Some(Box::new(BlockComments::new())));
