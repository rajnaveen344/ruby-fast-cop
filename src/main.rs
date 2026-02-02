use ruby_fast_cop::{Config, check_file_with_config, find_unsupported_cops};
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Parse arguments
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut config_path: Option<PathBuf> = None;
    let mut show_warnings = true;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-c" | "--config" => {
                if i + 1 < args.len() {
                    config_path = Some(PathBuf::from(&args[i + 1]));
                    i += 2;
                } else {
                    eprintln!("Error: --config requires a path argument");
                    std::process::exit(1);
                }
            }
            "--no-warnings" => {
                show_warnings = false;
                i += 1;
            }
            "-h" | "--help" => {
                print_help();
                return;
            }
            "-v" | "--version" => {
                println!("ruby-fast-cop {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            arg if arg.starts_with('-') => {
                eprintln!("Unknown option: {}", arg);
                std::process::exit(1);
            }
            _ => {
                paths.push(PathBuf::from(&args[i]));
                i += 1;
            }
        }
    }

    // Default path if none specified
    if paths.is_empty() {
        paths.push(PathBuf::from("."));
    }

    // Load configuration
    let config = if let Some(ref path) = config_path {
        match Config::load_from_file(path) {
            Ok(cfg) => cfg,
            Err(e) => {
                eprintln!("Error loading config: {}", e);
                std::process::exit(1);
            }
        }
    } else {
        // Try to find .rubocop.yml in current directory or parent directories
        find_config_file().map_or_else(Config::default, |path| {
            Config::load_from_file(&path).unwrap_or_default()
        })
    };

    // Check for unsupported cops and warn
    if show_warnings {
        let unsupported = find_unsupported_cops(&config);
        if !unsupported.is_empty() {
            eprintln!("Warning: The following cops are not yet supported by ruby-fast-cop:");
            for cop in &unsupported {
                eprintln!("  - {}", cop);
            }
            eprintln!();
        }
    }

    let mut total_offenses = 0;
    let mut files_inspected = 0;

    for path in paths {
        if path.is_file() && path.extension().is_some_and(|ext| ext == "rb") {
            process_file(&path, &config, &mut files_inspected, &mut total_offenses);
        } else if path.is_dir() {
            // Walk directory for .rb files
            for entry in walkdir(&path) {
                if entry.extension().is_some_and(|ext| ext == "rb") {
                    process_file(&entry, &config, &mut files_inspected, &mut total_offenses);
                }
            }
        }
    }

    println!();
    println!(
        "{} files inspected, {} offenses detected",
        files_inspected, total_offenses
    );

    if total_offenses > 0 {
        std::process::exit(1);
    }
}

fn process_file(
    path: &PathBuf,
    config: &Config,
    files_inspected: &mut usize,
    total_offenses: &mut usize,
) {
    // Skip if globally excluded
    if config.is_excluded(path) {
        return;
    }

    match check_file_with_config(path, config) {
        Ok(offenses) => {
            *files_inspected += 1;
            for offense in &offenses {
                println!("{}", offense);
            }
            *total_offenses += offenses.len();
        }
        Err(e) => {
            eprintln!("Error reading {}: {}", path.display(), e);
        }
    }
}

fn find_config_file() -> Option<PathBuf> {
    let mut current = std::env::current_dir().ok()?;

    loop {
        let config_path = current.join(".rubocop.yml");
        if config_path.exists() {
            return Some(config_path);
        }

        if !current.pop() {
            break;
        }
    }

    None
}

fn print_help() {
    println!(
        r#"ruby-fast-cop - A fast Ruby linter written in Rust

USAGE:
    ruby-fast-cop [OPTIONS] [FILES/DIRS...]

OPTIONS:
    -c, --config <PATH>    Use specified config file instead of .rubocop.yml
    --no-warnings          Don't show warnings about unsupported cops
    -h, --help             Show this help message
    -v, --version          Show version

EXAMPLES:
    ruby-fast-cop                    Check all .rb files in current directory
    ruby-fast-cop app lib            Check .rb files in app/ and lib/ directories
    ruby-fast-cop foo.rb bar.rb      Check specific files
    ruby-fast-cop -c custom.yml .    Use custom config file

CONFIGURATION:
    ruby-fast-cop reads .rubocop.yml from the current directory or parent
    directories. It supports:

    - inherit_from (for local files, e.g., inherit_from: .rubocop-base.yml)
    - inherit_gem (for gems, uses Ruby's gem system to locate gems)
    - AllCops/Exclude patterns
    - Per-cop Enabled/Exclude settings
    - EnforcedStyle for style cops
    - Max for metrics cops

    Gem resolution uses Ruby's Gem::Specification (requires ruby in PATH).

SUPPORTED COPS:
    Layout/LineLength
    Lint/AssignmentInCondition
    Lint/Debugger
    Metrics/BlockLength
    Style/AutoResourceCleanup
    Style/FormatStringToken
    Style/HashSyntax
    Style/MethodCalledOnDoEndBlock
    Style/RaiseArgs
    Style/RescueStandardError
    Style/StringMethods

Unsupported cops in your config will be listed as warnings.
"#
    );
}

/// Simple recursive directory walker
fn walkdir(path: &std::path::Path) -> Vec<PathBuf> {
    let mut files = Vec::new();

    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // Skip hidden directories and common exclude patterns
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with('.') || name == "node_modules" || name == "vendor" {
                        continue;
                    }
                }
                files.extend(walkdir(&path));
            } else {
                files.push(path);
            }
        }
    }

    files
}
