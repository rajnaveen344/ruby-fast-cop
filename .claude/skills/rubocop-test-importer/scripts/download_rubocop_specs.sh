#!/bin/bash
# Download RuboCop spec files using sparse checkout
# This only downloads the spec/rubocop/cop directory to minimize bandwidth

set -e

RUBOCOP_DIR="${RUBOCOP_DIR:-/tmp/rubocop-specs}"
RUBOCOP_VERSION="${RUBOCOP_VERSION:-master}"

echo "Downloading RuboCop specs to $RUBOCOP_DIR..."

# Clean up existing directory if it exists
if [ -d "$RUBOCOP_DIR" ]; then
    echo "Removing existing directory..."
    rm -rf "$RUBOCOP_DIR"
fi

# Sparse checkout just the spec/rubocop/cop directory
git clone --depth 1 --filter=blob:none --sparse \
    --branch "$RUBOCOP_VERSION" \
    https://github.com/rubocop/rubocop.git "$RUBOCOP_DIR"

cd "$RUBOCOP_DIR"
git sparse-checkout set spec/rubocop/cop

# Count downloaded spec files
SPEC_COUNT=$(find spec/rubocop/cop -name "*_spec.rb" 2>/dev/null | wc -l | tr -d ' ')

echo ""
echo "Downloaded specs to $RUBOCOP_DIR/spec/rubocop/cop"
echo "Total spec files: $SPEC_COUNT"
