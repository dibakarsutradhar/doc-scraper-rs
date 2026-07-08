#!/usr/bin/env bash
set -euo pipefail

URL="${1:-https://docs.strata.markets}"
OUT="$(pwd)/strata-docs"
rm -rf "$OUT"

echo "==> cargo build --release"
( cd "$(dirname "$0")/.." && cargo build --release )

BIN="$(dirname "$0")/../target/release/doc-scraper"
test -x "$BIN"

START=$(date +%s.%N)
"$BIN" "$URL" -o "$OUT" --delay 0.3 --retries 3 --timeout 20 --concurrency 20
END=$(date +%s.%N)

PAGE_COUNT=$(find "$OUT" -name '*.md' ! -name 'index.md' | wc -l | tr -d ' ')
SIZE=$(du -sk "$OUT" | cut -f1)
ELAPSED=$(echo "$END - $START" | bc)
RATE=$(echo "scale=2; $PAGE_COUNT / $ELAPSED" | bc)

echo "Pages (excl. index.md): $PAGE_COUNT"
echo "Total KB:               $SIZE"
echo "Elapsed (s):            $ELAPSED"
echo "Pages/sec:              $RATE"

ls "$OUT" >/dev/null
ls "$OUT/markets/ethena-usde" >/dev/null
test -f "$OUT/index.md"
test -f "$OUT/llms.txt"

# Loose assertions matching the spec
if [ "$PAGE_COUNT" -lt 30 ]; then
  echo "ERROR: expected ~35 pages, got $PAGE_COUNT" >&2
  exit 1
fi
if [ "$SIZE" -lt 100 ]; then
  echo "ERROR: expected ~180 KB, got ${SIZE}KB" >&2
  exit 1
fi

echo "OK: smoke test passed"
