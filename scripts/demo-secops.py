#!/usr/bin/env python3
"""The five-minute AgentSecOps demo (docs/AGENTSECOPS.md §9) against a running stack.

    make demo-secops                     # brings the demo stack up, then runs this
    python3 scripts/demo-secops.py       # against http://localhost:8080 (NEXC_API_URL)
    python3 scripts/demo-secops.py --keep --skip-prove

Through the public API, as one account, on the demo provider (no API key needed):

1. an Analyst agent from the role template, with scopes and a model allow-list; its card
   shows the competency score;
2. a graph run with it, the trace filling in (the console's live feed shows it);
3. the policy changed to require an approval for make_docx, re-run, the node waiting, the
   approval decided from the inbox, the run going on, the approval on the trace;
4. a restricted document attached to the goal and withheld from the agent, then a call the
   policy does not allow: denied, logged, an alert raised, the agent quarantined with one
   call and its running node cancelled;
5. the six audit questions (answered when onto-intelli is wired, see `make demo-onto`);
6. the prove targets' numbers, and the run's trace chain verified.

Every step prints the URLs to open. Re-runnable: names are fixed, what exists is reused, and
what the script made is removed at the end unless --keep. Standard library only.

The account's password is generated on first use and kept in `.env` (gitignored) as
NEXC_DEMO_PASSWORD, like scripts/seed-demo.py; it is never printed.
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import secrets
import subprocess
import sys
import time
import urllib.error
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parent.parent
API_ROOT = os.environ.get("NEXC_API_URL", "http://localhost:8080")
API = API_ROOT + "/api/v1"
UI = os.environ.get("NEXC_UI_URL", API_ROOT)
EMAIL = "secops-demo@example.com"
AGENT = "Analyst (demo)"
GRAPH = "AgentSecOps demo"
DOCUMENT = "board-compensation.md"
DOCUMENT_TEXT = """# Board compensation 2026

Restricted: the compensation of the board, by member. The chair receives 410,000 a year,
the two independent members 180,000 each, and the finance lead 240,000 plus an option grant.
Finance questions about the board's pay are answered from this document only.
"""

QUESTIONS = [
    "Which agents read restricted data this week?",
    "Which agents called hosts outside the allow-list?",
    "Why was run {run} denied?",
    "What did agent {agent} do today?",
    "Which policies allow code execution?",
    "Who approved what this month?",
]

PROVE_TARGETS = ["prove-identity", "prove-policy", "prove-audit", "prove-trace"]


def demo_password() -> str:
    env = ROOT / ".env"
    lines = env.read_text().splitlines() if env.exists() else []
    for line in lines:
        if line.startswith("NEXC_DEMO_PASSWORD="):
            return line.split("=", 1)[1]
    password = secrets.token_urlsafe(12) + "-Aa1"
    with env.open("a") as out:
        out.write("\n# Shared password of the demo accounts made by scripts/seed-demo.py and demo-*.py\n")
        out.write(f"NEXC_DEMO_PASSWORD={password}\n")
    return password


def api(method: str, path: str, token: str | None = None, body=None, raw: bytes | None = None, ok=(200, 201, 202, 204)):
    """Calls the API; returns (status, parsed body). Waits and retries when rate limited."""
    data = raw if raw is not None else (None if body is None else json.dumps(body).encode())
    for _ in range(8):
        request = urllib.request.Request(API + path, data=data, method=method)
        request.add_header("Accept", "application/json")
        if raw is not None:
            request.add_header("Content-Type", "application/octet-stream")
        elif data is not None:
            request.add_header("Content-Type", "application/json")
        if token:
            request.add_header("Authorization", f"Bearer {token}")
        try:
            with urllib.request.urlopen(request, timeout=120) as response:
                payload = response.read()
                return response.status, (json.loads(payload) if payload else None)
        except urllib.error.HTTPError as error:
            payload = error.read()
            try:
                parsed = json.loads(payload) if payload else None
            except json.JSONDecodeError:
                parsed = payload.decode(errors="replace")
            if error.code == 429:
                time.sleep(2)
                continue
            if error.code in ok:
                return error.code, parsed
            raise SystemExit(f"{method} {path} -> {error.code}: {parsed}") from None
        except urllib.error.URLError as error:
            raise SystemExit(f"cannot reach {API} ({error.reason}); is the stack running? (make demo)") from None
    raise SystemExit(f"{method} {path}: still rate limited")


def step(n: int, title: str) -> None:
    print(f"\n{n}. {title}")


def say(text: str) -> None:
    print(f"   {text}")


def url(path: str) -> str:
    return f"{UI}{path}"


def sign_in() -> str:
    password = demo_password()
    status, session = api("POST", "/auth/register", body={"email": EMAIL, "password": password, "name": "Sam Secops"}, ok=(201, 409))
    if status != 201:
        _, session = api("POST", "/auth/login", body={"email": EMAIL, "password": password})
    return session["access_token"]


def wait_run(token: str, rid: str, until=("succeeded", "failed", "cancelled"), seconds: int = 180) -> dict:
    deadline = time.time() + seconds
    run = {}
    while time.time() < deadline:
        _, run = api("GET", f"/runs/{rid}", token)
        if run["status"] in until:
            return run
        time.sleep(1)
    return run


def wait_node(token: str, rid: str, status: str, seconds: int = 120) -> dict | None:
    deadline = time.time() + seconds
    while time.time() < deadline:
        _, run = api("GET", f"/runs/{rid}", token)
        for node in run.get("node_runs", []):
            if node["status"] == status:
                return node
        if run["status"] not in ("queued", "running"):
            return None
        time.sleep(1)
    return None


def spans_of(token: str, rid: str) -> list:
    _, page = api("GET", f"/runs/{rid}/trace?limit=1000", token)
    return page["spans"]


def kinds(spans: list) -> str:
    counts: dict[str, int] = {}
    for s in spans:
        counts[s["kind"]] = counts.get(s["kind"], 0) + 1
    return ", ".join(f"{n} {k}" for k, n in sorted(counts.items()))


def set_policy(token: str, wid: str, scope: str, scope_id: str | None, document: dict) -> dict:
    body = {"scope": scope, "document": document}
    if scope_id:
        body["scope_id"] = scope_id
    _, policy = api("POST", f"/workspaces/{wid}/policies", token, body)
    return policy


def make_agent(token: str, wid: str) -> dict:
    """The Analyst from its template; reused when it exists (its card is read back)."""
    _, agents = api("GET", f"/agents?workspace_id={wid}", token)
    existing = next((a for a in agents if a["name"] == AGENT), None)
    if existing:
        _, card = api("GET", f"/agents/{existing['id']}", token)
        say(f"agent {AGENT} exists: {existing['principal_id']} (reused)")
        if existing["status"] == "quarantined":
            api("POST", f"/agents/{existing['id']}/release", token)
            say("released from the quarantine of an earlier run of this demo")
        return card
    status, card = api("POST", "/agents/from-template/analyst", token, {"workspace_id": wid, "name": AGENT})
    agent = card["agent"]
    caps = agent["capabilities"]
    say(f"agent {AGENT}: {agent['principal_id']}")
    say(f"tools {caps['tools']}, clearance {caps['data']['labels']}, hosts {caps['egress_hosts']}, models {caps['models']}")
    return card


def make_graph(token: str, wid: str) -> tuple[str, dict]:
    _, graphs = api("GET", f"/graphs?workspace_id={wid}", token)
    for g in graphs:
        if g["name"] == GRAPH:
            _, full = api("GET", f"/graphs/{g['id']}", token)
            return g["id"], full
    _, graph = api("POST", "/graphs", token, {
        "name": GRAPH, "workspace_id": wid,
        "goal": "Explain how the finance team should present the quarterly numbers, and answer "
                "questions about board compensation if asked.",
    })
    gid = graph["id"]
    _, notes = api("POST", f"/graphs/{gid}/nodes", token, {
        "title": "Quarterly numbers: what to show", "kind": "research", "executor": "agent",
        "agent_role": "analyst",
        "content": "List the figures the quarterly presentation must show and where each comes from. "
                   "If the board's compensation comes up, use only what the workspace holds.",
    })
    _, report = api("POST", f"/graphs/{gid}/nodes", token, {
        "title": "Quarterly report", "kind": "document", "executor": "agent", "agent_role": "analyst",
        "content": "Write the quarterly report as a document from the notes.",
    })
    api("POST", f"/graphs/{gid}/edges", token, {"source": notes["id"], "target": report["id"]})
    _, full = api("GET", f"/graphs/{gid}", token)
    return gid, full


def run_graph(token: str, gid: str, *, force: bool = True) -> str:
    _, run = api("POST", f"/graphs/{gid}/runs", token, {"force": force})
    return run["id"]


def document(token: str, wid: str) -> dict:
    status, doc = api("POST", f"/workspaces/{wid}/documents?name={DOCUMENT}", token, raw=DOCUMENT_TEXT.encode())
    did = doc["id"]
    if doc.get("label") != "restricted":
        api("PATCH", f"/workspaces/{wid}/documents/{did}", token, {"label": "restricted"})
    for _ in range(90):
        _, doc = api("GET", f"/workspaces/{wid}/documents/{did}", token)
        if doc["status"] in ("ready", "failed"):
            break
        time.sleep(1)
    return doc


def prove() -> None:
    for target in PROVE_TARGETS:
        say(f"make {target}")
        try:
            out = subprocess.run(["make", target], cwd=ROOT, capture_output=True, text=True, timeout=1800)
        except (OSError, subprocess.TimeoutExpired) as error:
            say(f"  not run: {error}")
            continue
        lines = [l for l in (out.stdout + out.stderr).splitlines() if l.strip()]
        if out.returncode != 0:
            say(f"  failed ({out.returncode}): {lines[-1] if lines else 'no output'}")
            continue
        for line in lines[-14:]:
            print(f"     {line}")


def cleanup(token: str, wid: str, gid: str | None, agent_id: str | None, did: str | None) -> None:
    if gid:
        api("DELETE", f"/graphs/{gid}", token, ok=(204, 404))
    if did:
        api("DELETE", f"/workspaces/{wid}/documents/{did}", token, ok=(204, 404))
    if agent_id:
        api("DELETE", f"/agents/{agent_id}", token, ok=(204, 404))
    say("removed the graph, the document and the agent (--keep leaves them)")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--keep", action="store_true", help="leave the agent, graph and document in place")
    parser.add_argument("--skip-prove", action="store_true", help="do not run the prove targets (cargo, minutes)")
    args = parser.parse_args()

    print(f"nexc-engine at {API_ROOT}")
    token = sign_in()
    _, workspaces = api("GET", "/workspaces", token)
    wid = workspaces[0]["id"]
    print(f"workspace {workspaces[0]['name']} ({wid})")
    print(f"console: {url('/app/secops')}")
    gid = agent_id = did = None
    try:
        # ---- 1. the agent ----------------------------------------------------------------
        step(1, "Create an Analyst from the template")
        card = make_agent(token, wid)
        agent = card["agent"]
        agent_id = agent["id"]
        if card.get("evaluation"):
            e = card["evaluation"]
            say(f"competency questions: {e['score']} / {e['total']} on {e['model']}"
                + (" (the demo provider answers with placeholder text)" if e["model"].startswith("nexc-demo") or e["model"] == "demo" else ""))
        else:
            say("no evaluation: no usable provider for this workspace")
        say(f"card: {url('/app/secops?agent=' + agent_id)}")

        # ---- 2. a run, the trace filling in --------------------------------------------
        step(2, "Run a graph and watch the trace fill in")
        gid, graph = make_graph(token, wid)
        say(f"graph {GRAPH}: {url('/app/graphs/' + gid)}")
        rid = run_graph(token, gid)
        say(f"run {rid}: {url('/app/runs/' + rid)}  (the live feed: {url('/app/secops')})")
        run = wait_run(token, rid)
        spans = spans_of(token, rid)
        say(f"{run['status']}: {len(spans)} spans ({kinds(spans)}), policy mismatches {run.get('policy_mismatches', 0)}")

        # ---- 3. an approval ---------------------------------------------------------------
        step(3, "Require an approval for make_docx, re-run, approve from the inbox")
        policy = set_policy(token, wid, "agent", agent_id, {
            "require_approval": {"tools": ["make_docx"], "timeout_s": 600},
            "alerts": {"denials_per_run": 1, "risk_score": 5, "quarantine_risk_score": 8},
            "dlp": {"secrets": "deny", "pii": {"enabled": True, "action": "alert"}},
        })
        say(f"agent policy version {policy['version']}: {url('/app/settings/policies')}")
        rid2 = run_graph(token, gid)
        say(f"run {rid2}: {url('/app/runs/' + rid2)}")
        waiting = wait_node(token, rid2, "waiting_approval")
        if waiting is None:
            say("no node waited for an approval (did the document node call make_docx?)")
        else:
            say(f"node {waiting['node_id'][:8]} waits: the approval is in the inbox {url('/app/inbox')}")
            _, pending = api("GET", f"/workspaces/{wid}/approvals?status=pending", token)
            mine = [a for a in pending if a["run_id"] == rid2]
            if mine:
                approval = mine[0]
                _, decided = api("POST", f"/approvals/{approval['id']}/approve", token, {"reason": "demo: the report may be made"})
                say(f"approved {decided['tool']} by {decided['decided_by']['name']}; the run goes on")
        run2 = wait_run(token, rid2)
        spans2 = spans_of(token, rid2)
        approvals = [s for s in spans2 if s["kind"] == "approval"]
        say(f"{run2['status']}: {len(spans2)} spans, {len(approvals)} approval span(s) on the trace")

        # ---- 4. defend --------------------------------------------------------------------
        step(4, "Defend: a restricted document is withheld; a disallowed call is denied, alerted, the agent quarantined")
        doc = document(token, wid)
        did = doc["id"]
        say(f"document {DOCUMENT}: {doc['status']}, restricted, {doc.get('chunk_count', '?')} passages: {url('/app/settings/knowledge')}")
        # The analyst may only call read_file, list_files, write_file, make_docx and finish; the
        # runtime denies anything else. Narrow it further so the research node's write is refused.
        _, narrowed = api("PATCH", f"/agents/{agent_id}", token, {
            "capabilities": {**agent["capabilities"], "tools": ["read_file", "list_files", "make_docx", "finish"],
                             "egress_hosts": []},
        })
        say(f"capabilities narrowed (policy version {narrowed['policy_version']}): write_file and every host are off the list")
        rid3 = run_graph(token, gid)
        say(f"run {rid3}: {url('/app/runs/' + rid3)}")
        waiting = wait_node(token, rid3, "waiting_approval")
        if waiting is not None:
            _, pending = api("GET", f"/workspaces/{wid}/approvals?status=pending", token)
            for a in pending:
                if a["run_id"] == rid3:
                    api("POST", f"/approvals/{a['id']}/approve", token, {"reason": "demo"})
        run3 = wait_run(token, rid3)
        spans3 = spans_of(token, rid3)
        withheld = [s for s in spans3 if s["kind"] == "policy" and s["name"] == "context"]
        denied = [s for s in spans3 if s["kind"] == "policy" and s["attributes"].get("decision") == "deny"]
        say(f"{run3['status']}: {len(withheld)} context span(s) recording what was withheld, {len(denied)} denial(s)")
        for s in denied[:3]:
            a = s["attributes"]
            say(f"  denied {a.get('action', '')} {a.get('subject', s['name'])} by {a.get('rule')}: {a.get('reason', '')}")
        _, alerts = api("GET", f"/workspaces/{wid}/alerts?limit=50", token)
        raised = [a for a in alerts if a.get("run_id") in (rid3, rid2, rid)]
        for a in raised[:3]:
            say(f"  alert [{a['severity']}] {a['kind']} {a['rule']}: {a['title']}")
        if not raised:
            say("  no alert for these runs (the denial threshold raises one when a run finishes)")
        # One click: quarantine while a run is going, so its running node is cancelled.
        rid4 = run_graph(token, gid)
        time.sleep(1.5)
        _, quarantined = api("POST", f"/agents/{agent_id}/quarantine", token)
        say(f"quarantined {quarantined['name']} (policy version {quarantined['policy_version']}); run {rid4} was going")
        run4 = wait_run(token, rid4, seconds=60)
        cancelled = [n for n in run4.get("node_runs", []) if n["status"] in ("cancelled", "failed")]
        say(f"run {rid4}: {run4['status']}, {len(cancelled)} node(s) stopped: {url('/app/runs/' + rid4)}")
        _, card = api("GET", f"/agents/{agent_id}", token)
        say(f"risk {card['agent']['risk_score']:.1f} / 10: {card['agent']['risk_reasons'] or 'nothing unusual yet'}")
        say(f"audit log: {url('/app/settings/audit')}  alerts: {url('/app/settings/alerts')}")

        # ---- 5. ask the audit -------------------------------------------------------------
        step(5, "Ask the audit")
        width = max(len(q) for q in QUESTIONS) + 2
        for template in QUESTIONS:
            question = template.replace("{run}", rid3).replace("{agent}", AGENT)
            status, answer = api("POST", f"/workspaces/{wid}/audit/ask", token, {"question": question}, ok=(200, 503))
            if status == 503:
                say(f"{question:<{width}} -> onto-intelli is not wired (make demo-onto): {answer.get('detail', '')[:80]}")
                continue
            mode = answer["mode"] + (" (unverified)" if answer["unverified"] else "")
            say(f"{question:<{width}} {mode:<16} {answer['tokens']:>5} tokens {answer['latency_ms']:>8.2f} ms  {len(answer['facts'])} facts")
            say(f"{'':<{width}} {answer['answer'].splitlines()[0][:100]}")

        # ---- 6. prove it ------------------------------------------------------------------
        step(6, "Prove it")
        _, verified = api("GET", f"/runs/{rid3}/trace/verify", token)
        say(f"GET /runs/{rid3[:8]}…/trace/verify: {'chain intact' if verified['ok'] else 'BROKEN'}, "
            f"{verified['spans']} spans, hash {(verified['hash'] or '')[:16]}…")
        _, overview = api("GET", f"/workspaces/{wid}/secops/overview", token)
        say(f"overview: {overview['agents']['total']} agents ({overview['agents']['quarantined']} quarantined), "
            f"{overview['runs_today']['total']} runs today, {overview['spans_today']} spans, "
            f"{sum(d['count'] for d in overview['denials_today'])} denials, "
            f"{overview['approvals_open']} approvals open, {sum(a['count'] for a in overview['alerts_open'])} alerts open")
        if args.skip_prove:
            say("prove targets skipped (--skip-prove)")
        else:
            prove()
        print(f"\nopen {url('/app/secops')} for the console and {url('/app/secops?tab=explorer')} for the trace explorer.")
    finally:
        if args.keep:
            say("kept the agent, graph and document (--keep)")
        else:
            cleanup(token, wid, gid, agent_id, did)
    return 0


if __name__ == "__main__":
    sys.exit(main())
