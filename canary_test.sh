#!/usr/bin/env bash
set -euo pipefail
ROUTE=${1:-local}
PAYLOAD='{"prompt":"What is the capital of France? Answer in one short sentence.","max_tokens":40}'
./call_and_trace.sh "$ROUTE" "$PAYLOAD"
# find latest trace file
TRACE=$(ls -1 trace_*.json | tail -n1)
if jq -r '.response_text' "$TRACE" | grep -qi "paris"; then
  echo "Canary OK for $ROUTE"
  exit 0
else
  echo "Canary FAILED for $ROUTE"
  exit 4
fi
