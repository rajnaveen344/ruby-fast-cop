mod block_length;
mod class_length;
mod cyclomatic_complexity;
mod method_length;
mod perceived_complexity;

pub use block_length::BlockLength;
pub use class_length::ClassLength;
pub use cyclomatic_complexity::CyclomaticComplexity;
pub use method_length::MethodLength;
pub use perceived_complexity::PerceivedComplexity;

/// Deserializes a YAML sequence of strings as `Vec<String>`, gracefully returning an empty
/// vec when the value is absent, null, or any non-sequence type (e.g. a bare string like
/// `CountAsOne = "config"` in a test fixture).
pub(super) fn seq_or_empty<'de, D>(de: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    let v = serde_yaml::Value::deserialize(de)?;
    match v {
        serde_yaml::Value::Sequence(seq) => Ok(seq
            .into_iter()
            .filter_map(|item| item.as_str().map(|s| s.to_owned()))
            .collect()),
        _ => Ok(Vec::new()),
    }
}
