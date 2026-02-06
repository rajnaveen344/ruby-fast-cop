#!/usr/bin/env ruby
# frozen_string_literal: true

# Validates that all RuboCop cops have corresponding test TOML files
#
# Usage:
#   ruby .claude/skills/rubocop-test-importer/scripts/validate_test_coverage.rb

require 'fileutils'

# Find project root
SCRIPT_DIR = File.dirname(File.expand_path(__FILE__))
PROJECT_ROOT = File.expand_path('../../../../', SCRIPT_DIR)
FIXTURES_DIR = File.join(PROJECT_ROOT, 'tests/fixtures')
RUBOCOP_SPEC_DIR = '/tmp/rubocop-repo/spec/rubocop/cop'

DEPARTMENTS = %w[lint style layout metrics naming bundler gemspec security internal_affairs migration].freeze

def get_cop_specs
  cops = []
  DEPARTMENTS.each do |dept|
    spec_dir = File.join(RUBOCOP_SPEC_DIR, dept)
    next unless File.directory?(spec_dir)

    Dir.glob(File.join(spec_dir, '*_spec.rb')).each do |spec_file|
      basename = File.basename(spec_file, '_spec.rb')
      cop_name = basename.split('_').map(&:capitalize).join
      full_name = "#{dept.split('_').map(&:capitalize).join}/#{cop_name}"
      cops << { department: dept, name: full_name, spec_file: spec_file, toml_name: "#{basename}.toml" }
    end
  end
  cops
end

def get_toml_files
  tomls = []
  DEPARTMENTS.each do |dept|
    toml_dir = File.join(FIXTURES_DIR, dept)
    next unless File.directory?(toml_dir)

    Dir.glob(File.join(toml_dir, '*.toml')).each do |toml_file|
      tomls << { department: dept, file: toml_file, basename: File.basename(toml_file) }
    end
  end
  tomls
end

def validate_toml_file(toml_file)
  content = File.read(toml_file)

  # Basic validation: check for required keys
  cop = content[/^cop\s*=\s*"(.+?)"/, 1]
  return { valid: false, error: 'Missing cop field' } unless cop

  implemented = content.include?('implemented = true')
  has_tests = content.include?('[[tests]]')
  test_count = content.scan('[[tests]]').length

  {
    valid: true,
    cop: cop,
    implemented: implemented,
    test_count: test_count,
    has_offenses: content.include?('[[tests.offenses]]'),
    has_tests: has_tests
  }
rescue => e
  { valid: false, error: e.message }
end

def main
  puts "=== RuboCop Test Coverage Validation ===\n\n"

  unless File.directory?(RUBOCOP_SPEC_DIR)
    puts "Warning: RuboCop specs not downloaded yet."
    puts "Run: .claude/skills/rubocop-test-importer/scripts/download_rubocop_specs.sh\n\n"
  end

  specs = get_cop_specs
  tomls = get_toml_files

  spec_map = specs.map { |s| [s[:toml_name], s] }.to_h
  toml_map = tomls.map { |t| [t[:basename], t] }.to_h

  missing = []
  invalid = []
  stats = { total: 0, implemented: 0 }

  specs.each { |spec| missing << spec unless toml_map[spec[:toml_name]] }

  tomls.each do |toml|
    stats[:total] += 1
    info = validate_toml_file(toml[:file])

    unless info[:valid]
      invalid << { file: toml[:file], error: info[:error] }
      next
    end

    stats[:implemented] += 1 if info[:implemented]
  end

  puts "Summary"
  puts "-" * 40
  puts "Total TOML files:     #{stats[:total]}"
  puts "Implemented (true):   #{stats[:implemented]}"
  puts "Spec files found:     #{specs.size}"
  puts ""

  if missing.any?
    puts "Missing TOML files (#{missing.size}):"
    missing.each { |m| puts "   #{m[:department]}/#{m[:toml_name]}" }
    puts ""
  else
    puts "All spec files have corresponding TOML files"
    puts ""
  end

  if invalid.any?
    puts "Invalid TOML files (#{invalid.size}):"
    invalid.each { |i| puts "   - #{i[:file].sub(FIXTURES_DIR + '/', '')}" }
    puts ""
  end

  puts "By Department:"
  puts "-" * 40
  DEPARTMENTS.each do |dept|
    spec_count = specs.count { |s| s[:department] == dept }
    toml_count = tomls.count { |t| t[:department] == dept }
    status = spec_count == toml_count ? "ok" : "MISMATCH"
    printf "   %-20s specs: %3d  tomls: %3d  %s\n", dept, spec_count, toml_count, status
  end

  exit(1) if missing.any? || invalid.any?
end

main if __FILE__ == $PROGRAM_NAME
