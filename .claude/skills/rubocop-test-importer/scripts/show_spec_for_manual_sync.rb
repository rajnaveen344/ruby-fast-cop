#!/usr/bin/env ruby
# frozen_string_literal: true

# Helper script to show spec file content for manual YAML sync
#
# Usage:
#   ruby .claude/skills/rubocop-test-importer/scripts/show_spec_for_manual_sync.rb layout/line_length

require 'yaml'

SCRIPT_DIR = File.dirname(File.expand_path(__FILE__))
PROJECT_ROOT = File.expand_path('../../../../', SCRIPT_DIR)
FIXTURES_DIR = File.join(PROJECT_ROOT, 'tests/fixtures')
RUBOCOP_SPEC_DIR = '/tmp/rubocop-specs/spec/rubocop/cop'

def snake_to_camel(str)
  str.split('_').map(&:capitalize).join
end

def list_invalid_files
  puts "Invalid YAML files that need manual sync:\n\n"

  Dir.glob(File.join(FIXTURES_DIR, '**', '*.yaml')).each do |yaml_file|
    begin
      YAML.safe_load(File.read(yaml_file))
    rescue => e
      rel_path = yaml_file.sub(FIXTURES_DIR + '/', '').sub('.yaml', '')
      puts "  #{rel_path}"
    end
  end
end

def show_spec(dept_cop)
  parts = dept_cop.split('/')
  if parts.length != 2
    puts "Usage: ruby #{$0} department/cop_name"
    puts "Example: ruby #{$0} layout/line_length"
    exit 1
  end

  dept, cop = parts
  spec_file = File.join(RUBOCOP_SPEC_DIR, dept, "#{cop}_spec.rb")
  yaml_file = File.join(FIXTURES_DIR, dept, "#{cop}.yaml")

  unless File.exist?(spec_file)
    puts "Spec file not found: #{spec_file}"
    puts "Make sure RuboCop specs are downloaded first."
    exit 1
  end

  cop_name = "#{snake_to_camel(dept)}/#{snake_to_camel(cop)}"

  puts "=" * 80
  puts "SPEC FILE: #{spec_file}"
  puts "YAML FILE: #{yaml_file}"
  puts "COP NAME:  #{cop_name}"
  puts "=" * 80
  puts
  puts "SPEC CONTENT:"
  puts "-" * 80
  puts File.read(spec_file)
  puts "-" * 80
  puts
  puts "EXPECTED YAML STRUCTURE:"
  puts "-" * 80
  puts <<~YAML
    cop: #{cop_name}
    department: #{dept}
    severity: convention  # or warning for Lint cops
    implemented: false

    # NOTE: This file was manually synced from RuboCop specs.

    tests:
      - name: descriptive_test_name
        source: |
          # Ruby code here
          # Use spaces instead of tabs, or quoted strings with \\t
        offenses:
          - line: 1
            column_start: 1
            column_end: 10
            message: "Error message from expect_offense"
        # Optional fields:
        corrected: |
          # Corrected code (from expect_correction)
        config:
          EnforcedStyle: something
        ruby_version: ">= 3.1"
  YAML
  puts "-" * 80
  puts
  puts "INSTRUCTIONS:"
  puts "1. Parse the RSpec tests above"
  puts "2. Extract each 'it' block with expect_offense or expect_no_offenses"
  puts "3. The ^^^ markers indicate column positions"
  puts "4. Convert literal tabs to spaces or use quoted strings"
  puts "5. Add '# NOTE: This file was manually synced' comment"
end

if ARGV.empty?
  puts "Usage: ruby #{$0} department/cop_name\n\n"
  list_invalid_files
else
  show_spec(ARGV[0])
end
