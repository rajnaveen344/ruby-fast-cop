#!/usr/bin/env ruby
# frozen_string_literal: true

# Script to extract RuboCop RSpec tests into TOML fixtures for ruby-fast-cop
#
# Usage:
#   ruby .claude/skills/rubocop-test-importer/scripts/sync_rubocop_tests.rb
#   ruby .claude/skills/rubocop-test-importer/scripts/sync_rubocop_tests.rb --update
#
# Reads from: /tmp/rubocop-specs/spec/rubocop/cop/
# Outputs to: tests/fixtures/{department}/{cop_name}.toml

require 'yaml'
require 'fileutils'
require 'optparse'

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

# Departments we care about
DEPARTMENTS = %w[lint style layout metrics naming bundler gemspec security internal_affairs migration].freeze

# Mapping from department directory name to cop namespace
DEPARTMENT_MAP = {
  'lint' => 'Lint',
  'style' => 'Style',
  'layout' => 'Layout',
  'metrics' => 'Metrics',
  'naming' => 'Naming',
  'bundler' => 'Bundler',
  'gemspec' => 'Gemspec',
  'security' => 'Security',
  'internal_affairs' => 'InternalAffairs',
  'migration' => 'Migration'
}.freeze

# Default severity by department
DEFAULT_SEVERITY = {
  'Lint' => 'warning',
  'Style' => 'convention',
  'Layout' => 'convention',
  'Metrics' => 'convention',
  'Naming' => 'convention',
  'Bundler' => 'convention',
  'Gemspec' => 'convention',
  'Security' => 'warning',
  'InternalAffairs' => 'convention',
  'Migration' => 'convention'
}.freeze

class RSpecTestExtractor
  attr_reader :source, :tests

  def initialize(source)
    @source = source
    @tests = []
  end

  def parse
    extract_tests_with_context
    @tests
  end

  private

  def extract_tests_with_context
    context_stack = []
    let_blocks = {}
    current_ruby_version = nil
    lines = source.lines
    i = 0

    while i < lines.length
      line = lines[i]

      if line =~ /^\s*(RSpec\.describe|describe|context)\s+(['"](.+?)['"]|[\w:]+)/
        context_name = $3 || $2
        context_stack << { name: context_name, indent: line[/^\s*/].length }

        if line =~ /:ruby(\d+)/
          version = $1
          major = version[0]
          minor = version[1] || '0'
          current_ruby_version = ">= #{major}.#{minor}"
        elsif context_name =~ /Ruby\s*(>=?|<=?)\s*(\d+\.\d+)/i
          current_ruby_version = "#{$1} #{$2}"
        end
      end

      if line =~ /^\s*let\(:cop_config\)\s*(?:do|\{)/
        let_content = extract_block_content(lines, i)
        let_blocks['cop_config'] = let_content if let_content
      end

      if line =~ /^\s*end\s*$/
        indent = line[/^\s*/].length
        while context_stack.any? && context_stack.last[:indent] >= indent
          context_stack.pop
        end
      end

      if line =~ /^\s*(it|specify)\s+['"](.+?)['"]/
        test_name = $2
        ruby_version_tag = nil

        if line =~ /:ruby(\d+)/
          version = $1
          major = version[0]
          minor = version[1] || '0'
          ruby_version_tag = ">= #{major}.#{minor}"
        end

        block_content = extract_block_content(lines, i)

        if block_content
          test = parse_test_block(block_content, context_stack, test_name, let_blocks)
          if test
            test[:ruby_version] = ruby_version_tag || current_ruby_version if ruby_version_tag || current_ruby_version
            @tests << test
          end
        end
      end

      i += 1
    end
  end

  def extract_block_content(lines, start_index)
    first_line = lines[start_index]

    if first_line =~ /\bdo\s*$/
      indent = first_line[/^\s*/].length
      content_lines = []
      i = start_index + 1
      nesting = 1

      while i < lines.length && nesting > 0
        line = lines[i]
        if line =~ /^\s*(?:if|unless|case|while|until|for|begin|def|class|module)\b.*\bdo\b|\bdo\s*(?:\|[^|]*\|)?\s*$/
          nesting += 1
        elsif line =~ /^\s{0,#{indent}}end\b/
          nesting -= 1
        end
        content_lines << line if nesting > 0
        i += 1
      end

      content_lines.join
    elsif first_line =~ /\{\s*$/
      content_lines = []
      i = start_index + 1
      nesting = 1

      while i < lines.length && nesting > 0
        line = lines[i]
        nesting += line.count('{') - line.count('}')
        content_lines << line if nesting > 0
        i += 1
      end

      content_lines.join
    else
      if first_line =~ /do\s+(.+?)\s+end/
        $1
      elsif first_line =~ /\{\s*(.+?)\s*\}/
        $1
      else
        nil
      end
    end
  end

  def parse_test_block(block_content, context_stack, test_name, let_blocks)
    # Extract expect_offense with its specific heredoc
    if block_content =~ /expect_offense\s*\(?\s*<<[~-]?['"]?(\w+)['"]?/
      offense_marker = $1
      heredoc_content = extract_heredoc_for_method(block_content, 'expect_offense', offense_marker)
      return nil unless heredoc_content

      source_code, offenses = parse_offense_heredoc(heredoc_content)
      return nil if source_code.nil? || source_code.empty?

      # Extract expect_correction separately - it has its own heredoc
      corrected = nil
      if block_content =~ /expect_correction\s*\(?\s*<<[~-]?['"]?(\w+)['"]?/
        correction_marker = $1
        corrected = extract_heredoc_for_method(block_content, 'expect_correction', correction_marker)
        corrected = process_squiggly_heredoc(corrected) if corrected
      end

      config = extract_cop_config(let_blocks)

      return {
        name: build_test_name(context_stack, test_name),
        source: source_code,
        offenses: offenses,
        corrected: corrected,
        config: config
      }
    end

    if block_content =~ /expect_no_offenses\s*\(?\s*<<[~-]?['"]?(\w+)['"]?/
      marker = $1
      heredoc_content = extract_heredoc_for_method(block_content, 'expect_no_offenses', marker)
      return nil unless heredoc_content

      heredoc_content = process_squiggly_heredoc(heredoc_content)
      config = extract_cop_config(let_blocks)

      return {
        name: build_test_name(context_stack, test_name),
        source: heredoc_content,
        offenses: [],
        config: config
      }
    end

    if block_content =~ /expect_no_offenses\s*\(\s*(['"])(.*?)\1\s*\)/m
      quote = $1
      raw_string = $2
      source_code = unescape_ruby_string(raw_string, quote)
      config = extract_cop_config(let_blocks)

      return {
        name: build_test_name(context_stack, test_name),
        source: source_code,
        offenses: [],
        config: config
      }
    end

    nil
  end

  # Extract heredoc content for a specific method call
  def extract_heredoc_for_method(content, method_name, marker)
    pattern = /#{Regexp.escape(method_name)}\s*\(?\s*<<[~-]?['"]?#{Regexp.escape(marker)}['"]?\s*(?:,\s*[^)]+)?\)?\s*\n(.*?)\n\s*#{Regexp.escape(marker)}/m

    if content =~ pattern
      heredoc_content = $1
      if content =~ /#{Regexp.escape(method_name)}\s*\(?\s*<<~/
        heredoc_content = process_squiggly_heredoc(heredoc_content)
      end
      heredoc_content
    else
      nil
    end
  end

  def process_squiggly_heredoc(content)
    return content unless content
    lines = content.lines
    return content if lines.empty?

    min_indent = lines
      .reject { |l| l.strip.empty? }
      .map { |l| l[/^\s*/].length }
      .min || 0

    lines.map { |l|
      if l.strip.empty?
        "\n"
      elsif l.length > min_indent
        l[min_indent..]
      else
        l.lstrip
      end
    }.join.chomp
  end

  def parse_offense_heredoc(content)
    lines = content.lines
    source_lines = []
    offenses = []
    i = 0

    while i < lines.length
      line = lines[i]

      if i + 1 < lines.length && lines[i + 1] =~ /^\s*[\^_]+[\s{]/
        source_lines << line.chomp
        marker_line = lines[i + 1]
        offense = parse_marker_line(marker_line, source_lines.length)
        offenses << offense if offense
        i += 2
      elsif line =~ /^\s*[\^_]+[\s{]/
        offense = parse_marker_line(line, source_lines.length)
        offenses << offense if offense
        i += 1
      else
        source_lines << line.chomp
        i += 1
      end
    end

    [source_lines.join("\n"), offenses]
  end

  def parse_marker_line(line, line_number)
    return nil unless line =~ /^(\s*)([\^_]+)\s+(.+)/

    prefix = $1
    carets = $2
    message = $3.strip

    return nil if prefix =~ /[_^]\{\w+\}/ || carets =~ /[_^]\{\w+\}/

    {
      line: line_number,
      column_start: prefix.length,
      column_end: prefix.length + carets.length,
      message: message
    }
  end

  def extract_cop_config(let_blocks)
    config = {}
    return config unless let_blocks['cop_config']

    config_source = let_blocks['cop_config']
    config_source.scan(/['"](\w+)['"]\s*=>\s*(?:(['"])([^'"]*)\2|(\w+)|(\d+))/m) do |key, _quote, str_val, word_val, num_val|
      value = str_val || word_val || num_val
      if value
        value = value.to_i if value =~ /^\d+$/
        value = true if value == 'true'
        value = false if value == 'false'
        config[key] = value
      end
    end

    config
  end

  def build_test_name(context_stack, test_name)
    parts = context_stack.map { |c| c[:name].to_s.gsub(/\s+/, '_').gsub(/[^a-zA-Z0-9_]/, '') }
    parts << test_name.to_s.gsub(/\s+/, '_').gsub(/[^a-zA-Z0-9_]/, '')
    parts.reject(&:empty?).join('__').downcase
  end

  def unescape_ruby_string(str, quote)
    result = str.dup
    result.gsub!('\\n', "\n")
    result.gsub!('\\t', "\t")
    result.gsub!('\\r', "\r")
    result.gsub!('\\\\', '\\')
    quote == "'" ? result.gsub!("\\'", "'") : result.gsub!('\\"', '"')
    result
  end
end

class TestSyncer
  attr_reader :rubocop_dir, :output_dir, :update_mode

  def initialize(rubocop_dir:, output_dir:, update_mode: false)
    @rubocop_dir = rubocop_dir
    @output_dir = output_dir
    @update_mode = update_mode
    @stats = { new: 0, updated: 0, unchanged: 0, skipped: 0, errors: 0 }
  end

  def run
    puts "Syncing RuboCop tests..."
    puts "  Source: #{rubocop_dir}"
    puts "  Output: #{output_dir}"
    puts

    DEPARTMENTS.each { |dept| process_department(dept) }
    print_summary
  end

  private

  def process_department(dept)
    spec_dir = File.join(rubocop_dir, dept)
    return unless File.directory?(spec_dir)

    spec_files = Dir.glob(File.join(spec_dir, '*_spec.rb'))
    return if spec_files.empty?

    puts "Processing #{DEPARTMENT_MAP[dept]}..."
    spec_files.each { |spec_file| process_spec_file(spec_file, dept) }
  end

  def process_spec_file(spec_file, dept)
    cop_name = File.basename(spec_file, '_spec.rb').split('_').map(&:capitalize).join
    full_cop_name = "#{DEPARTMENT_MAP[dept]}/#{cop_name}"

    begin
      source = File.read(spec_file)
      tests = RSpecTestExtractor.new(source).parse

      if tests.empty?
        @stats[:skipped] += 1
        return
      end

      write_toml(dept, cop_name, full_cop_name, tests)
    rescue => e
      @stats[:errors] += 1
      puts "  ERROR: #{full_cop_name}: #{e.message}"
    end
  end

  def write_toml(dept, cop_name, full_cop_name, tests)
    output_subdir = File.join(output_dir, dept)
    FileUtils.mkdir_p(output_subdir)

    toml_file = File.join(output_subdir, "#{snake_case(cop_name)}.toml")

    implemented = IMPLEMENTED_COPS.include?(full_cop_name)
    if File.exist?(toml_file) && update_mode
      content = File.read(toml_file)
      implemented = true if content.include?('implemented = true')
    end

    severity = DEFAULT_SEVERITY[full_cop_name.split('/').first] || 'convention'

    toml_content = generate_toml(
      cop: full_cop_name,
      department: dept,
      severity: severity,
      implemented: implemented,
      tests: tests
    )

    if File.exist?(toml_file)
      existing_content = File.read(toml_file)
      if existing_content == toml_content
        @stats[:unchanged] += 1
        return
      end
      @stats[:updated] += 1
      action = 'Updated'
    else
      @stats[:new] += 1
      action = 'Created'
    end

    File.write(toml_file, toml_content)
    puts "  #{action}: #{toml_file} (#{tests.length} tests)"
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
      lines << "name = #{toml_string(test[:name])}"

      # Source - check if it needs base_indent
      source = test[:source] || ''
      base_indent = compute_base_indent(source)

      if base_indent && base_indent > 0
        # Strip the base indentation from source
        stripped = source.lines.map { |l|
          if l.strip.empty?
            "\n"
          elsif l.length > base_indent
            l[base_indent..]
          else
            l.lstrip
          end
        }.join.chomp
        lines << "source = #{toml_literal_string(stripped)}"
        lines << "base_indent = #{base_indent}"
      else
        lines << "source = #{toml_literal_string(source)}"
      end

      # Corrected (optional)
      if test[:corrected]
        lines << "corrected = #{toml_literal_string(test[:corrected])}"
      end

      # Ruby version (optional)
      if test[:ruby_version]
        lines << "ruby_version = #{toml_string(test[:ruby_version])}"
      end

      # Interpolated/verified (optional)
      if test[:interpolated]
        lines << "interpolated = true"
        lines << "verified = false"
      end

      # Offenses
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
          lines << "message = #{toml_string(o[:message])}"
        end
      end

      # Config (optional)
      config = test[:config]
      if config && !config.empty?
        lines << ""
        lines << "[tests.config]"
        config.sort.each do |key, value|
          lines << "#{toml_key(key)} = #{toml_value(value)}"
        end
      end

      lines << ""
    end

    lines.join("\n")
  end

  # Compute base indentation needed for TOML literal strings
  # Returns base indent if first line is more indented than zero
  def compute_base_indent(source)
    indents = source.lines
      .reject { |l| l.strip.empty? }
      .map { |l| l[/^\s*/].length }

    return nil if indents.empty?

    min_indent = indents.min
    first_indent = indents.first

    # Only need base_indent if the minimum indentation is > 0
    # (TOML literal strings preserve indentation as-is)
    min_indent > 0 ? min_indent : nil
  end

  # TOML basic string (double-quoted, with escaping)
  def toml_string(str)
    escaped = str.to_s
      .gsub('\\', '\\\\\\\\')
      .gsub('"', '\\"')
      .gsub("\n", '\\n')
      .gsub("\t", '\\t')
      .gsub("\r", '\\r')
    "\"#{escaped}\""
  end

  # TOML literal string (triple-single-quoted, no escaping)
  # Falls back to basic string if content contains '''
  def toml_literal_string(str)
    content = str.to_s.chomp
    if content.include?("'''")
      # Fall back to basic string with escaping
      toml_string(content)
    else
      "'''\n#{content}\n'''"
    end
  end

  # TOML key - quote if contains special characters
  def toml_key(key)
    if key =~ %r{[^a-zA-Z0-9_-]}
      "\"#{key}\""
    else
      key
    end
  end

  # Convert a Ruby value to TOML representation
  def toml_value(val)
    case val
    when String
      toml_string(val)
    when Integer, Float
      val.to_s
    when TrueClass, FalseClass
      val.to_s
    when Array
      items = val.map { |v| toml_value(v) }
      "[#{items.join(', ')}]"
    when NilClass
      '""'
    else
      toml_string(val.to_s)
    end
  end

  def snake_case(str)
    str.gsub(/([A-Z]+)([A-Z][a-z])/, '\1_\2').gsub(/([a-z\d])([A-Z])/, '\1_\2').downcase
  end

  def print_summary
    puts "\nSummary:"
    puts "  New files:       #{@stats[:new]}"
    puts "  Updated files:   #{@stats[:updated]}"
    puts "  Unchanged files: #{@stats[:unchanged]}"
    puts "  Skipped files:   #{@stats[:skipped]}"
    puts "  Errors:          #{@stats[:errors]}"
  end
end

# Main execution
if __FILE__ == $PROGRAM_NAME
  # Find project root (where tests/fixtures should be)
  script_dir = File.dirname(File.expand_path(__FILE__))
  project_root = File.expand_path('../../../../', script_dir)

  options = {
    rubocop_dir: '/tmp/rubocop-specs/spec/rubocop/cop',
    output_dir: File.join(project_root, 'tests/fixtures'),
    update: false
  }

  OptionParser.new do |opts|
    opts.banner = "Usage: #{$PROGRAM_NAME} [options]"
    opts.on('--update', 'Re-sync existing files') { options[:update] = true }
    opts.on('--source DIR', 'RuboCop specs directory') { |dir| options[:rubocop_dir] = dir }
    opts.on('--output DIR', 'Output directory') { |dir| options[:output_dir] = dir }
    opts.on('-h', '--help', 'Show this help') { puts opts; exit }
  end.parse!

  unless File.directory?(options[:rubocop_dir])
    puts "Error: RuboCop specs directory not found: #{options[:rubocop_dir]}"
    puts "Run: .claude/skills/rubocop-test-importer/scripts/download_rubocop_specs.sh"
    exit 1
  end

  TestSyncer.new(
    rubocop_dir: options[:rubocop_dir],
    output_dir: options[:output_dir],
    update_mode: options[:update]
  ).run
end
