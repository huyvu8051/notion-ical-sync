#!/bin/bash
set -euo pipefail

ENDPOINT="${1:-https://notion-sync.opendiy.vn/cal/4cb38c7656ae483d8ee5650d9fb02108}"
TMP=$(mktemp)

echo "Fetching $ENDPOINT ..."
if ! curl -sfL "$ENDPOINT" -o "$TMP"; then
  echo "Fetch failed"
  rm -f "$TMP"
  exit 1
fi

echo "Validating with ical-validator ..."
if command -v ical-validator >/dev/null 2>&1; then
  ical-validator "$TMP"
  echo "Validation passed"
else
  echo "ical-validator not installed; install with: brew install ical-validator"
  exit 1
fi

rm -f "$TMP"
