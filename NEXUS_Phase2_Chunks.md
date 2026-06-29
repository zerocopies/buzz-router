# NEXUS — Phase 2 Chunk Prompts
**Phase 2 Goal:** First real agency client in recommend-only mode.
Google Ads Agent, WhatsApp Agent, agency dashboard backend, 
human-in-loop approval flow, and Phase 2 gate test.

**Phase 1 Status:** PASSED ✔
**Phase 2 Gate Condition:** Agency dashboard runs, all 4 agents active,
human approval flow works, recommend-only mode enforced.

---

## PHASE 2 FILE MAP

```
nexus/
├── core/
│   └── approval_engine.py       ← NEW: human-in-loop approval system
├── agents/
│   ├── google_agent.py          ← NEW: Google Ads API integration
│   └── whatsapp_agent.py        ← NEW: WhatsApp Business API integration
├── dashboard/
│   ├── __init__.py              ← NEW
│   ├── api.py                   ← NEW: FastAPI dashboard backend
│   ├── models.py                ← NEW: dashboard data models
│   └── routes/
│       ├── __init__.py          ← NEW
│       ├── clients.py           ← NEW: client management endpoints
│       ├── decisions.py         ← NEW: decision queue endpoints
│       └── analytics.py        ← NEW: performance analytics endpoints
└── tests/
    └── test_phase2.py           ← NEW: Phase 2 gate test
```

---

## CHUNK 13 — Google Ads Agent

### Prompt:

```
You are building the Google Ads Agent for NEXUS — a goal-aware multi-agent
marketing coordination system. Phase 1 is complete with Meta and LinkedIn agents.

CONTEXT:
- BaseAgent is in agents/base_agent.py (same pattern as MetaAgent and LinkedInAgent)
- Google Ads API base URL: https://googleads.googleapis.com/v14
- Simulation mode when GOOGLE_ADS_DEVELOPER_TOKEN is not set in .env
- Google Ads focuses on search intent signals — different from social platforms
- Key metrics: CPC (cost per click), conversion rate, quality score, impression share

FILE: agents/google_agent.py
Build class GoogleAgent that inherits from BaseAgent.

PLATFORM PROPERTIES:
- platform_name = "google"
- api_version = "v14"
- supported_signal_types = ["cpc_rising", "conversion_rate_drop", "quality_score_drop",
  "budget_exhaustion", "impression_share_loss", "keyword_irrelevance"]

__init__:
- Reads GOOGLE_ADS_DEVELOPER_TOKEN and GOOGLE_ADS_CUSTOMER_ID from environment
- simulation_mode = True if no token
- self._keyword_cache: Dict[str, Any] = {}
- self._campaign_cache: Dict[str, Any] = {}

fetch_platform_data() -> dict:
  Simulation mode:
    Seed with int(time.time() / 300) for realistic variation
    Return:
    - current_kpi: float — CPC between 2.0-25.0 AED
    - spend_to_date: float
    - daily_burn_rate: float
    - pacing: "ahead" | "on_track" | "behind"
    - performance_status: derived from CPC vs goal thresholds
    - impressions: int
    - clicks: int
    - ctr: float (2-8% typical for search)
    - conversions: int
    - conversion_rate: float (0.02-0.15)
    - avg_quality_score: float (1-10, Google specific)
    - impression_share: float (0.1-0.9)
    - top_keywords: List[str] (3 dummy keywords relevant to dental/health)
    - lost_impression_share_budget: float (0-0.4)

  Real mode:
    Use Google Ads API v14 via requests with developer token header
    GET campaigns and their metrics for today
    Parse clicks, impressions, cost, conversions
    Calculate CPC = cost / clicks
    Handle errors with cache fallback

_assess_performance_status(data, goal) -> str:
  Google uses CPC not CPL — adjust logic:
  - "critical" if CPC > goal.kpi_threshold_critical OR conversion_rate < 0.02
  - "warning" if CPC > goal.kpi_threshold_warning OR conversion_rate < 0.04
  - "excellent" if CPC < goal.kpi_target * 0.8 AND conversion_rate > 0.10
  - "stable" otherwise

execute_action(action: str) -> bool:
  Simulation: log and return True
  Real mode:
  - "pause_keyword" — pause underperforming keywords
  - "increase_bid" — increase CPC bid by 10%
  - "decrease_bid" — decrease CPC bid by 15%
  - "pause_campaign" — pause entire campaign

get_search_intent_summary() -> str:
  Returns plain English summary of search intent signals.
  Includes: top keywords, quality scores, impression share, conversion rate.

Include standalone test at bottom — simulation mode, print results, print PASSED.
```

---

## CHUNK 14 — WhatsApp Agent

### Prompt:

```
You are building the WhatsApp Agent for NEXUS. Phase 1 complete.

CONTEXT:
- BaseAgent in agents/base_agent.py
- WhatsApp Business API (Cloud API) base URL: https://graph.facebook.com/v18.0
- Simulation mode when WHATSAPP_ACCESS_TOKEN not set
- WhatsApp is a response/conversion channel — measures lead quality after ad click
- Key metrics: response rate, conversation-to-appointment rate, sentiment, response time

FILE: agents/whatsapp_agent.py
Build class WhatsAppAgent that inherits from BaseAgent.

PLATFORM PROPERTIES:
- platform_name = "whatsapp"
- api_version = "v18.0"
- supported_signal_types = ["response_rate_drop", "sentiment_negative",
  "conversion_drop", "response_time_spike", "message_volume_spike"]

__init__:
- Reads WHATSAPP_ACCESS_TOKEN and WHATSAPP_PHONE_NUMBER_ID from environment
- simulation_mode = True if no token
- self._conversation_cache: List[Dict] = []
- self._sentiment_history: List[float] = []

fetch_platform_data() -> dict:
  Simulation mode:
    Seed with int(time.time() / 300)
    Return:
    - current_kpi: float — response rate 0.2-0.95 (this is the primary KPI)
    - spend_to_date: float (0 — WhatsApp is organic/free response channel)
    - daily_burn_rate: float (0)
    - pacing: "on_track" always (WhatsApp has no budget)
    - performance_status: derived from response rate and sentiment
    - messages_received_today: int
    - messages_responded_today: int
    - avg_response_time_minutes: float (2-45 minutes)
    - sentiment_score: float (0.0-1.0, 1.0 = very positive)
    - conversations_opened: int
    - appointments_booked: int
    - conversion_rate: float (appointments / conversations)
    - top_inquiry_topics: List[str] (3 dummy topics for dental clinic)
    - unread_count: int

  Real mode:
    WhatsApp Cloud API — GET message analytics
    Parse message counts, response times
    Note: sentiment analysis requires separate NLP call
    Handle API errors gracefully

_assess_performance_status(data, goal) -> str:
  WhatsApp uses response rate and sentiment — not cost:
  - "critical" if response_rate < 0.3 OR sentiment_score < 0.3
  - "warning" if response_rate < 0.5 OR sentiment_score < 0.5
  - "excellent" if response_rate > 0.85 AND sentiment_score > 0.8
  - "stable" otherwise

  Note: Override run_cycle() to use response_rate as current_kpi
  and set severity based on sentiment + response rate combined.

execute_action(action: str) -> bool:
  Simulation: log and return True
  Real mode:
  - "send_template" — send a follow-up template message to unresponsive leads
  - "flag_negative_sentiment" — flag conversation for human review
  - "escalate_to_human" — mark conversation for immediate human takeover

get_conversation_summary() -> str:
  Returns plain English summary of WhatsApp channel health.
  Includes: response rate, sentiment trend, top inquiry topics,
  appointments booked today.

Include standalone test at bottom — simulation mode, print PASSED.
```

---

## CHUNK 15 — Approval Engine

### Prompt:

```
You are building the human-in-loop approval engine for NEXUS.
This is the most important safety component in Phase 2.

CONTEXT:
- OrchestratorDecision has: decision_id, client_id, decision_type, action,
  affected_platforms, confidence_score, reasoning, requires_human_approval
- Phase 2 default is RECOMMEND-ONLY mode for all new clients
- Agencies must approve/reject every recommendation before it executes
- Three operating modes: RECOMMEND_ONLY, THRESHOLD_AUTO, FULL_AUTONOMOUS

FILE: core/approval_engine.py

Build an ApprovalEngine class with these components:

1. Enum OperatingMode:
   RECOMMEND_ONLY = "recommend_only"    — every action needs approval
   THRESHOLD_AUTO = "threshold_auto"    — auto-approve below spend threshold
   FULL_AUTONOMOUS = "full_autonomous"  — act within guardrails only

2. Pydantic model ApprovalRequest:
   - request_id: str (auto UUID)
   - decision_id: str
   - client_id: str
   - agency_id: str
   - action: str
   - reasoning: str
   - affected_platforms: List[str]
   - confidence_score: float
   - estimated_budget_impact_aed: float
   - created_at: datetime
   - expires_at: datetime (default: created_at + 24 hours)
   - status: str = "pending"  — "pending"|"approved"|"rejected"|"expired"
   - reviewed_by: Optional[str] = None
   - reviewed_at: Optional[datetime] = None
   - rejection_reason: Optional[str] = None

3. ApprovalEngine class:
   __init__(self, db: NexusDatabase):
     self._db = db
     self._lock = threading.RLock()
     self._pending_queue: Dict[str, ApprovalRequest] = {}
     self._operating_modes: Dict[str, OperatingMode] = {}
     self._auto_approve_threshold_aed: Dict[str, float] = {}
     Default mode for all new clients: RECOMMEND_ONLY

   set_operating_mode(client_id, agency_id, mode, threshold_aed=500.0):
     Sets mode for a client. Logs the change.

   get_operating_mode(client_id) -> OperatingMode:
     Returns current mode. Defaults to RECOMMEND_ONLY if not set.

   submit_for_approval(decision, agency_id, estimated_budget_impact) -> ApprovalRequest:
     Creates ApprovalRequest from OrchestratorDecision.
     Adds to pending queue and saves to database.
     Returns the request.

   should_auto_approve(request: ApprovalRequest) -> bool:
     Returns True ONLY if:
     - Mode is THRESHOLD_AUTO AND
     - estimated_budget_impact_aed <= threshold AND
     - decision confidence_score >= 0.75 AND
     - NOT a budget reallocation action
     Returns False for RECOMMEND_ONLY always.
     Returns True for FULL_AUTONOMOUS always (within guardrails).

   approve(request_id, reviewed_by) -> bool:
     Sets status to approved, records reviewer and timestamp.
     Removes from pending queue.
     Logs approval.

   reject(request_id, reviewed_by, reason) -> bool:
     Sets status to rejected with reason.
     Removes from pending queue.
     Logs rejection.

   get_pending_queue(agency_id) -> List[ApprovalRequest]:
     Returns all pending requests for this agency, sorted by created_at.

   expire_old_requests() -> int:
     Marks all requests past expires_at as expired.
     Returns count of expired requests.
     Call this on startup and periodically.

   get_approval_stats(agency_id, days=30) -> dict:
     Returns: total_submitted, approved, rejected, expired, 
     auto_approved, avg_response_time_hours

   HARD RULES (enforce in submit_for_approval):
   - Any action with "budget" in it AND impact > 1000 AED: always RECOMMEND_ONLY
     regardless of operating mode
   - Any action with "pause all" or affecting 3+ platforms: always RECOMMEND_ONLY
   - Log every override of auto-approve to audit trail

Include full test at bottom — create engine, submit request,
approve it, check stats, print PASSED.
```

---

## CHUNK 16 — Dashboard Backend API

### Prompt:

```
You are building the agency dashboard API backend for NEXUS.
This is what agencies see and interact with — the human face of the system.

CONTEXT:
- FastAPI framework
- ApprovalEngine in core/approval_engine.py
- NexusDatabase in core/database.py
- GoalEngine in core/goal_engine.py
- DecisionLog in core/decision_log.py
- All agents: MetaAgent, LinkedInAgent, GoogleAgent, WhatsAppAgent
- Recommend-only mode by default

FILE 1: dashboard/models.py
Pydantic models for API requests and responses:

- ClientProfile: client_id, agency_id, business_name, industry, 
  active_platforms, monthly_budget_aed, operating_mode, created_at
- AgencyDashboardSummary: agency_id, total_clients, active_alerts,
  pending_approvals, total_spend_today_aed, top_performing_client,
  worst_performing_client
- DecisionQueueItem: request_id, client_id, business_name, action,
  reasoning, confidence_score, estimated_impact_aed, created_at, 
  expires_at, affected_platforms
- ClientPerformanceSnapshot: client_id, business_name, 
  per_platform_kpi (dict), overall_status, spend_today_aed,
  decisions_today, last_updated
- ApprovalAction: request_id, reviewed_by, action ("approve"|"reject"),
  rejection_reason (optional)

FILE 2: dashboard/api.py
FastAPI application with these endpoints:

POST /agency/{agency_id}/clients
  Register a new client under this agency.
  Body: ClientProfile
  Returns: ClientProfile with created_at

GET /agency/{agency_id}/dashboard
  Returns AgencyDashboardSummary for the agency.
  Pulls from database — all clients, pending approvals, today's spend.

GET /agency/{agency_id}/clients
  Returns list of all ClientPerformanceSnapshot for this agency.

GET /agency/{agency_id}/decisions/pending
  Returns list of DecisionQueueItem — all pending approvals.
  Sorted by created_at ascending (oldest first — needs action soonest).

POST /agency/{agency_id}/decisions/action
  Approve or reject a decision.
  Body: ApprovalAction
  Returns: updated ApprovalRequest

GET /agency/{agency_id}/clients/{client_id}/history
  Returns last 50 decisions for a specific client.
  Query param: limit (default 50, max 200)

GET /agency/{agency_id}/clients/{client_id}/audit
  Returns audit trail for client.
  Query params: start_date, end_date (ISO format)
  Calls DecisionLog.generate_audit_trail()

GET /health
  Returns: {"status": "healthy", "version": "2.0", "phase": "2"}

SETUP:
- app = FastAPI(title="NEXUS Agency Dashboard", version="2.0")
- CORS middleware: allow all origins for Phase 2 (tighten in Phase 3)
- Dependency injection: get_db(), get_approval_engine(), get_goal_engine()
- All endpoints return proper HTTP status codes
- All errors return {"detail": "error message"} format

FILE 3: dashboard/__init__.py
Empty init file.

FILE 4: dashboard/routes/__init__.py  
Empty init file.

Include startup test at bottom of api.py:
  if __name__ == "__main__":
    import uvicorn
    uvicorn.run(app, host="0.0.0.0", port=8000)
```

---

## CHUNK 17 — Update requirements + .env

### Prompt:

```
You are updating the NEXUS configuration for Phase 2.
Two simple files need updating.

FILE 1: requirements.txt
Add these to the existing Phase 1 requirements.txt:
  fastapi>=0.104.0
  uvicorn>=0.24.0
  httpx>=0.25.0        (for async HTTP in tests)

Keep all existing Phase 1 requirements intact.

FILE 2: .env.example
Add these new sections to the existing .env.example:

# ─── GOOGLE ADS (optional — simulation if not set) ───
# Get from: https://developers.google.com/google-ads/api/docs/get-started/introduction
GOOGLE_ADS_DEVELOPER_TOKEN=
GOOGLE_ADS_CUSTOMER_ID=

# ─── WHATSAPP BUSINESS (optional — simulation if not set) ───
# Get from: https://developers.facebook.com/docs/whatsapp/cloud-api
WHATSAPP_ACCESS_TOKEN=
WHATSAPP_PHONE_NUMBER_ID=

# ─── DASHBOARD ───────────────────────────────────────
DASHBOARD_PORT=8000
DASHBOARD_HOST=0.0.0.0

Keep all existing Phase 1 variables intact.

Also update main.py to add:
  --dashboard flag that starts the FastAPI server:
  python main.py --dashboard
  
  Which runs:
  import uvicorn
  from dashboard.api import app
  uvicorn.run(app, host=os.getenv("DASHBOARD_HOST","0.0.0.0"),
              port=int(os.getenv("DASHBOARD_PORT", 8000)))
```

---

## CHUNK 18 — Phase 2 Gate Test

### Prompt:

```
You are building the Phase 2 gate test for NEXUS.
Phase 2 gate condition: All 4 agents active, agency dashboard running,
recommend-only mode enforced, human approval flow works end to end.

CONTEXT:
- All 4 agents: MetaAgent, LinkedInAgent, GoogleAgent, WhatsAppAgent
- ApprovalEngine in core/approval_engine.py
- Dashboard API in dashboard/api.py
- NexusDatabase in core/database.py
- Same mock injection pattern as Phase 0 and Phase 1
- Mock injection: gateway.create_message = handler

FILE: tests/test_phase2.py

IMPORTS:
from core.goal_engine import GoalEngine, BusinessGoal
from core.context_manager import ContextManager
from core.orchestrator import Orchestrator
from core.decision_log import DecisionLog
from core.rate_limiter import RateLimiter
from core.approval_engine import ApprovalEngine, OperatingMode
from core.database import NexusDatabase
from agents.meta_agent import MetaAgent
from agents.linkedin_agent import LinkedInAgent
from agents.google_agent import GoogleAgent
from agents.whatsapp_agent import WhatsAppAgent
from core.llm_gateway import LLMResponse
from fastapi.testclient import TestClient
from dashboard.api import app

Same mock handler pattern as Phase 1 — 
inject into goal_engine.gateway, orchestrator.gateway, decision_log.gateway.

10 STEPS:

STEP 1: Initialize all components including ApprovalEngine and TestClient
  db = NexusDatabase()
  approval_engine = ApprovalEngine(db)
  client = TestClient(app)

STEP 2: Set business goal — same Dubai Smile Dental as before
  active_platforms: ["meta", "linkedin", "google", "whatsapp"]
  platform_budget_allocation: {"meta": 40.0, "linkedin": 30.0, 
                                "google": 20.0, "whatsapp": 10.0}

STEP 3: Initialize all 4 agents in simulation mode
  Assert all 4 are in simulation mode.
  Print "[SUCCESS] All 4 agents in simulation mode"

STEP 4: Verify recommend-only mode is default
  mode = approval_engine.get_operating_mode(client_id)
  Assert mode == OperatingMode.RECOMMEND_ONLY
  Print "[GATE ✔] Default mode: RECOMMEND_ONLY confirmed"

STEP 5: Run 4-agent coordination cycle
  Force Meta to CRITICAL state (same as Phase 1)
  Force LinkedIn to STABLE state
  Force Google to WARNING state (CPC 22 AED, threshold 20 AED)
  Force WhatsApp to STABLE state
  decisions = orchestrator.coordinate_agents(
    agents=[meta_agent, linkedin_agent, google_agent, whatsapp_agent],
    goal=goal
  )
  Print number of decisions.

STEP 6: Submit critical decisions for approval
  For each decision that requires_human_approval:
    request = approval_engine.submit_for_approval(
      decision=decision,
      agency_id="test_agency_01",
      estimated_budget_impact=500.0
    )
  pending = approval_engine.get_pending_queue("test_agency_01")
  Assert len(pending) >= 1
  Print f"[GATE ✔] {len(pending)} decision(s) queued for human approval"

STEP 7: Test dashboard API endpoint
  response = client.get("/agency/test_agency_01/decisions/pending")
  Assert response.status_code == 200
  Print "[GATE ✔] Dashboard API: pending decisions endpoint working"

STEP 8: Approve one decision via API
  first_request = pending[0]
  response = client.post("/agency/test_agency_01/decisions/action", json={
    "request_id": first_request.request_id,
    "reviewed_by": "agency_manager_01",
    "action": "approve"
  })
  Assert response.status_code == 200
  Print "[GATE ✔] Human approval flow: decision approved via API"

STEP 9: Verify audit trail covers all platforms
  audit = decision_log.generate_audit_trail(client_id, start-1hr, end+1hr)
  Assert audit is not empty
  Print "[SUCCESS] Audit trail generated"

STEP 10: Health check
  response = client.get("/health")
  Assert response.status_code == 200
  Assert response.json()["phase"] == "2"
  Print "[GATE ✔] Dashboard health check passed"

RESULT: PHASE 2 GATE TEST: PASSED / FAILED
Cleanup in finally block.

Also update main.py --test-phase2 flag.
```

---

## PHASE 2 SUMMARY

| Chunk | Files | Gate Condition |
|---|---|---|
| 13 | `agents/google_agent.py` | Simulation test passes |
| 14 | `agents/whatsapp_agent.py` | Simulation test passes |
| 15 | `core/approval_engine.py` | Approval flow test passes |
| 16 | `dashboard/api.py` + models | API endpoints respond correctly |
| 17 | `requirements.txt` + `.env.example` + `main.py` | Dashboard starts |
| 18 | `tests/test_phase2.py` | PHASE 2 GATE TEST: PASSED |

---

## PHASE 2 ENV ADDITIONS

```
GOOGLE_ADS_DEVELOPER_TOKEN=   (optional — simulation if blank)
GOOGLE_ADS_CUSTOMER_ID=       (optional)
WHATSAPP_ACCESS_TOKEN=        (optional — simulation if blank)
WHATSAPP_PHONE_NUMBER_ID=     (optional)
DASHBOARD_PORT=8000
```

---

*NEXUS Phase 2 — Four agents. One dashboard. First agency client.*
