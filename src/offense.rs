/// Severity level of an offense
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Convention,
    Warning,
    Error,
    Fatal,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Severity::Info => "I",
            Severity::Convention => "C",
            Severity::Warning => "W",
            Severity::Error => "E",
            Severity::Fatal => "F",
        };
        write!(f, "{}", s)
    }
}

/// Location in source code
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub line: u32,
    pub column: u32,
    pub last_line: u32,
    pub last_column: u32,
}

impl Location {
    pub fn new(line: u32, column: u32, last_line: u32, last_column: u32) -> Self {
        Self {
            line,
            column,
            last_line,
            last_column,
        }
    }

    /// Create a Location from byte offsets and source code
    pub fn from_offsets(source: &str, start_offset: usize, end_offset: usize) -> Self {
        let (start_line, start_col) = offset_to_line_col(source, start_offset);
        let (end_line, end_col) = offset_to_line_col(source, end_offset);
        Self {
            line: start_line,
            column: start_col,
            last_line: end_line,
            last_column: end_col,
        }
    }
}

/// Convert a byte offset to (line, column) - line is 1-indexed, column is 0-indexed (RuboCop convention)
fn offset_to_line_col(source: &str, offset: usize) -> (u32, u32) {
    let mut line = 1u32;
    let mut col = 0u32;

    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }

    (line, col)
}

/// A single offense found by a cop
#[derive(Debug, Clone)]
pub struct Offense {
    pub cop_name: String,
    pub message: String,
    pub severity: Severity,
    pub location: Location,
    pub filename: String,
}

impl Offense {
    pub fn new(
        cop_name: impl Into<String>,
        message: impl Into<String>,
        severity: Severity,
        location: Location,
        filename: impl Into<String>,
    ) -> Self {
        Self {
            cop_name: cop_name.into(),
            message: message.into(),
            severity,
            location,
            filename: filename.into(),
        }
    }
}

impl std::fmt::Display for Offense {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}:{}: {}: {}: {}",
            self.filename,
            self.location.line,
            self.location.column,
            self.severity,
            self.cop_name,
            self.message
        )
    }
}
