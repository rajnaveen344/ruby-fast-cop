//! Configuration file parsing for .rubocop.yml
//!
//! Supports:
//! - Reading .rubocop.yml files
//! - inherit_from for files and gems
//! - Per-cop configuration (Enabled, Exclude, custom options)
//! - Global AllCops configuration

use globset::Glob;
use serde::{Deserialize, Deserializer};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Main configuration structure
#[derive(Debug, Default)]
pub struct Config {
    /// Global configuration from AllCops
    pub all_cops: AllCopsConfig,
    /// Per-cop configuration
    pub cops: HashMap<String, CopConfig>,
    /// Cops that were requested but are not implemented
    pub unsupported_cops: HashSet<String>,
    /// Base directory for resolving relative paths
    base_dir: PathBuf,
}

/// AllCops configuration
#[derive(Debug, Default, Clone)]
pub struct AllCopsConfig {
    /// Files/patterns to exclude globally
    pub exclude: Vec<String>,
    /// Target Ruby version
    pub target_ruby_version: Option<f64>,
    /// Whether to use cache
    pub use_cache: Option<bool>,
    /// New cops behavior
    pub new_cops: Option<String>,
}

/// Per-cop configuration
#[derive(Debug, Default, Clone)]
pub struct CopConfig {
    /// Whether the cop is enabled
    pub enabled: Option<bool>,
    /// Files/patterns to exclude for this cop
    pub exclude: Vec<String>,
    /// Files/patterns to include for this cop
    pub include: Vec<String>,
    /// Severity override
    pub severity: Option<String>,
    /// Style enforcement (for style cops)
    pub enforced_style: Option<String>,
    /// Max value (for metrics cops)
    pub max: Option<usize>,
    /// Allow safe assignment (for Lint/AssignmentInCondition)
    pub allow_safe_assignment: Option<bool>,
    /// Count comments (for metrics cops)
    pub count_comments: Option<bool>,
    /// All raw options for custom processing
    pub raw: HashMap<String, serde_yaml::Value>,
}

/// Raw YAML structure for parsing
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct RawConfig {
    inherit_from: Option<InheritFrom>,
    /// inherit_gem is RuboCop's way to inherit from gems
    /// Format: { "gem-name": ["config/default.yml"] } or { "gem-name": "config/default.yml" }
    inherit_gem: Option<HashMap<String, GemConfigPaths>>,
    #[serde(rename = "AllCops")]
    all_cops: Option<RawAllCops>,
    #[serde(flatten)]
    cops: HashMap<String, serde_yaml::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum InheritFrom {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum GemConfigPaths {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct RawAllCops {
    #[serde(
        rename = "Exclude",
        default,
        deserialize_with = "deserialize_string_or_vec"
    )]
    exclude: Vec<String>,
    #[serde(rename = "TargetRubyVersion")]
    target_ruby_version: Option<f64>,
    #[serde(rename = "UseCache")]
    use_cache: Option<bool>,
    #[serde(rename = "NewCops")]
    new_cops: Option<String>,
}

fn deserialize_string_or_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrVec {
        String(String),
        Vec(Vec<String>),
    }

    match StringOrVec::deserialize(deserializer)? {
        StringOrVec::String(s) => Ok(vec![s]),
        StringOrVec::Vec(v) => Ok(v),
    }
}

impl Config {
    /// Load configuration from .rubocop.yml in the given directory
    pub fn load(dir: &Path) -> Self {
        let config_path = dir.join(".rubocop.yml");
        if config_path.exists() {
            Self::load_from_file(&config_path).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    /// Load configuration from a specific file
    pub fn load_from_file(path: &Path) -> Result<Self, ConfigError> {
        let base_dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let content = std::fs::read_to_string(path)
            .map_err(|e| ConfigError::IoError(path.to_path_buf(), e.to_string()))?;

        Self::parse_with_inheritance(&content, &base_dir)
    }

    fn parse_with_inheritance(content: &str, base_dir: &Path) -> Result<Self, ConfigError> {
        let raw: RawConfig =
            serde_yaml::from_str(content).map_err(|e| ConfigError::ParseError(e.to_string()))?;

        let mut config = Config {
            base_dir: base_dir.to_path_buf(),
            ..Default::default()
        };

        // Process inherit_gem first (gems are loaded before files, per RuboCop behavior)
        if let Some(inherit_gems) = raw.inherit_gem {
            for (gem_name, config_paths) in inherit_gems {
                let paths = match config_paths {
                    GemConfigPaths::Single(s) => vec![s],
                    GemConfigPaths::Multiple(v) => v,
                };

                if let Some(gem_dir) = config.resolve_gem_path(&gem_name) {
                    for config_path in paths {
                        let full_path = gem_dir.join(&config_path);
                        if full_path.exists() {
                            if let Ok(inherited) = Self::load_from_file(&full_path) {
                                config.merge(&inherited);
                            }
                        }
                    }
                }
                // If gem not found, skip silently (common for optional deps)
            }
        }

        // Process inherit_from (files only - this is how RuboCop works)
        if let Some(inherit) = raw.inherit_from {
            let paths = match inherit {
                InheritFrom::Single(s) => vec![s],
                InheritFrom::Multiple(v) => v,
            };

            for inherit_path in paths {
                config.merge_inherited_file(&inherit_path)?;
            }
        }

        // Process AllCops
        if let Some(all_cops) = raw.all_cops {
            config.all_cops.exclude.extend(all_cops.exclude);
            if all_cops.target_ruby_version.is_some() {
                config.all_cops.target_ruby_version = all_cops.target_ruby_version;
            }
            if all_cops.use_cache.is_some() {
                config.all_cops.use_cache = all_cops.use_cache;
            }
            if all_cops.new_cops.is_some() {
                config.all_cops.new_cops = all_cops.new_cops;
            }
        }

        // Process individual cops
        for (name, value) in raw.cops {
            // Skip non-cop entries
            if name == "inherit_from" || name == "require" || name == "inherit_mode" {
                continue;
            }

            let cop_config = Self::parse_cop_config(&value)?;
            // Merge with existing config (from inherit_from) rather than overwriting
            config
                .cops
                .entry(name)
                .and_modify(|existing| {
                    // Child config takes precedence, but inherit unset values from parent
                    if cop_config.enabled.is_some() {
                        existing.enabled = cop_config.enabled;
                    }
                    existing.exclude.extend(cop_config.exclude.clone());
                    existing.include.extend(cop_config.include.clone());
                    if cop_config.severity.is_some() {
                        existing.severity = cop_config.severity.clone();
                    }
                    if cop_config.enforced_style.is_some() {
                        existing.enforced_style = cop_config.enforced_style.clone();
                    }
                    if cop_config.max.is_some() {
                        existing.max = cop_config.max;
                    }
                    if cop_config.allow_safe_assignment.is_some() {
                        existing.allow_safe_assignment = cop_config.allow_safe_assignment;
                    }
                    if cop_config.count_comments.is_some() {
                        existing.count_comments = cop_config.count_comments;
                    }
                    for (key, value) in &cop_config.raw {
                        existing.raw.insert(key.clone(), value.clone());
                    }
                })
                .or_insert(cop_config);
        }

        Ok(config)
    }

    /// Merge an inherited file (inherit_from handles files only, like RuboCop)
    fn merge_inherited_file(&mut self, inherit_path: &str) -> Result<(), ConfigError> {
        // Resolve the file path
        let full_path = if Path::new(inherit_path).is_absolute() {
            PathBuf::from(inherit_path)
        } else {
            self.base_dir.join(inherit_path)
        };

        if full_path.exists() {
            let inherited = Self::load_from_file(&full_path)?;
            self.merge(&inherited);
        }
        // If file doesn't exist, skip silently (might be optional)

        Ok(())
    }

    /// Resolve a gem's installation path using Ruby's gem system
    /// This matches RuboCop's behavior: Gem::Specification.find_by_name(gem_name).gem_dir
    fn resolve_gem_path(&self, gem_name: &str) -> Option<PathBuf> {
        // Method 1: Use Ruby's gem system directly (most accurate)
        if let Some(path) = self.resolve_gem_via_ruby(gem_name) {
            return Some(path);
        }

        // Method 2: Try bundler if available
        if let Some(path) = self.resolve_gem_via_bundler(gem_name) {
            return Some(path);
        }

        None
    }

    /// Use Ruby to find gem path (matches RuboCop's exact behavior)
    fn resolve_gem_via_ruby(&self, gem_name: &str) -> Option<PathBuf> {
        // Run: ruby -e "puts Gem::Specification.find_by_name('gem_name').gem_dir"
        let output = std::process::Command::new("ruby")
            .arg("-e")
            .arg(format!(
                "puts Gem::Specification.find_by_name('{}').gem_dir",
                gem_name
            ))
            .output()
            .ok()?;

        if output.status.success() {
            let path_str = String::from_utf8_lossy(&output.stdout);
            let path = PathBuf::from(path_str.trim());
            if path.exists() {
                return Some(path);
            }
        }

        None
    }

    /// Try to find gem using bundler
    fn resolve_gem_via_bundler(&self, gem_name: &str) -> Option<PathBuf> {
        // Find Gemfile.lock in base_dir or parents
        let mut search_dir = self.base_dir.clone();
        loop {
            if search_dir.join("Gemfile.lock").exists() {
                // Try `bundle show gem_name`
                if let Ok(output) = std::process::Command::new("bundle")
                    .arg("show")
                    .arg(gem_name)
                    .current_dir(&search_dir)
                    .output()
                {
                    if output.status.success() {
                        let path_str = String::from_utf8_lossy(&output.stdout);
                        let path = PathBuf::from(path_str.trim());
                        if path.exists() {
                            return Some(path);
                        }
                    }
                }
                break;
            }
            if !search_dir.pop() {
                break;
            }
        }
        None
    }

    fn merge(&mut self, other: &Config) {
        // Merge AllCops excludes
        self.all_cops.exclude.extend(other.all_cops.exclude.clone());

        if other.all_cops.target_ruby_version.is_some() {
            self.all_cops.target_ruby_version = other.all_cops.target_ruby_version;
        }

        // Merge cops (other takes precedence)
        for (name, cop_config) in &other.cops {
            self.cops
                .entry(name.clone())
                .and_modify(|existing| existing.merge(cop_config))
                .or_insert_with(|| cop_config.clone());
        }
    }

    fn parse_cop_config(value: &serde_yaml::Value) -> Result<CopConfig, ConfigError> {
        let mut config = CopConfig::default();

        if let serde_yaml::Value::Mapping(map) = value {
            for (key, val) in map {
                if let serde_yaml::Value::String(key_str) = key {
                    match key_str.as_str() {
                        "Enabled" => {
                            config.enabled = val.as_bool();
                        }
                        "Exclude" => {
                            if let Some(seq) = val.as_sequence() {
                                config.exclude = seq
                                    .iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect();
                            }
                        }
                        "Include" => {
                            if let Some(seq) = val.as_sequence() {
                                config.include = seq
                                    .iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect();
                            }
                        }
                        "Severity" => {
                            config.severity = val.as_str().map(String::from);
                        }
                        "EnforcedStyle" => {
                            config.enforced_style = val.as_str().map(String::from);
                        }
                        "Max" => {
                            config.max = val.as_u64().map(|v| v as usize);
                        }
                        "AllowSafeAssignment" => {
                            config.allow_safe_assignment = val.as_bool();
                        }
                        "CountComments" => {
                            config.count_comments = val.as_bool();
                        }
                        _ => {}
                    }
                    // Store all raw values for custom processing
                    config.raw.insert(key_str.clone(), val.clone());
                }
            }
        }

        Ok(config)
    }

    /// Check if a file should be excluded globally
    pub fn is_excluded(&self, file_path: &Path) -> bool {
        let file_str = file_path.to_string_lossy();
        for pattern in &self.all_cops.exclude {
            if self.matches_pattern(&file_str, pattern) {
                return true;
            }
        }
        false
    }

    /// Check if a file should be excluded for a specific cop
    pub fn is_excluded_for_cop(&self, file_path: &Path, cop_name: &str) -> bool {
        // Check global excludes first
        if self.is_excluded(file_path) {
            return true;
        }

        // Check cop-specific excludes
        if let Some(cop_config) = self.cops.get(cop_name) {
            let file_str = file_path.to_string_lossy();
            for pattern in &cop_config.exclude {
                if self.matches_pattern(&file_str, pattern) {
                    return true;
                }
            }
        }

        false
    }

    /// Check if a cop is enabled
    pub fn is_cop_enabled(&self, cop_name: &str) -> bool {
        self.cops
            .get(cop_name)
            .and_then(|c| c.enabled)
            .unwrap_or(true)
    }

    /// Get cop configuration
    pub fn get_cop_config(&self, cop_name: &str) -> Option<&CopConfig> {
        self.cops.get(cop_name)
    }

    fn matches_pattern(&self, file_path: &str, pattern: &str) -> bool {
        // Build glob matcher - globset supports brace expansion {a,b}, **, ?, [abc], etc.
        let glob = match Glob::new(pattern) {
            Ok(g) => g,
            Err(_) => return false,
        };
        let matcher = glob.compile_matcher();

        // Try direct match first
        if matcher.is_match(file_path) {
            return true;
        }

        // Strip leading "./" from path
        let normalized_path = file_path.trim_start_matches("./");
        if matcher.is_match(normalized_path) {
            return true;
        }

        // For absolute paths, try matching against path suffixes
        // This handles patterns like "vendor/**/*" matching "/project/vendor/foo.rb"
        if file_path.starts_with('/') {
            // Extract possible root directories from the pattern
            let pattern_roots = self.extract_pattern_roots(pattern);

            for root in &pattern_roots {
                // Find if this root exists in the path and try matching from that point
                for (i, _) in file_path.match_indices(&format!("/{}/", root)) {
                    let suffix = &file_path[i + 1..];
                    if matcher.is_match(suffix) {
                        return true;
                    }
                }
                // Also try at end (for paths like /project/vendor)
                if let Some(i) = file_path.rfind(&format!("/{}", root)) {
                    let suffix = &file_path[i + 1..];
                    if matcher.is_match(suffix) {
                        return true;
                    }
                }
            }
        }

        // Try pattern without leading "./" if it has one
        let normalized_pattern = pattern.trim_start_matches("./");
        if normalized_pattern != pattern {
            if let Ok(norm_glob) = Glob::new(normalized_pattern) {
                let norm_matcher = norm_glob.compile_matcher();
                if norm_matcher.is_match(file_path) || norm_matcher.is_match(normalized_path) {
                    return true;
                }
            }
        }

        false
    }

    /// Extract possible root directory names from a pattern
    /// Handles brace expansion like "{spec,test}/**/*" -> ["spec", "test"]
    fn extract_pattern_roots(&self, pattern: &str) -> Vec<String> {
        let pattern = pattern.trim_start_matches("./");
        let first_component = pattern.split('/').next().unwrap_or("");

        // Check if first component has brace expansion
        if first_component.starts_with('{') && first_component.contains('}') {
            // Extract content between braces
            if let Some(start) = first_component.find('{') {
                if let Some(end) = first_component.find('}') {
                    let inside = &first_component[start + 1..end];
                    return inside.split(',').map(|s| s.trim().to_string()).collect();
                }
            }
        }

        // No brace expansion, return the first component if it's not a wildcard
        if !first_component.is_empty()
            && !first_component.starts_with('*')
            && !first_component.contains('?')
        {
            vec![first_component.to_string()]
        } else {
            vec![]
        }
    }

    /// Mark a cop as unsupported
    pub fn mark_unsupported(&mut self, cop_name: &str) {
        self.unsupported_cops.insert(cop_name.to_string());
    }

    /// Get list of configured but unsupported cops
    pub fn get_unsupported_cops(&self) -> Vec<&String> {
        self.unsupported_cops.iter().collect()
    }

    /// Create a Config with a single cop's configuration from YAML value
    /// This is useful for testing where each test case has its own config.
    /// Also handles cross-cop config: if a key's value is a mapping and the key
    /// looks like a PascalCase cop short name, store it as a separate cop config
    /// under `{department}/{key}` using the primary cop's department.
    pub fn from_cop_toml(cop_name: &str, yaml_value: &serde_yaml::Value) -> Self {
        let mut config = Config::default();

        if yaml_value.is_null() || yaml_value.as_mapping().map_or(true, |m| m.is_empty()) {
            return config;
        }

        // Extract the department from the cop name (e.g., "Layout" from "Layout/SpaceAfterComma")
        let department = cop_name.split('/').next().unwrap_or("");

        // Look for cross-cop config entries: keys whose values are mappings
        // and look like PascalCase cop short names (e.g., "SpaceInsideBlockBraces")
        if let Some(mapping) = yaml_value.as_mapping() {
            for (key, val) in mapping {
                if let Some(key_str) = key.as_str() {
                    // A cross-cop config entry is a PascalCase key whose value is a mapping
                    if val.is_mapping() && is_pascal_case(key_str) && !department.is_empty() {
                        let cross_cop_name = format!("{}/{}", department, key_str);
                        if let Ok(cross_config) = Self::parse_cop_config(val) {
                            config.cops.insert(cross_cop_name, cross_config);
                        }
                    }
                }
            }
        }

        if let Ok(cop_config) = Self::parse_cop_config(yaml_value) {
            config.cops.insert(cop_name.to_string(), cop_config);
        }

        config
    }
}

/// Check if a string looks like a PascalCase cop short name
/// (starts with uppercase, contains at least one lowercase, no slashes)
fn is_pascal_case(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_uppercase() => {}
        _ => return false,
    }
    // Must contain at least one lowercase letter and no slashes
    s.chars().any(|c| c.is_ascii_lowercase()) && !s.contains('/')
}

impl CopConfig {
    fn merge(&mut self, other: &CopConfig) {
        if other.enabled.is_some() {
            self.enabled = other.enabled;
        }
        self.exclude.extend(other.exclude.clone());
        self.include.extend(other.include.clone());
        if other.severity.is_some() {
            self.severity = other.severity.clone();
        }
        if other.enforced_style.is_some() {
            self.enforced_style = other.enforced_style.clone();
        }
        if other.max.is_some() {
            self.max = other.max;
        }
        if other.allow_safe_assignment.is_some() {
            self.allow_safe_assignment = other.allow_safe_assignment;
        }
        if other.count_comments.is_some() {
            self.count_comments = other.count_comments;
        }
        for (key, value) in &other.raw {
            self.raw.insert(key.clone(), value.clone());
        }
    }
}

/// Errors that can occur during config parsing
#[derive(Debug)]
pub enum ConfigError {
    IoError(PathBuf, String),
    ParseError(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::IoError(path, err) => {
                write!(f, "Failed to read config file {:?}: {}", path, err)
            }
            ConfigError::ParseError(err) => {
                write!(f, "Failed to parse config: {}", err)
            }
        }
    }
}

impl std::error::Error for ConfigError {}

/// List of all supported cop names
pub const SUPPORTED_COPS: &[&str] = &[
    "Layout/LeadingCommentSpace",
    "Layout/LineLength",
    "Layout/SpaceAfterComma",
    "Layout/TrailingEmptyLines",
    "Layout/TrailingWhitespace",
    "Lint/AssignmentInCondition",
    "Lint/Debugger",
    "Lint/DuplicateMethods",
    "Lint/LiteralInInterpolation",
    "Lint/RedundantTypeConversion",
    "Metrics/BlockLength",
    "Metrics/ClassLength",
    "Metrics/MethodLength",
    "Style/AccessModifierDeclarations",
    "Style/AutoResourceCleanup",
    "Style/ConditionalAssignment",
    "Style/FormatStringToken",
    "Style/FrozenStringLiteralComment",
    "Style/HashSyntax",
    "Style/MethodCalledOnDoEndBlock",
    "Style/MutableConstant",
    "Style/NegativeArrayIndex",
    "Style/NumericLiterals",
    "Style/RaiseArgs",
    "Style/RescueStandardError",
    "Style/SafeNavigation",
    "Style/Semicolon",
    "Style/StringLiterals",
    "Style/StringMethods",
];

/// Check if a cop name is supported
pub fn is_supported_cop(name: &str) -> bool {
    SUPPORTED_COPS.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_config() {
        let yaml = r#"
AllCops:
  Exclude:
    - 'vendor/**/*'
    - 'db/schema.rb'

Style/HashSyntax:
  EnforcedStyle: ruby19_no_mixed_keys

Metrics/BlockLength:
  Max: 50
  Exclude:
    - 'spec/**/*'
"#;
        let config = Config::parse_with_inheritance(yaml, Path::new(".")).unwrap();

        assert_eq!(config.all_cops.exclude.len(), 2);
        assert!(config.all_cops.exclude.contains(&"vendor/**/*".to_string()));

        let hash_syntax = config.cops.get("Style/HashSyntax").unwrap();
        assert_eq!(
            hash_syntax.enforced_style,
            Some("ruby19_no_mixed_keys".to_string())
        );

        let block_length = config.cops.get("Metrics/BlockLength").unwrap();
        assert_eq!(block_length.max, Some(50));
        assert!(block_length.exclude.contains(&"spec/**/*".to_string()));
    }

    #[test]
    fn test_cop_enabled() {
        let yaml = r#"
Style/HashSyntax:
  Enabled: false

Style/StringMethods:
  Enabled: true
"#;
        let config = Config::parse_with_inheritance(yaml, Path::new(".")).unwrap();

        assert!(!config.is_cop_enabled("Style/HashSyntax"));
        assert!(config.is_cop_enabled("Style/StringMethods"));
        assert!(config.is_cop_enabled("Style/NonExistent")); // Default is enabled
    }

    #[test]
    fn test_pattern_matching() {
        let config = Config::default();

        // Basic patterns
        assert!(config.matches_pattern("vendor/bundle/foo.rb", "vendor/**/*"));
        assert!(config.matches_pattern("db/schema.rb", "db/schema.rb"));
        assert!(config.matches_pattern("spec/models/user_spec.rb", "spec/**/*"));
    }

    #[test]
    fn test_pattern_matching_brace_expansion() {
        let config = Config::default();

        // Brace expansion {a,b} - matches either a or b
        assert!(config.matches_pattern("spec/models/user_spec.rb", "{spec,test}/**/*"));
        assert!(config.matches_pattern("test/models/user_test.rb", "{spec,test}/**/*"));
        assert!(!config.matches_pattern("app/models/user.rb", "{spec,test}/**/*"));

        // Multiple options in braces
        assert!(config.matches_pattern("spec/foo.rb", "{spec,test,features}/**/*"));
        assert!(config.matches_pattern("test/foo.rb", "{spec,test,features}/**/*"));
        assert!(config.matches_pattern("features/foo.rb", "{spec,test,features}/**/*"));
    }

    #[test]
    fn test_pattern_matching_recursive() {
        let config = Config::default();

        // ** matches any depth
        assert!(config.matches_pattern("a/b/c/d/e.rb", "**/*.rb"));
        assert!(config.matches_pattern("foo.rb", "**/*.rb"));
        assert!(config.matches_pattern("spec/models/concerns/foo_spec.rb", "spec/**/*_spec.rb"));
    }

    #[test]
    fn test_pattern_matching_character_class() {
        let config = Config::default();

        // [abc] character class
        assert!(config.matches_pattern("test.rb", "tes[t].rb"));
        assert!(config.matches_pattern("file1.rb", "file[0-9].rb"));
        assert!(!config.matches_pattern("filea.rb", "file[0-9].rb"));
    }

    #[test]
    fn test_pattern_matching_single_wildcard() {
        let config = Config::default();

        // ? matches single character
        assert!(config.matches_pattern("file1.rb", "file?.rb"));
        assert!(config.matches_pattern("filea.rb", "file?.rb"));
        assert!(!config.matches_pattern("file12.rb", "file?.rb"));
    }

    #[test]
    fn test_is_excluded_for_cop() {
        let yaml = r#"
AllCops:
  Exclude:
    - 'vendor/**/*'

Metrics/BlockLength:
  Max: 50
  Exclude:
    - 'spec/**/*_spec.rb'
    - '{db,config}/**/*'
"#;
        let config = Config::parse_with_inheritance(yaml, Path::new(".")).unwrap();

        // Global excludes
        assert!(config.is_excluded(Path::new("vendor/bundle/foo.rb")));
        assert!(!config.is_excluded(Path::new("app/models/user.rb")));

        // Cop-specific excludes
        assert!(
            config
                .is_excluded_for_cop(Path::new("spec/models/user_spec.rb"), "Metrics/BlockLength")
        );
        assert!(config.is_excluded_for_cop(
            Path::new("db/migrate/001_create_users.rb"),
            "Metrics/BlockLength"
        ));
        assert!(config.is_excluded_for_cop(Path::new("config/routes.rb"), "Metrics/BlockLength"));
        assert!(
            !config.is_excluded_for_cop(Path::new("app/models/user.rb"), "Metrics/BlockLength")
        );

        // Global exclude applies to all cops
        assert!(
            config.is_excluded_for_cop(Path::new("vendor/bundle/foo.rb"), "Metrics/BlockLength")
        );
    }

    #[test]
    fn test_exclude_with_absolute_paths() {
        let yaml = r#"
AllCops:
  Exclude:
    - 'vendor/**/*'

Metrics/BlockLength:
  Exclude:
    - 'spec/**/*_spec.rb'
"#;
        let config = Config::parse_with_inheritance(yaml, Path::new(".")).unwrap();

        // Absolute paths should still match relative patterns
        // This is how RuboCop works - it uses paths relative to project root
        assert!(config.is_excluded(Path::new("/project/vendor/bundle/foo.rb")));
        assert!(config.is_excluded_for_cop(
            Path::new("/project/spec/models/user_spec.rb"),
            "Metrics/BlockLength"
        ));
    }

    #[test]
    fn test_exclude_brace_expansion_with_absolute_paths() {
        let yaml = r#"
Metrics/BlockLength:
  Exclude:
    - '{spec,test}/**/*'
    - '{db,config}/**/*'
"#;
        let config = Config::parse_with_inheritance(yaml, Path::new(".")).unwrap();

        // Brace expansion with absolute paths
        assert!(config.is_excluded_for_cop(
            Path::new("/my/project/spec/models/user_spec.rb"),
            "Metrics/BlockLength"
        ));
        assert!(config.is_excluded_for_cop(
            Path::new("/my/project/test/models/user_test.rb"),
            "Metrics/BlockLength"
        ));
        assert!(config.is_excluded_for_cop(
            Path::new("/my/project/db/migrate/001_create.rb"),
            "Metrics/BlockLength"
        ));
        assert!(config.is_excluded_for_cop(
            Path::new("/my/project/config/routes.rb"),
            "Metrics/BlockLength"
        ));

        // Should NOT match app directory
        assert!(!config.is_excluded_for_cop(
            Path::new("/my/project/app/models/user.rb"),
            "Metrics/BlockLength"
        ));
    }

    #[test]
    fn test_inherit_from() {
        use std::fs;
        let temp_dir = std::env::temp_dir().join("rubocop_test_inherit");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        // Create parent config
        fs::write(
            temp_dir.join("parent.yml"),
            r#"
Layout/LineLength:
  Max: 80
Style/HashSyntax:
  EnforcedStyle: hash_rockets
"#,
        )
        .unwrap();

        // Create child config
        fs::write(
            temp_dir.join(".rubocop.yml"),
            r#"
inherit_from: parent.yml

Style/HashSyntax:
  EnforcedStyle: ruby19

Layout/LineLength:
  Exclude:
    - 'spec/**/*'
"#,
        )
        .unwrap();

        let config = Config::load(&temp_dir);

        // Child's HashSyntax should override parent
        let hash_syntax = config.cops.get("Style/HashSyntax").unwrap();
        assert_eq!(hash_syntax.enforced_style, Some("ruby19".to_string()));

        // Parent's LineLength Max should be inherited
        let line_length = config.cops.get("Layout/LineLength").unwrap();
        assert_eq!(line_length.max, Some(80));
        assert!(line_length.exclude.contains(&"spec/**/*".to_string()));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_inherit_from_is_file_only() {
        use std::fs;
        let temp_dir = std::env::temp_dir().join("rubocop_test_file_inherit");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        // Create a shared config file
        fs::write(
            temp_dir.join("shared.yml"),
            r#"
Layout/LineLength:
  Max: 100
"#,
        )
        .unwrap();

        // Create project config that inherits from file
        fs::write(
            temp_dir.join(".rubocop.yml"),
            r#"
inherit_from: shared.yml

Metrics/BlockLength:
  Max: 30
"#,
        )
        .unwrap();

        let config = Config::load(&temp_dir);

        // Should have inherited from file
        let line_length = config.cops.get("Layout/LineLength").unwrap();
        assert_eq!(line_length.max, Some(100));

        // Should have local config too
        let block_length = config.cops.get("Metrics/BlockLength").unwrap();
        assert_eq!(block_length.max, Some(30));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_inherit_from_nonexistent_file_is_skipped() {
        use std::fs;
        let temp_dir = std::env::temp_dir().join("rubocop_test_missing_file");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        // Create project config that inherits from non-existent file
        fs::write(
            temp_dir.join(".rubocop.yml"),
            r#"
inherit_from: nonexistent.yml

Metrics/BlockLength:
  Max: 30
"#,
        )
        .unwrap();

        // Should not panic, just skip the missing file
        let config = Config::load(&temp_dir);

        // Should have local config
        let block_length = config.cops.get("Metrics/BlockLength").unwrap();
        assert_eq!(block_length.max, Some(30));

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
