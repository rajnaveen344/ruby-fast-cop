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
    Thread.current[:inside_expect] = false
  end

  # Flag to suppress _investigate/inspect_source captures when called
  # from within expect_offense/expect_no_offenses (which call them internally).
  def self.inside_expect?
    Thread.current[:inside_expect] || false
  end

  def self.inside_expect=(val)
    Thread.current[:inside_expect] = val
  end

  def self.flush_pending!
    if (capture = self.pending_capture)
      self.captures << capture
      self.pending_capture = nil
    end
  end

  # Convert a RuboCop offense object to our hash format
  def self.offense_to_hash(offense)
    {
      line: offense.line,
      column_start: offense.column,
      column_end: offense.last_column,
      message: offense.message
    }
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
    TestDataCapture.inside_expect = true

    # Apply format replacements (same as RuboCop does)
    replacements.each do |keyword, value|
      value = value.to_s
      source = source.gsub("%{#{keyword}}", value)
                     .gsub("^{#{keyword}}", '^' * value.size)
                     .gsub("_{#{keyword}}", ' ' * value.size)
    end

    clean_source, offenses = TestDataCapture.parse_annotated_source(source)

    # Resolve abbreviated messages: RuboCop's expect_offense format allows
    # "[...]" as a wildcard suffix to truncate long messages. We need the
    # full messages for our test fixtures, so run the cop via super and
    # use the actual offense messages to replace abbreviated ones.
    if offenses.any? { |o| o[:message].include?('[...]') }
      begin
        actual_offenses = super(source, file, severity: severity, chomp: chomp, **replacements)
        if actual_offenses.is_a?(Array)
          offenses.each do |parsed|
            next unless parsed[:message].include?('[...]')
            match = actual_offenses.find do |actual|
              actual.line == parsed[:line] &&
                actual.column == parsed[:column_start]
            end
            parsed[:message] = match.message if match
          end
        end
      rescue => e
        $stderr.puts "[TestDataCapture] Could not resolve [...] messages: #{e.message}"
      end
    end

    # Extract cop config if available
    config_hash = extract_config_hash

    TestDataCapture.pending_capture = {
      source: clean_source,
      offenses: offenses,
      config: config_hash,
      corrected: nil
    }
  rescue => e
    $stderr.puts "[TestDataCapture] Error in expect_offense: #{e.message}"
  ensure
    TestDataCapture.inside_expect = false
  end

  # Override expect_no_offenses to capture test data
  def expect_no_offenses(source, file = nil)
    TestDataCapture.flush_pending!
    TestDataCapture.inside_expect = true

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
  ensure
    TestDataCapture.inside_expect = false
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

  # Override inspect_source to capture test data from specs that use it
  # instead of expect_offense/expect_no_offenses.
  # Skip capture when called internally by expect_offense/expect_no_offenses.
  def inspect_source(source, file = nil)
    return super if TestDataCapture.inside_expect?

    TestDataCapture.flush_pending!

    result = super
    config_hash = extract_config_hash

    offenses = if result.is_a?(Array)
                 result.map { |o| TestDataCapture.offense_to_hash(o) }
               else
                 []
               end

    TestDataCapture.captures << {
      source: source.to_s.chomp,
      offenses: offenses,
      config: config_hash,
      corrected: nil
    }

    result
  rescue => e
    $stderr.puts "[TestDataCapture] Error in inspect_source: #{e.message}"
    super
  end

  # Override _investigate to capture test data from specs that call it directly.
  # Skip capture when called internally by expect_offense/expect_no_offenses.
  def _investigate(cop_obj, processed_source)
    return super if TestDataCapture.inside_expect?

    TestDataCapture.flush_pending!

    result = super
    config_hash = extract_config_hash

    source = processed_source.respond_to?(:raw_source) ? processed_source.raw_source : processed_source.to_s

    offenses = if result.is_a?(Array)
                 result.map { |o| TestDataCapture.offense_to_hash(o) }
               else
                 []
               end

    TestDataCapture.captures << {
      source: source.to_s.chomp,
      offenses: offenses,
      config: config_hash,
      corrected: nil
    }

    result
  rescue => e
    $stderr.puts "[TestDataCapture] Error in _investigate: #{e.message}"
    super
  end

  private

  # Meta keys that should be filtered out from cop config.
  # These are RuboCop infrastructure keys, not behavioral settings.
  CONFIG_META_KEYS = %w[
    Enabled Description StyleGuide Safe SafeAutoCorrect VersionAdded
    VersionChanged VersionRemoved Reference AutoCorrect Severity
    Details SupportedStyles SupportedEnforcedStyles
  ].freeze

  # Extract the cop config from the fully-resolved cop object.
  # Uses cop.cop_config (the merged config) instead of the cop_config
  # let variable, which may not reflect all config overrides (e.g.,
  # those set via `let(:config)` or `let(:cop_options)`).
  #
  # Also captures cross-cop config entries: when a test sets up config
  # for other cops (e.g., SpaceInsideHashLiteralBraces for SpaceAfterComma),
  # those entries are included under their short name (after the /).
  def extract_config_hash
    result = {}

    # Try the cop object's fully-resolved config first
    if respond_to?(:cop, true)
      cop_obj = cop

      # 1. Primary cop config
      if cop_obj.respond_to?(:cop_config)
        raw = cop_obj.cop_config
        if raw.is_a?(Hash)
          filtered = normalize_config(raw).reject { |k, _| CONFIG_META_KEYS.include?(k) }
          result.merge!(filtered)
        end
      end

      # 2. Cross-cop config: scan the full config for other cop entries
      #    that were explicitly set (e.g., via let(:config) or let(:other_cops)).
      if cop_obj.respond_to?(:config) && cop_obj.config.respond_to?(:keys)
        primary_name = cop_obj.class.respond_to?(:cop_name) ? cop_obj.class.cop_name : nil
        cop_obj.config.keys.each do |key|
          next if key == 'AllCops'
          next if key == primary_name
          next unless key.include?('/') # Only full cop names like "Layout/SpaceInsideHashLiteralBraces"

          begin
            cross_config = cop_obj.config[key]
            if cross_config.is_a?(Hash) && !cross_config.empty?
              short_name = key.split('/').last
              filtered = normalize_config(cross_config).reject { |k, _| CONFIG_META_KEYS.include?(k) }
              result[short_name] = filtered unless filtered.empty?
            end
          rescue => e
            # Skip if config access fails
          end
        end
      end

      return result unless result.empty?
    end

    # Fallback: try the cop_config let variable (older-style tests)
    if respond_to?(:cop_config, true)
      raw_config = cop_config
      if raw_config.is_a?(Hash)
        return normalize_config(raw_config)
      end
    end

    {}
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

        # Capture target ruby version from RSpec shared contexts (e.g., :ruby21, :ruby31).
        # The shared contexts define `let(:ruby_version)` which flows into AllCops.TargetRubyVersion.
        # We only record non-default versions (default is 2.7 in modern RuboCop).
        target_ruby_version = begin
          if respond_to?(:ruby_version, true)
            rv = ruby_version
            rv if rv.is_a?(Numeric)
          end
        rescue => e
          nil
        end

        # Tag all captures from this example with metadata
        TestDataCapture.captures.each do |capture|
          capture[:test_name] ||= test_name
          capture[:cop_name] ||= cop_name
          capture[:ruby_version] ||= target_ruby_version if target_ruby_version
        end
      end
    end
  end
end
