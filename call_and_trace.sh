#!/usr/bin/env bash
set -euo pipefail
if [ $# -lt 2 ]; then
  echo "Usage: $0 <route> '<json_payload>'"
  exit 2
fi
ROUTE="$1"
PAYLOAD="$2"
REQID=$(uuidgen)
TS_START=$(date -u +"%Y-%m-%dT%H:%M:%S.%3NZ")
URL="http://127.0.0.1:7474/chat/${ROUTE}"
OUT="/tmp/resp_${REQID}.json"
META="/tmp/meta_${REQID}.txt"

curl -s -w "\n__CURL_META__\nhttp_code:%{http_code}\ntime_total:%{time_total}\n" \
  -X POST "$URL" \
  -H "Content-Type: application/json" \
  -H "X-Request-Id: $REQID" \
  -d "$PAYLOAD" \
  -o "$OUT" > "$META"

HTTP_CODE=$(grep -oP '(?<=http_code:)\d+' "$META" || echo 0)
TIME_TOTAL=$(grep -oP '(?<=time_total:)[0-9.]+$' "$META" || echo 0)
LAT_MS=$(awk -v t="$TIME_TOTAL" 'BEGIN{printf("%d", t*1000)}')

# attempt to extract tokens and cost if present in response JSON
INPUT_TOKENS=$(jq -r '.input_tokens // 0' "$OUT" 2>/dev/null || echo 0)
OUTPUT_TOKENS=$(jq -r '.output_tokens // 0' "$OUT" 2>/dev/null || echo 0)
COST_USD=$(jq -r '.cost_incurred // 0.0' "$OUT" 2>/dev/null || echo 0.0)
RESPONSE_TEXT=$(jq -r '.output // .response_text // ""' "$OUT" 2>/dev/null || echo "")

# export env for render_trace
export REQID TS_START ROUTE PROVIDER="Local" MODEL="qfz3-local" \
       INPUT_TOKENS OUTPUT_TOKENS COST_USD LATENCY_MS="$LAT_MS" \
       STEP_SPANS='[]' RETRIEVAL_HITS=0 RETRIEVAL_SCORE_AVG=null \
       RESPONSE_TEXT="$(echo "$RESPONSE_TEXT" | sed 's/"/\\"/g')" \
       STOP_REASON="$(jq -r '.stop_reason // ""' "$OUT" 2>/dev/null || echo "")" \
       ERROR_CODE="$(jq -r '.error_code // ""' "$OUT" 2>/dev/null || echo "")" \
       WARNINGS='[]' REQUEST_PAYLOAD="$(echo "$PAYLOAD" | sed 's/"/\\"/g')" \
       RESPONSE_PAYLOAD="$(cat "$OUT" | sed 's/"/\\"/g')" HTTP_STATUS="$HTTP_CODE" \
       CORRELATION_ID="$(jq -r '.correlation_id // ""' "$OUT" 2>/dev/null || echo "")" \
       USER_ID=""

./render_trace.sh
echo "Call complete, response saved to $OUT"
