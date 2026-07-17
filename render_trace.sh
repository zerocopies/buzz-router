#!/usr/bin/env bash
set -euo pipefail
REQID=${REQID:-$(uuidgen)}
TS_START=${TS_START:-$(date -u +"%Y-%m-%dT%H:%M:%S.%3NZ")}
ROUTE=${ROUTE:-local}
PROVIDER=${PROVIDER:-Local}
MODEL=${MODEL:-qfz3-local}
INPUT_TOKENS=${INPUT_TOKENS:-0}
OUTPUT_TOKENS=${OUTPUT_TOKENS:-0}
COST_USD=${COST_USD:-0.0}
LATENCY_MS=${LATENCY_MS:-0}
STEP_SPANS=${STEP_SPANS:-'[]'}
RETRIEVAL_HITS=${RETRIEVAL_HITS:-0}
RETRIEVAL_SCORE_AVG=${RETRIEVAL_SCORE_AVG:-null}
RESPONSE_TEXT=${RESPONSE_TEXT:-"\"\""}
STOP_REASON=${STOP_REASON:-""}
ERROR_CODE=${ERROR_CODE:-""}
WARNINGS=${WARNINGS:-'[]'}
REQUEST_PAYLOAD=${REQUEST_PAYLOAD:-'{}'}
RESPONSE_PAYLOAD=${RESPONSE_PAYLOAD:-'{}'}
HTTP_STATUS=${HTTP_STATUS:-200}
CORRELATION_ID=${CORRELATION_ID:-""}
USER_ID=${USER_ID:-""}

# produce file by replacing placeholders
sed \
  -e "s/{{REQUEST_ID}}/${REQID}/g" \
  -e "s/{{TIMESTAMP_START}}/${TS_START}/g" \
  -e "s/{{ROUTE}}/${ROUTE}/g" \
  -e "s/{{PROVIDER}}/${PROVIDER}/g" \
  -e "s/{{MODEL}}/${MODEL}/g" \
  -e "s/{{INPUT_TOKENS}}/${INPUT_TOKENS}/g" \
  -e "s/{{OUTPUT_TOKENS}}/${OUTPUT_TOKENS}/g" \
  -e "s/{{COST_USD}}/${COST_USD}/g" \
  -e "s/{{LATENCY_MS}}/${LATENCY_MS}/g" \
  -e "s/{{STEP_SPANS}}/${STEP_SPANS}/g" \
  -e "s/{{RETRIEVAL_HITS}}/${RETRIEVAL_HITS}/g" \
  -e "s/{{RETRIEVAL_SCORE_AVG}}/${RETRIEVAL_SCORE_AVG}/g" \
  -e "s/{{RESPONSE_TEXT}}/${RESPONSE_TEXT}/g" \
  -e "s/{{STOP_REASON}}/${STOP_REASON}/g" \
  -e "s/{{ERROR_CODE}}/${ERROR_CODE}/g" \
  -e "s/{{WARNINGS}}/${WARNINGS}/g" \
  -e "s/{{REQUEST_PAYLOAD}}/${REQUEST_PAYLOAD}/g" \
  -e "s/{{RESPONSE_PAYLOAD}}/${RESPONSE_PAYLOAD}/g" \
  -e "s/{{HTTP_STATUS}}/${HTTP_STATUS}/g" \
  -e "s/{{CORRELATION_ID}}/${CORRELATION_ID}/g" \
  -e "s/{{USER_ID}}/${USER_ID}/g" \
  trace_template.json > trace_${REQID}.json

echo "Wrote trace_${REQID}.json"
