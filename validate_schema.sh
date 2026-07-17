#!/usr/bin/env bash
set -euo pipefail
FILE=${1:-}
if [ -z "$FILE" ] || [ ! -f "$FILE" ]; then
  echo "Usage: $0 <response_json_file>"
  exit 2
fi

# required fields list
MISSING=$(jq -e '[
  "request_id","timestamp_utc","route","provider","model",
  "input_tokens","output_tokens","cost_usd","latency_ms",
  "response_text","stop_reason","request_payload","response_payload","http_status"
] as $req | ($req - (keys[])) | length' "$FILE" 2>/dev/null || echo 0)

# simpler check: ensure keys exist
for k in request_id timestamp_utc route provider model input_tokens output_tokens cost_usd latency_ms response_text stop_reason request_payload response_payload http_status; do
  if ! jq -e "has(\"$k\")" "$FILE" >/dev/null 2>&1; then
    echo "Schema validation failed: missing $k"
    exit 3
  fi
done

echo "Schema validation passed"
