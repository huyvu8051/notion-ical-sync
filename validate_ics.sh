#!/bin/bash
set -euo pipefail
URL="${1:-https://notion-sync.opendiy.vn/cal/4cb38c7656ae483d8ee5650d9fb02108}"
TMP=$(mktemp /tmp/calendar.ics.XXXXXX)
echo "Fetching $URL ..."
if ! curl -sfL "$URL" -o "$TMP"; then
  echo "Fetch failed"
  rm -f "$TMP"
  exit 1
fi
uv run --with icalendar python3 validate_ics.py "$TMP"
rm -f "$TMP"
