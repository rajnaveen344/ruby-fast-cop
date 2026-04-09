#!/usr/bin/env ruby
# frozen_string_literal: true

# Extract RuboCop test data by running actual RSpec specs with monkey-patched
# expect_offense/expect_no_offenses/expect_correction methods.
#
# This captures fully-resolved test data — all Ruby string interpolation,
# let blocks, shared contexts, and config values are evaluated by RSpec
# before our patches see them.
#
# Usage:
#   cd /tmp/rubocop-repo && bundle exec ruby <path>/extract_via_rspec.rb --output <path>/tests/fixtures
#   cd /tmp/rubocop-repo && bundle exec ruby <path>/extract_via_rspec.rb --department style
#   cd /tmp/rubocop-repo && bundle exec ruby <path>/extract_via_rspec.rb --cop Style/RaiseArgs

require 'optparse'
require 'json'
require 'fileutils'

# --- Configuration ---

DEPARTMENTS = %w[lint style layout metrics naming bundler gemspec security migration].freeze

DEPARTMENT_MAP = {
  'lint' => 'Lint',
  'style' => 'Style',
  'layout' => 'Layout',
  'metrics' => 'Metrics',
  'naming' => 'Naming',
  'bundler' => 'Bundler',
  'gemspec' => 'Gemspec',
  'security' => 'Security',
  'migration' => 'Migration'
}.freeze

DEFAULT_SEVERITY = {
  'Lint' => 'warning',
  'Style' => 'convention',
  'Layout' => 'convention',
  'Metrics' => 'convention',
  'Naming' => 'convention',
  'Bundler' => 'convention',
  'Gemspec' => 'convention',
  'Security' => 'warning',
  'Migration' => 'convention'
}.freeze

# Cops that are implemented in ruby-fast-cop
IMPLEMENTED_COPS = %w[
  Lint/Debugger
  Lint/AssignmentInCondition
  Layout/LineLength
  Metrics/BlockLength
  Style/AutoResourceCleanup
  Style/FormatStringToken
  Style/HashSyntax
  Style/MethodCalledOnDoEndBlock
  Style/RaiseArgs
  Style/RescueStandardError
  Style/StringMethods
].freeze

# --- Option Parsing ---

options = {
  output_dir: nil,
  rubocop_dir: '/tmp/rubocop-repo',
  department: nil,
  cop: nil
}

OptionParser.new do |opts|
  opts.banner = "Usage: #{$PROGRAM_NAME} [options]"

  opts.on('--output DIR', 'Output fixtures directory (required)') do |dir|
    options[:output_dir] = dir
  end

  opts.on('--rubocop-dir DIR', 'RuboCop repo path (default: /tmp/rubocop-repo)') do |dir|
    options[:rubocop_dir] = dir
  end

  opts.on('--department DEPT', 'Process only one department (e.g., style)') do |dept|
    options[:department] = dept.downcase
  end

  opts.on('--cop COP', 'Process only one cop (e.g., Style/RaiseArgs)') do |cop|
    options[:cop] = cop
  end

  opts.on('-h', '--help', 'Show this help') do
    puts opts
    exit
  end
end.parse!

unless options[:output_dir]
  $stderr.puts "Error: --output DIR is required"
  exit 1
end

unless File.directory?(options[:rubocop_dir])
  $stderr.puts "Error: RuboCop repo not found at #{options[:rubocop_dir]}"
  $stderr.puts "Run: .claude/skills/rubocop-test-importer/scripts/download_rubocop_specs.sh"
  exit 1
end

# --- Setup RuboCop + RSpec Environment ---

# We must be running from within the rubocop repo with bundler
Dir.chdir(options[:rubocop_dir])

# Add spec dir to load path (needed for require_relative in spec files)
$LOAD_PATH.unshift(File.join(options[:rubocop_dir], 'spec'))
$LOAD_PATH.unshift(File.join(options[:rubocop_dir], 'lib'))

# Disable colors
require 'rainbow'
Rainbow.enabled = false

# Load RuboCop
require 'rubocop'
require 'rubocop/cop/internal_affairs'

# Load RSpec
require 'rspec/core'
require 'rspec/expectations'
require 'rspec/mocks'

# Load RuboCop's RSpec support (this defines ExpectOffense, shared contexts, etc.)
require 'rubocop/rspec/support'

# Load spec support files (shared examples, helpers, etc.)
support_dir = File.join(options[:rubocop_dir], 'spec', 'support')
if File.directory?(support_dir)
  Dir.glob(File.join(support_dir, '**', '*.rb')).sort.each do |f|
    begin
      require f
    rescue => e
      $stderr.puts "Warning: Failed to load support file #{f}: #{e.message}"
    end
  end
end

# Load core extensions used in specs
core_ext = File.join(options[:rubocop_dir], 'spec', 'core_ext', 'string.rb')
require core_ext if File.exist?(core_ext)

# Load our monkey-patch module
require_relative 'test_data_capture'

# Prepend our module onto RuboCop's ExpectOffense
# Prepend onto ExpectOffense (expect_offense, expect_no_offenses, expect_correction)
RuboCop::RSpec::ExpectOffense.prepend(TestDataCapture)
# Prepend onto CopHelper (inspect_source, _investigate)
CopHelper.prepend(TestDataCapture)

# --- TOML Generation Helpers ---

def toml_string(str)
  escaped = str.to_s
    .gsub('\\', '\\\\\\\\')
    .gsub('"', '\\"')
    .gsub("\n", '\\n')
    .gsub("\t", '\\t')
    .gsub("\r", '\\r')
  "\"#{escaped}\""
end

def toml_literal_string(str)
  content = str.to_s.chomp
  if content.include?("'''")
    toml_string(content)
  else
    "'''\n#{content}\n'''"
  end
end

def toml_key(key)
  if key =~ %r{[^a-zA-Z0-9_-]}
    "\"#{key}\""
  else
    key
  end
end

def toml_value(val)
  case val
  when String then toml_string(val)
  when Float
    return 'inf' if val == Float::INFINITY
    return '-inf' if val == -Float::INFINITY
    return 'nan' if val.nan?
    val.to_s
  when Integer then val.to_s
  when TrueClass, FalseClass then val.to_s
  when Array
    items = val.map { |v| toml_value(v) }
    "[#{items.join(', ')}]"
  when Hash
    # Inline table for nested hashes
    pairs = val.map { |k, v| "#{toml_key(k.to_s)} = #{toml_value(v)}" }
    "{ #{pairs.join(', ')} }"
  when NilClass then '""'
  else toml_string(val.to_s)
  end
end

def compute_base_indent(source)
  indents = source.lines
    .reject { |l| l.strip.empty? }
    .map { |l| l[/^ */].length }

  return nil if indents.empty?
  min_indent = indents.min
  min_indent > 0 ? min_indent : nil
end

def snake_case(str)
  str.gsub(/([A-Z]+)([A-Z][a-z])/, '\1_\2')
     .gsub(/([a-z\d])([A-Z])/, '\1_\2')
     .downcase
end

def ensure_utf8(str)
  return '' if str.nil?
  str = str.to_s
  if str.encoding != Encoding::UTF_8
    str = str.encode('UTF-8', invalid: :replace, undef: :replace, replace: '?')
  elsif !str.valid_encoding?
    str = str.encode('UTF-8', 'UTF-8', invalid: :replace, undef: :replace, replace: '?')
  end
  str
end

def generate_toml(cop:, department:, severity:, implemented:, tests:)
  lines = []
  lines << "cop = #{toml_string(cop)}"
  lines << "department = #{toml_string(department)}"
  lines << "severity = #{toml_string(severity)}"
  lines << "implemented = #{implemented}"
  lines << ""

  tests.each do |test|
    lines << "[[tests]]"
    lines << "name = #{toml_string(ensure_utf8(test[:test_name] || 'unnamed'))}"

    # Output ruby_version if it differs from the default (2.7 in RuboCop).
    # RuboCop's default TargetRubyVersion is 2.7 (RuboCop::TargetRuby::DEFAULT_VERSION).
    if test[:ruby_version] && test[:ruby_version] != 2.7
      lines << "ruby_version = #{toml_string(">= #{test[:ruby_version]}")}"
    end

    # Output filename if captured (used by cops like Naming/FileName)
    if test[:filename].is_a?(String) && !test[:filename].empty?
      lines << "filename = #{toml_string(ensure_utf8(test[:filename]))}"
    end

    source = ensure_utf8(test[:source] || '')
    base_indent = compute_base_indent(source)

    if base_indent && base_indent > 0
      stripped = source.lines.map { |l|
        if l.strip.empty?
          "\n"
        else
          l.sub(/^ {#{base_indent}}/, '')
        end
      }.join.chomp
      lines << "source = #{toml_literal_string(stripped)}"
      lines << "base_indent = #{base_indent}"
    else
      lines << "source = #{toml_literal_string(source)}"
    end

    if test[:corrected]
      lines << "corrected = #{toml_literal_string(ensure_utf8(test[:corrected]))}"
    end

    offenses = test[:offenses] || []
    if offenses.empty?
      lines << "offenses = []"
    else
      offenses.each do |o|
        lines << ""
        lines << "[[tests.offenses]]"
        lines << "line = #{o[:line]}"
        lines << "column_start = #{o[:column_start]}"
        lines << "column_end = #{o[:column_end]}"
        lines << "message = #{toml_string(ensure_utf8(o[:message]))}"
      end
    end

    config = test[:config]
    if config && !config.empty?
      # Separate scalar config (for [tests.config]) from cross-cop config (sub-tables)
      scalar_config = {}
      cross_cop_config = {}
      config.sort.each do |key, value|
        if value.is_a?(Hash) && key.include?('/')
          cross_cop_config[key] = value
        else
          scalar_config[key] = value
        end
      end

      unless scalar_config.empty?
        lines << ""
        lines << "[tests.config]"
        scalar_config.sort.each do |key, value|
          lines << "#{toml_key(key)} = #{toml_value(value)}"
        end
      end

      # Output cross-cop config as separate TOML table sections
      cross_cop_config.sort.each do |cop_key, cop_value|
        lines << ""
        lines << "[tests.config.#{toml_key(cop_key)}]"
        cop_value.sort.each do |k, v|
          lines << "#{toml_key(k)} = #{toml_value(v)}"
        end
      end
    end

    lines << ""
  end

  lines.join("\n")
end

# --- Spec Discovery ---

def discover_spec_files(rubocop_dir, department: nil, cop: nil)
  spec_dir = File.join(rubocop_dir, 'spec', 'rubocop', 'cop')
  files = []

  if cop
    # Single cop: e.g., "Style/RaiseArgs" -> spec/rubocop/cop/style/raise_args_spec.rb
    dept, cop_name = cop.split('/')
    dept_lower = snake_case(dept)
    cop_snake = snake_case(cop_name)
    spec_file = File.join(spec_dir, dept_lower, "#{cop_snake}_spec.rb")
    if File.exist?(spec_file)
      files << { department: dept_lower, spec_file: spec_file }
    else
      $stderr.puts "Warning: Spec file not found: #{spec_file}"
    end
  elsif department
    dept_dir = File.join(spec_dir, department)
    if File.directory?(dept_dir)
      Dir.glob(File.join(dept_dir, '*_spec.rb')).sort.each do |f|
        files << { department: department, spec_file: f }
      end
    else
      $stderr.puts "Warning: Department directory not found: #{dept_dir}"
    end
  else
    DEPARTMENTS.each do |dept|
      dept_dir = File.join(spec_dir, dept)
      next unless File.directory?(dept_dir)
      Dir.glob(File.join(dept_dir, '*_spec.rb')).sort.each do |f|
        files << { department: dept, spec_file: f }
      end
    end
  end

  files
end

def cop_name_from_spec(spec_file, department)
  basename = File.basename(spec_file, '_spec.rb')
  cop_class_name = basename.split('_').map(&:capitalize).join
  dept_namespace = DEPARTMENT_MAP[department] || department.split('_').map(&:capitalize).join
  "#{dept_namespace}/#{cop_class_name}"
end

# --- Read existing TOML files to preserve `implemented` flag ---

def read_existing_implemented(output_dir)
  implemented_set = {}
  Dir.glob(File.join(output_dir, '**', '*.toml')).each do |f|
    content = File.read(f)
    if content =~ /^cop\s*=\s*"(.+?)"/
      cop_name = $1
      if content.include?('implemented = true')
        implemented_set[cop_name] = true
      end
    end
  end
  implemented_set
end

# --- Main Extraction Logic ---

def run_spec_file(spec_file)
  TestDataCapture.reset!

  # Configure RSpec for programmatic execution
  config = RSpec.configuration
  world = RSpec.world

  # Silence RSpec output
  config.output_stream = StringIO.new
  config.error_stream = StringIO.new

  # Load and run the spec file
  begin
    # Clear any previously loaded examples
    world.reset

    # Load the spec file
    load spec_file

    # Run the examples
    runner = RSpec::Core::Runner.new(RSpec::Core::ConfigurationOptions.new([]))

    # Run all examples that were just loaded
    world.ordered_example_groups.each do |group|
      group.run(config.reporter)
    end
  rescue => e
    $stderr.puts "  Error running #{spec_file}: #{e.message}"
    $stderr.puts "  #{e.backtrace.first(3).join("\n  ")}"
  ensure
    # Reset RSpec world for next file
    RSpec.world.reset
    RSpec.clear_examples
  end

  # Return captured test data
  TestDataCapture.captures.dup
end

def process_captures(captures, cop_name)
  # Deduplicate by test name (in case of shared examples running multiple times)
  seen_names = {}
  unique_captures = []

  # Determine the actual cop name from captures. The filename-derived cop_name
  # may not match described_class.cop_name (e.g., split spec files like
  # conditional_assignment_assign_in_condition_spec.rb → described_class is
  # Style/ConditionalAssignment, not Style/ConditionalAssignmentAssignInCondition).
  # We use the most common cop_name from captures as the authoritative name.
  actual_cop_names = captures.map { |c| c[:cop_name] }.compact
  actual_cop_name = actual_cop_names.tally.max_by { |_, count| count }&.first

  # Use actual cop name from captures if available, otherwise fall back to filename-derived
  effective_cop_name = actual_cop_name || cop_name

  captures.each do |capture|
    # Skip captures that don't belong to this cop
    next if capture[:cop_name] && capture[:cop_name] != effective_cop_name

    name = capture[:test_name] || "unnamed_#{unique_captures.size}"

    # Ensure unique test names
    if seen_names[name]
      suffix = 2
      while seen_names["#{name}_#{suffix}"]
        suffix += 1
      end
      name = "#{name}_#{suffix}"
    end
    seen_names[name] = true

    capture[:test_name] = name
    unique_captures << capture
  end

  [unique_captures, effective_cop_name]
end

# --- Main ---

output_dir = options[:output_dir]
rubocop_dir = options[:rubocop_dir]

# Read existing implemented flags
existing_implemented = read_existing_implemented(output_dir)

# Install the RSpec after(:each) hook
TestDataCaptureHook.install!

# Discover spec files
spec_files = discover_spec_files(rubocop_dir,
                                  department: options[:department],
                                  cop: options[:cop])

if spec_files.empty?
  $stderr.puts "No spec files found."
  exit 1
end

puts "Found #{spec_files.size} spec file(s) to process"
puts ""

# Phase 1: Run all spec files and collect tests grouped by actual cop name.
# Multiple spec files can map to the same cop (e.g., ConditionalAssignment has
# two spec files). We merge their tests into a single TOML file.

stats = { processed: 0, tests_captured: 0, files_written: 0, errors: 0, zero_tests: [] }

# cop_name => { dept:, tests:[], spec_files:[] }
cops_data = {}

spec_files.each do |entry|
  dept = entry[:department]
  spec_file = entry[:spec_file]
  filename_cop_name = cop_name_from_spec(spec_file, dept)

  print "  Processing #{filename_cop_name}..."
  $stdout.flush

  begin
    captures = run_spec_file(spec_file)
    tests, effective_cop_name = process_captures(captures, filename_cop_name)

    if tests.empty?
      puts " 0 tests (WARNING: no test data captured)"
      stats[:zero_tests] << { spec_file: spec_file, cop_name: filename_cop_name }

      # Still register the cop so an empty fixture is written (prevents stale data)
      cops_data[filename_cop_name] ||= { dept: dept, tests: [], spec_files: [] }
      cops_data[filename_cop_name][:spec_files] << spec_file
    else
      cop_name = effective_cop_name

      if cop_name != filename_cop_name
        puts " #{tests.size} tests captured (actual cop: #{cop_name})"
      else
        puts " #{tests.size} tests captured"
      end

      cops_data[cop_name] ||= { dept: dept, tests: [], spec_files: [] }
      cops_data[cop_name][:tests].concat(tests)
      cops_data[cop_name][:spec_files] << spec_file
      stats[:tests_captured] += tests.size
    end
  rescue => e
    puts " ERROR: #{e.message}"
    $stderr.puts "  #{e.backtrace.first(3).join("\n  ")}"
    stats[:errors] += 1
  end

  stats[:processed] += 1
end

# Phase 2: Write one TOML file per cop (merging tests from multiple spec files).

puts ""
puts "Writing TOML files..."

cops_data.each do |cop_name, data|
  dept = data[:dept]
  tests = data[:tests]

  implemented = IMPLEMENTED_COPS.include?(cop_name) ||
                existing_implemented[cop_name] == true

  severity = DEFAULT_SEVERITY[cop_name.split('/').first] || 'convention'

  toml_content = generate_toml(
    cop: cop_name,
    department: dept,
    severity: severity,
    implemented: implemented,
    tests: tests
  )

  # Determine output filename from the cop name (snake_case)
  cop_short = cop_name.split('/').last
  cop_snake = snake_case(cop_short)
  output_subdir = File.join(output_dir, dept)
  FileUtils.mkdir_p(output_subdir)
  toml_file = File.join(output_subdir, "#{cop_snake}.toml")

  # Ensure valid UTF-8 output
  toml_content = toml_content
    .encode('UTF-8', invalid: :replace, undef: :replace, replace: '?')
  File.write(toml_file, toml_content, encoding: 'UTF-8')

  if data[:spec_files].size > 1
    puts "  #{cop_name}: #{tests.size} tests (merged from #{data[:spec_files].size} spec files)"
  end

  stats[:files_written] += 1

  # Clean up stale split-spec TOML files that no longer match the canonical filename
  data[:spec_files].each do |sf|
    old_snake = File.basename(sf, '_spec.rb')
    old_toml = File.join(output_subdir, "#{old_snake}.toml")
    if old_toml != toml_file && File.exist?(old_toml)
      File.delete(old_toml)
      puts "  Removed stale split-spec file: #{old_toml}"
    end
  end
end

puts ""
puts "Summary:"
puts "  Spec files processed: #{stats[:processed]}"
puts "  TOML files written:   #{stats[:files_written]}"
puts "  Total tests captured: #{stats[:tests_captured]}"
puts "  Errors:               #{stats[:errors]}"

if stats[:zero_tests].any?
  puts ""
  puts "WARNING: #{stats[:zero_tests].size} spec(s) produced 0 test captures:"
  stats[:zero_tests].each do |entry|
    puts "  - #{entry[:cop_name]} (#{entry[:spec_file]})"
  end
end
