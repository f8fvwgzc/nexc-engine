#!/usr/bin/env bash
# End-to-end smoke test against a running stack (local `make dev`, Docker Compose or minikube).
# Walks the whole product flow: register -> graph from template -> plan (SSE) -> apply -> run ->
# artifacts. Works in demo mode, so no API key is needed.
#
#   scripts/smoke.sh [base_url]        # default http://localhost:8080
#
# With a real LLM provider, raise the limits: SMOKE_PLAN_TIMEOUT / SMOKE_RUN_TIMEOUT (seconds,
# default 120 / 300). SMOKE_TEMPLATE picks the starter graph (default research-report-docx) and
# SMOKE_TOPIC what it is about.
set -euo pipefail

BASE="${1:-http://localhost:8080}"
TEMPLATE="${SMOKE_TEMPLATE:-research-report-docx}"
TOPIC="${SMOKE_TOPIC:-The state of solid-state batteries for electric vehicles in 2026}"
PLAN_TIMEOUT="${SMOKE_PLAN_TIMEOUT:-120}"
RUN_TIMEOUT="${SMOKE_RUN_TIMEOUT:-300}"
API="$BASE/api/v1"
for tool in curl jq; do
  command -v "$tool" >/dev/null 2>&1 || { echo "error: '$tool' is required" >&2; exit 1; }
done

WORK="$(mktemp -d)"
SSE_PID=""
cleanup() {
  if [[ -n "$SSE_PID" ]]; then
    kill "$SSE_PID" 2>/dev/null || true
    wait "$SSE_PID" 2>/dev/null || true
  fi
  rm -rf "$WORK"
}
trap cleanup EXIT

step() { printf '\033[36m==>\033[0m %s\n' "$*"; }
fail() { printf '\033[31mFAIL:\033[0m %s\n' "$*" >&2; exit 1; }

# Logs in again (access tokens live 15 minutes; real-LLM runs can take longer).
login() {
  TOKEN="$(curl -sSf -H 'Content-Type: application/json' \
    --data "$(jq -n --arg e "$EMAIL" --arg p "$PASSWORD" '{email:$e,password:$p}')" \
    "$API/auth/login" | jq -r .access_token)"
}

# api METHOD PATH [JSON]  -> prints body; fails on non-2xx (re-authenticates once on 401)
api() {
  local method="$1" path="$2" body="${3:-}" code attempt
  for attempt in 1 2; do
    local args=(-sS -o "$WORK/body" -w '%{http_code}' -X "$method" -H "Authorization: Bearer $TOKEN")
    [[ -n "$body" ]] && args+=(-H 'Content-Type: application/json' --data "$body")
    code="$(curl "${args[@]}" "$API$path")"
    [[ "$code" == 401 && "$attempt" == 1 ]] || break
    login
  done
  [[ "$code" == 2* ]] || fail "$method $path -> HTTP $code: $(cat "$WORK/body")"
  cat "$WORK/body"
}

step "health"
curl -sf "$API/readyz" >/dev/null || fail "backend not ready at $API/readyz"

step "register a throwaway user"
EMAIL="smoke-$(date +%s)-$RANDOM@nexc.test"
PASSWORD="$(openssl rand -hex 12)Aa1!"
TOKEN="$(curl -sSf -H 'Content-Type: application/json' \
  --data "$(jq -n --arg e "$EMAIL" --arg p "$PASSWORD" '{email:$e,password:$p,name:"Smoke Test"}')" \
  "$API/auth/register" | jq -r .access_token)"
[[ -n "$TOKEN" && "$TOKEN" != null ]] || fail "no access token"

step "orchestrator status"
api GET /orchestrator/status | jq -c '{demo_mode, backends: (.backends | map_values(.ok))}'

step "create a graph from the $TEMPLATE template"
GRAPH="$(api POST /graphs/from-template "$(jq -n --arg t "$TEMPLATE" --arg topic "$TOPIC" '{template_id:$t,topic:$topic}')")"
GID="$(jq -r .id <<<"$GRAPH")"
echo "graph $GID: $(jq '.nodes | length' <<<"$GRAPH") nodes, $(jq '.edges | length' <<<"$GRAPH") edges"

step "open the SSE stream"
TICKET="$(api POST /realtime/tickets "{\"graph_id\":\"$GID\"}" | jq -r .ticket)"
curl -sN "$API/graphs/$GID/events?ticket=$TICKET" >"$WORK/sse" &
SSE_PID=$!
sleep 1

step "request a plan"
PID="$(api POST "/graphs/$GID/plan" '{"instructions":"Keep it concise."}' | jq -r .id)"
for _ in $(seq 1 "$PLAN_TIMEOUT"); do
  STATUS="$(api GET "/graphs/$GID/plans/$PID" | jq -r .status)"
  [[ "$STATUS" == streaming ]] || break
  sleep 1
done
[[ "$STATUS" == ready ]] || fail "plan ended as $STATUS"
api GET "/graphs/$GID/plans/$PID" | jq -c '{summary: .summary[0:80], nodes: (.nodes|length), edges: (.edges|length)}'

step "apply the plan"
api POST "/graphs/$GID/plans/$PID/apply" | jq -c '{nodes: (.nodes|length), edges: (.edges|length)}'
api GET "/graphs/$GID/analysis" | jq -c '{levels: (.levels|length), critical_path: (.critical_path|length), cycles: (.cycles|length)}'

step "run the graph"
RID="$(api POST "/graphs/$GID/runs" '{}' | jq -r .id)"
for _ in $(seq 1 "$RUN_TIMEOUT"); do
  RUN="$(api GET "/runs/$RID")"
  case "$(jq -r .status <<<"$RUN")" in queued|running) sleep 1 ;; *) break ;; esac
done
jq -c '{status, tokens_in, tokens_out, cost_usd, nodes: [.node_runs[] | .status] | group_by(.) | map({(.[0]): length}) | add}' <<<"$RUN"
[[ "$(jq -r .status <<<"$RUN")" == succeeded ]] || fail "run did not succeed: $(jq -c '[.node_runs[] | select(.error) | .error]' <<<"$RUN")"

step "re-run hits the result cache"
RID2="$(api POST "/graphs/$GID/runs" '{}' | jq -r .id)"
for _ in $(seq 1 "$RUN_TIMEOUT"); do
  RUN2="$(api GET "/runs/$RID2")"
  case "$(jq -r .status <<<"$RUN2")" in queued|running) sleep 1 ;; *) break ;; esac
done
jq -c '{status, cached: ([.node_runs[] | select(.cached)] | length), total: (.node_runs | length)}' <<<"$RUN2"

step "artifacts"
ARTIFACTS="$(api GET "/runs/$RID/artifacts")"
jq -r '.[] | "  \(.path)  \(.mime)  \(.size) bytes"' <<<"$ARTIFACTS"
[[ "$(jq length <<<"$ARTIFACTS")" -gt 0 ]] || fail "no artifacts produced"
login # long real-LLM runs outlive the 15-minute access token
curl -sSf -H "Authorization: Bearer $TOKEN" -o "$WORK/artifacts.zip" "$API/runs/$RID/artifacts.zip"
[[ "$(head -c 2 "$WORK/artifacts.zip")" == PK ]] || fail "artifacts.zip is not a zip"
echo "  artifacts.zip $(wc -c <"$WORK/artifacts.zip" | tr -d ' ') bytes"

step "realtime events received"
sleep 1
grep '^event:' "$WORK/sse" | sort | uniq -c
for ev in plan.started plan.node plan.ready run.started node.status node.output run.finished; do
  grep -q "^event: $ev\$" "$WORK/sse" || fail "missing SSE event $ev"
done

printf '\033[32mOK\033[0m smoke test passed against %s\n' "$BASE"
