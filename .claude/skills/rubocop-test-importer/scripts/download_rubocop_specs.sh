#!/bin/bash
# Download the full RuboCop repository and install dependencies.
# A full clone is needed because spec files require RuboCop's lib,
# shared contexts, and support helpers for RSpec-based extraction.

set -e

RUBOCOP_DIR="${RUBOCOP_DIR:-/tmp/rubocop-repo}"
RUBOCOP_VERSION="${RUBOCOP_VERSION:-master}"

echo "Downloading RuboCop repo to $RUBOCOP_DIR..."

# Clean up existing directory if it exists
if [ -d "$RUBOCOP_DIR" ]; then
    echo "Removing existing directory..."
    rm -rf "$RUBOCOP_DIR"
fi

# Full shallow clone (needed for bundle install + running specs)
git clone --depth 1 --branch "$RUBOCOP_VERSION" \
    https://github.com/rubocop/rubocop.git "$RUBOCOP_DIR"

cd "$RUBOCOP_DIR"

echo "Installing dependencies with bundler..."
bundle install --jobs 4 --retry 3

# Verify the installation
echo ""
echo "Verifying installation..."
bundle exec ruby -e "require 'rubocop'; puts \"RuboCop #{RuboCop::Version::STRING} loaded successfully\""

# Count spec files
SPEC_COUNT=$(find spec/rubocop/cop -name "*_spec.rb" 2>/dev/null | wc -l | tr -d ' ')

echo ""
echo "RuboCop repo ready at $RUBOCOP_DIR"
echo "Total cop spec files: $SPEC_COUNT"
