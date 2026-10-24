#!/usr/bin/env python3
"""Asks the audit, end to end, against a running stack with onto-intelli.

    make demo-onto                       # brings the stack up, then runs this
    python3 scripts/demo-onto.py         # against http://localhost:8080 (NEXC_API_URL)

Through the public API, as one account: three agents with different clearances, a restricted
document, a two-node graph that reads it, one run of it. Then, once the trace has reached
onto-intelli, the six canned audit questions, printing for each the mode, the tokens, the
latency and the facts cited. Running it again reuses what it made. Standard library only.

The account's password is generated on first use and kept in `.env` (gitignored) as
NEXC_DEMO_PASSWORD, like scripts/seed-demo.py; it is never printed.
"""

from __future__ import annotations

import json
import os
import pathlib
import secrets
import sys
import time
import urllib.error
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parent.parent
API = os.environ.get("NEXC_API_URL", "http://localhost:8080") + "/api/v1"
EMAIL = "audit-demo@example.com"
DOCUMENT = "rotterdam-customer-accounts.md"
DOCUMENT_TEXT = """# Rotterdam customer accounts

Restricted: the customer accounts served from the Rotterdam warehouse, with their customs
references. Account RC-1041 (Vermeer Logistics) ships under customs reference NL-88-2041.
Account RC-1042 (Haven Foods) ships under NL-88-2042. Account RC-1043 (Delta Parts) ships
under NL-88-2043. Invoices from the Rotterdam warehouse quote the customs reference of the
account they bill.
"""

QUESTIONS = [
    "Which agents read restricted data this week?",
    "Which agents called hosts outside the allow-list?",
    "Why was run {run} denied?",
    "What did agent {agent} do today?",
    "Which policies allow code execution?",
    "Who approved what this month?",
]


def demo_password() -> str:
    env = ROOT / ".env"
    lines = env.read_text().splitlines() if env.exists() else []
    for line in lines:
        if line.startswith("NEXC_DEMO_PASSWORD="):
            return line.split("=", 1)[1]
    password = secrets.token_urlsafe(12) + "-Aa1"
    with env.open("a") as out:
        out.write("\n# Shared password of the demo accounts made by scripts/seed-demo.py and demo-onto.py\n")
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
            with urllib.request.urlopen(request, timeout=60) as response:
                payload = response.read()
                return response.status, (json.loads(payload) if payload else None)
        except urllib.error.HTTPError as error:
            payload = error.read()
            parsed = json.loads(payload) if payload else None
            if error.code == 429:
                time.sleep(2)
                continue
            if error.code in ok:
                return error.code, parsed
            raise SystemExit(f"{method} {path} -> {error.code}: {parsed}") from None
        except urllib.error.URLError as error:
            raise SystemExit(f"cannot reach {API} ({error.reason}); is the stack running?") from None
    raise SystemExit(f"{method} {path}: still rate limited")


def sign_in() -> str:
    password = demo_password()
    status, session = api("POST", "/auth/register", body={"email": EMAIL, "password": password, "name": "Ada Auditor"}, ok=(201, 409))
    if status != 201:
        status, session = api("POST", "/auth/login", body={"email": EMAIL, "password": password})
    return session["access_token"]


def agent(token: str, wid: str, agents: list, name: str, role: str, capabilities: dict, purpose: str) -> dict:
    for existing in agents:
        if existing["name"] == name:
            return existing
    _, made = api("POST", "/agents", token, {
        "name": name, "role": role, "runtime": "builtin", "purpose": purpose,
        "workspace_id": wid, "capabilities": capabilities,
    })
    print(f"  agent {name} ({role}): {made['principal_id']}")
    return made


def document(token: str, wid: str) -> dict:
    status, doc = api("POST", f"/workspaces/{wid}/documents?name={DOCUMENT}", token, raw=DOCUMENT_TEXT.encode())
    did = doc["id"]
    if doc.get("label") != "restricted":
        api("PATCH", f"/workspaces/{wid}/documents/{did}", token, {"label": "restricted"})
    for _ in range(60):
        _, doc = api("GET", f"/workspaces/{wid}/documents/{did}", token)
        if doc["status"] in ("ready", "failed"):
            break
        time.sleep(1)
    if doc["status"] != "ready":
        raise SystemExit(f"the document did not become ready: {doc}")
    print(f"  document {DOCUMENT}: {doc['status']}, restricted, {doc.get('chunk_count', '?')} passages")
    return doc


def run_graph(token: str) -> str:
    _, graph = api("POST", "/graphs", token, {
        "name": "Audit demo", "goal": "Answer customs questions about the Rotterdam customer accounts",
    })
    gid = graph["id"]
    _, first = api("POST", f"/graphs/{gid}/nodes", token, {
        "title": "Rotterdam customs references", "kind": "research", "executor": "llm", "agent_role": "customs-analyst",
        "content": "Which customs reference does each Rotterdam customer account ship under? List the accounts.",
    })
    _, second = api("POST", f"/graphs/{gid}/nodes", token, {
        "title": "Customer note", "kind": "document", "executor": "llm", "agent_role": "clerk",
        "content": "Write a short note for the Rotterdam warehouse about customs references on invoices.",
    })
    api("POST", f"/graphs/{gid}/edges", token, {"source": first["id"], "target": second["id"]})
    _, run = api("POST", f"/graphs/{gid}/runs", token, {})
    rid = run["id"]
    for _ in range(120):
        _, run = api("GET", f"/runs/{rid}", token)
        if run["status"] not in ("queued", "running"):
            break
        time.sleep(1)
    print(f"  run {rid}: {run['status']}, {len(run.get('node_runs', []))} nodes, policy mismatches {run.get('policy_mismatches', 0)}")
    return rid


def ask(token: str, wid: str, question: str) -> tuple[int, dict]:
    return api("POST", f"/workspaces/{wid}/audit/ask", token, {"question": question}, ok=(200, 503))


def wait_for_export(token: str, wid: str, agent_name: str) -> None:
    """The export runs every two seconds; the first answer that sees the run means it landed."""
    deadline = time.time() + 60
    while time.time() < deadline:
        status, answer = ask(token, wid, f"What did agent {agent_name} do today?")
        if status == 503:
            raise SystemExit(
                f"ask the audit is off or onto-intelli does not answer: {answer.get('detail')}\n"
                "set NEXC_ONTO_URL (make demo-onto wires it to the onto-intelli service)"
            )
        if "worked in" in answer["answer"]:
            return
        time.sleep(2)
    print("  warning: the run has not reached onto-intelli yet; asking anyway", file=sys.stderr)


def main() -> int:
    print(f"nexc-engine at {API}")
    token = sign_in()
    _, workspaces = api("GET", "/workspaces", token)
    wid = workspaces[0]["id"]
    print(f"workspace {workspaces[0]['name']} ({wid})")
    _, agents = api("GET", f"/agents?workspace_id={wid}", token)
    analyst = agent(token, wid, agents, "Analyst", "customs-analyst",
        {"data": {"labels": ["public", "internal", "confidential", "restricted"]}},
        "Answers customs questions from the restricted customer accounts.")
    agent(token, wid, agents, "Clerk", "clerk", {},
        "Writes notes for the warehouse; cleared for internal data only.")
    agent(token, wid, agents, "Scout", "scout",
        {"tools": ["web_search"], "egress_hosts": ["api.example.com"]},
        "Looks things up outside; may only reach api.example.com.")
    document(token, wid)
    rid = run_graph(token)
    wait_for_export(token, wid, analyst["name"])

    print("\nask the audit:")
    width = max(len(q) for q in QUESTIONS) + 2
    failed = False
    for template in QUESTIONS:
        question = template.replace("{run}", rid).replace("{agent}", analyst["name"])
        status, answer = ask(token, wid, question)
        if status != 200:
            failed = True
            print(f"  {question:<{width}} -> {status}: {answer}")
            continue
        mode = answer["mode"] + (" (unverified)" if answer["unverified"] else "")
        print(f"  {question:<{width}} {mode:<16} {answer['tokens']:>6} tokens {answer['latency_ms']:>8.2f} ms  {len(answer['facts'])} facts")
        print(f"  {'':<{width}} {answer['answer'].splitlines()[0][:110]}")
        for fact in answer["facts"][:4]:
            print(f"  {'':<{width}}   · {fact['kind']} {fact['label']}" + (f" -> {fact['link']}" if fact["link"] else ""))
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
