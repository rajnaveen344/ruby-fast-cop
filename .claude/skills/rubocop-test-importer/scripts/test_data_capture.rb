# frozen_string_literal: true

# Monkey-patch module that intercepts RuboCop's RSpec test helpers
# to capture fully-resolved test data (source, offenses, config, corrections).
#
# By the time expect_offense/expect_no_offenses are called, RSpec has
# already evaluated all let blocks, shared contexts, and string interpolation,
# so we get fully-resolved values without any $UNRESOLVED or #{...} placeholders.

module TestDataCapture
  # Thread-local storage for captured test data
  def self.captures
    Thread.current[:test_data_captures] ||= []
  end

  def self.pending_capture
    Thread.current[:pending_capture]
  end

  def self.pending_capture=(val)
    Thread.current[:pending_capture] = val
  end

  def self.reset!
    Thread.current[:test_data_captures] = []
    Thread.current[:pending_capture] = nil
  end

  def self.flush_pending!
    if (capture = self.pending_capture)
      self.captures << capture
      self.pending_capture = nil
    end
  end

  # Parse ^^^ annotation markers from annotated source.
  # Returns [clean_source, offenses_array]
  def self.parse_annotated_source(annotated_source)
    source_lines = []
    offenses = []

    annotated_source.each_line do |line|
      # Match annotation lines: leading whitespace + carets/underscores + optional message
      if line =~ /\A(\s*)((?:\^+|\^{}))\s?(.*)/
        prefix = $1
        carets = $2
        message = $3.rstrip

        line_number = source_lines.size # 1-indexed (current source line count)
        line_number = 1 if line_number == 0

        if carets == '^{}'
          # Zero-width offense marker
          offenses << {
            line: line_number,
            column_start: prefix.length,
            column_end: prefix.length,
            message: message
          }
        else
          offenses << {
            line: line_number,
            column_start: prefix.length,
            column_end: prefix.length + carets.length,
            message: message
          }
        end
      elsif line =~ /\A(\s*)(_{2,})\s?(.*)/
        # Underscore markers (used for offset placeholders — treat like carets)
        prefix = $1
        underscores = $2
        message = $3.rstrip

        line_number = source_lines.size
        line_number = 1 if line_number == 0

        offenses << {
          line: line_number,
          column_start: prefix.length,
          column_end: prefix.length + underscores.length,
          message: message
        }
      else
        source_lines << line
      end
    end

    clean_source = source_lines.join.chomp
    [clean_source, offenses]
  end

  # Override expect_offense to capture test data instead of running assertions
  def expect_offense(source, file = nil, severity: nil, chomp: false, **replacements)
    # Flush any pending capture from a previous expect_offense without expect_correction
    TestDataCapture.flush_pending!

    # Apply format replacements (same as RuboCop does)
    replacements.each do |keyword, value|
      value = value.to_s
      source = source.gsub("%{#{keyword}}", value)
                     .gsub("^{#{keyword}}", '^' * value.size)
                     .gsub("_{#{keyword}}", ' ' * value.size)
    end

    clean_source, offenses = TestDataCapture.parse_annotated_source(source)

    # Extract cop config if available
    config_hash = extract_config_hash

    TestDataCapture.pending_capture = {
      source: clean_source,
      offenses: offenses,
      config: config_hash,
      corrected: nil
    }
  rescue => e
    # If anything goes wrong, still try to call super so the test suite
    # doesn't break in unexpected ways. But log the error.
    $stderr.puts "[TestDataCapture] Error in expect_offense: #{e.message}"
    super
  end

  # Override expect_no_offenses to capture test data
  def expect_no_offenses(source, file = nil)
    TestDataCapture.flush_pending!

    config_hash = extract_config_hash

    TestDataCapture.captures << {
      source: source.chomp,
      offenses: [],
      config: config_hash,
      corrected: nil
    }
  rescue => e
    $stderr.puts "[TestDataCapture] Error in expect_no_offenses: #{e.message}"
    super
  end

  # Override expect_correction to attach corrected source to pending capture
  def expect_correction(correction, loop: true, source: nil)
    if TestDataCapture.pending_capture
      TestDataCapture.pending_capture[:corrected] = correction.chomp
      TestDataCapture.flush_pending!
    end
  rescue => e
    $stderr.puts "[TestDataCapture] Error in expect_correction: #{e.message}"
    super
  end

  # Override expect_no_corrections to flush pending capture without correction
  def expect_no_corrections
    TestDataCapture.flush_pending!
  end

  private

  # Extract the cop_config hash from the current test context.
  # This accesses the `cop_config` let variable which contains
  # the overrides hash (not full merged config).
  def extract_config_hash
    return {} unless respond_to?(:cop_config, true)

    raw_config = cop_config
    return {} unless raw_config.is_a?(Hash)

    # Convert symbol keys to string keys and normalize values
    normalize_config(raw_config)
  rescue NameError, NoMethodError
    {}
  end

  def normalize_config(hash)
    result = {}
    hash.each do |k, v|
      key = k.to_s
      result[key] = case v
                    when Hash then normalize_config(v)
                    when Symbol then v.to_s
                    else v
                    end
    end
    result
  end
end

# RSpec hook module — registers an after(:each) hook to flush
# any pending capture and attach test metadata.
module TestDataCaptureHook
  def self.install!
    RSpec.configure do |config|
      config.after(:each) do |example|
        # Flush any pending capture that wasn't followed by expect_correction
        TestDataCapture.flush_pending!

        # Attach metadata to each capture from this example
        cop_name = begin
          described_class.cop_name if described_class.respond_to?(:cop_name)
        rescue => e
          nil
        end

        test_name = example.full_description
          .gsub(/\s+/, '_')
          .gsub(/[^a-zA-Z0-9_]/, '')
          .downcase

        # Tag all captures from this example with metadata
        TestDataCapture.captures.each do |capture|
          capture[:test_name] ||= test_name
          capture[:cop_name] ||= cop_name
        end
      end
    end
  end
end
