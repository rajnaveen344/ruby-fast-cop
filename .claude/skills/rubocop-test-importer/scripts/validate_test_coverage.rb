#!/usr/bin/env ruby
# frozen_string_literal: true

# Validates that all RuboCop cops have corresponding test YAML files
#
# Usage:
#   ruby .claude/skills/rubocop-test-importer/scripts/validate_test_coverage.rb

require 'yaml'
require 'fileutils'

# Find project root
SCRIPT_DIR = File.dirname(File.expand_path(__FILE__))
PROJECT_ROOT = File.expand_path('../../../../', SCRIPT_DIR)
FIXTURES_DIR = File.join(PROJECT_ROOT, 'tests/fixtures')
RUBOCOP_SPEC_DIR = '/tmp/rubocop-specs/spec/rubocop/cop'

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
      cops << { department: dept, name: full_name, spec_file: spec_file, yaml_name: "#{basename}.yaml" }
    end
  end
  cops
end

def get_yaml_files
  yamls = []
  DEPARTMENTS.each do |dept|
    yaml_dir = File.join(FIXTURES_DIR, dept)
    next unless File.directory?(yaml_dir)

    Dir.glob(File.join(yaml_dir, '*.yaml')).each do |yaml_file|
      yamls << { department: dept, file: yaml_file, basename: File.basename(yaml_file) }
    end
  end
  yamls
end

def validate_yaml_file(yaml_file)
  content = YAML.safe_load(File.read(yaml_file), permitted_classes: [Symbol])
  return { valid: false, error: 'Failed to parse' } unless content

  tests = content['tests'] || []
  {
    valid: true,
    cop: content['cop'],
    implemented: content['implemented'],
    test_count: tests.size,
    has_offenses: tests.any? { |t| t['offenses'] && !t['offenses'].empty? },
    manually_synced: File.read(yaml_file).include?('manually synced')
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
  yamls = get_yaml_files

  spec_map = specs.map { |s| [s[:yaml_name], s] }.to_h
  yaml_map = yamls.map { |y| [y[:basename], y] }.to_h

  missing = []
  invalid = []
  manual = []
  stats = { total: 0, implemented: 0, auto_synced: 0, manual_synced: 0 }

  specs.each { |spec| missing << spec unless yaml_map[spec[:yaml_name]] }

  yamls.each do |yaml|
    stats[:total] += 1
    info = validate_yaml_file(yaml[:file])

    unless info[:valid]
      invalid << { file: yaml[:file], error: info[:error] }
      next
    end

    stats[:implemented] += 1 if info[:implemented]
    if info[:manually_synced]
      stats[:manual_synced] += 1
      manual << yaml[:file]
    else
      stats[:auto_synced] += 1
    end
  end

  puts "Summary"
  puts "-" * 40
  puts "Total YAML files:     #{stats[:total]}"
  puts "Auto-synced:          #{stats[:auto_synced]}"
  puts "Manually synced:      #{stats[:manual_synced]}"
  puts "Implemented (true):   #{stats[:implemented]}"
  puts "Spec files found:     #{specs.size}"
  puts ""

  if missing.any?
    puts "Missing YAML files (#{missing.size}):"
    missing.each { |m| puts "   #{m[:department]}/#{m[:yaml_name]}" }
    puts ""
  else
    puts "All spec files have corresponding YAML files"
    puts ""
  end

  if invalid.any?
    puts "Invalid YAML files (#{invalid.size}):"
    puts "   These need manual fixing (tabs, indentation, special chars)"
    invalid.each { |i| puts "   - #{i[:file].sub(FIXTURES_DIR + '/', '')}" }
    puts ""
    puts "   To fix: /rubocop-test-importer fix <dept/cop>"
    puts ""
  end

  if manual.any?
    puts "Manually synced files (#{manual.size}):"
    manual.each { |m| puts "   #{m.sub(FIXTURES_DIR + '/', '')}" }
    puts ""
  end

  puts "By Department:"
  puts "-" * 40
  DEPARTMENTS.each do |dept|
    spec_count = specs.count { |s| s[:department] == dept }
    yaml_count = yamls.count { |y| y[:department] == dept }
    status = spec_count == yaml_count ? "ok" : "MISMATCH"
    printf "   %-20s specs: %3d  yamls: %3d  %s\n", dept, spec_count, yaml_count, status
  end

  exit(1) if missing.any? || invalid.any?
end

main if __FILE__ == $PROGRAM_NAME
