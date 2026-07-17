#!/usr/bin/env python3
# File: trace_to_prometheus.py
import time, glob, json, os
from prometheus_client import start_http_server, Gauge, Counter

TRACE_GLOB = "trace_*.json"
POLL_INTERVAL = 5

# Metrics
g_latency = Gauge('ai_trace_latency_ms', 'Latency ms from trace', ['route','provider'])
g_input_tokens = Gauge('ai_trace_input_tokens', 'Input tokens from trace', ['route','provider'])
g_output_tokens = Gauge('ai_trace_output_tokens', 'Output tokens from trace', ['route','provider'])
c_requests = Counter('ai_trace_requests_total', 'Total traces processed', ['route','provider'])
c_errors = Counter('ai_trace_errors_total', 'Total error traces', ['route','provider'])
g_canary = Gauge('ai_trace_canary_passed', 'Canary passed (1 true, 0 false)', ['route','provider'])

def process_trace(path):
    try:
        with open(path,'r') as f:
            t = json.load(f)
    except Exception:
        return
    route = t.get('route','unknown')
    provider = t.get('provider','unknown')
    latency = t.get('latency_ms',0)
    in_t = t.get('input_tokens',0)
    out_t = t.get('output_tokens',0)
    http_status = t.get('http_status',200)
    stop_reason = t.get('stop_reason','')
    response_text = (t.get('response_text') or "").lower()
    canary = 1 if 'paris' in response_text else 0

    g_latency.labels(route,provider).set(latency)
    g_input_tokens.labels(route,provider).set(in_t)
    g_output_tokens.labels(route,provider).set(out_t)
    c_requests.labels(route,provider).inc()
    if http_status != 200 or stop_reason == 'error':
        c_errors.labels(route,provider).inc()
    g_canary.labels(route,provider).set(canary)

def main():
    start_http_server(8001)
    seen = set()
    while True:
        for path in sorted(glob.glob(TRACE_GLOB)):
            if os.path.basename(path) == 'trace_template.json':
                continue
            if path in seen:
                continue
            process_trace(path)
            seen.add(path)
        time.sleep(POLL_INTERVAL)

if __name__ == "__main__":
    main()

