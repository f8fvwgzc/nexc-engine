#!/usr/bin/env python3
"""Runs real issues through the whole loop with the configured model and reports what came out.

For each issue: give it a graph, have the planner refine it, apply the plan, run it, and read
back every node's result, the artifacts, the time and the tokens. It is how to judge whether the
agents do useful work on your own issues, and what that costs.

    NEXC_PROVE_EMAIL=... NEXC_PROVE_PASSWORD=... scripts/prove-loop.py ENG-7 DES-2 [--base URL]

The account must see the issues; pass their identifiers. Output goes to stdout and, in full, to
data/prove-loop/<time>.json. Nothing is deleted: the graphs and runs stay on the issues.
"""

from __future__ import annotations

import argparse
import datetime
import json
import os
import pathlib
import sys
import time
import urllib.error
import urllib.request


class Api:
    def __init__(self, base: str, email: str, password: str) -> None:
        self.base, self.email, self.password, self.token = base.rstrip("/") + "/api/v1", email, password, ""
        self.login()

    def login(self) -> None:
        self.token = ""
        self.token = self.call("POST", "/auth/login", {"email": self.email, "password": self.password})["access_token"]

    def call(self, method: str, path: str, body: object | None = None, retry: bool = True):
        data = None if body is None else json.dumps(body).encode()
        headers = {"Content-Type": "application/json"}
        if self.token:
            headers["Authorization"] = f"Bearer {self.token}"
        request = urllib.request.Request(self.base + path, data=data, method=method, headers=headers)
        try:
            with urllib.request.urlopen(request, timeout=120) as response:
                raw = response.read()
                return json.loads(raw) if raw else None
        except urllib.error.HTTPError as err:
            if err.code == 401 and retry and self.token:  # long runs outlive the access token
                self.login()
                return self.call(method, path, body, retry=False)
            raise SystemExit(f"{method} {path} -> {err.code}: {err.read().decode()[:400]}") from err


def wait(api: Api, path: str, busy: set[str], timeout: int) -> dict:
    deadline = time.monotonic() + timeout
    while True:
        state = api.call("GET", path)
        if state["status"] not in busy:
            return state
        if time.monotonic() > deadline:
            raise SystemExit(f"{path} was still {state['status']} after {timeout}s")
        time.sleep(3)


def prove(api: Api, workspace: str, identifier: str, plan_timeout: int, run_timeout: int) -> dict:
    found = api.call("GET", f"/workspaces/{workspace}/issues?q={identifier}&limit=5")
    issue = next((i for i in found if i["identifier"] == identifier), None)
    if issue is None:
        raise SystemExit(f"no issue {identifier} that this account can see")
    started = time.monotonic()
    gid = api.call("POST", f"/issues/{issue['id']}/graph")["graph_id"]
    plan = api.call("POST", f"/graphs/{gid}/plan", {"instructions": "Keep it to what the issue needs."})
    plan = wait(api, f"/graphs/{gid}/plans/{plan['id']}", {"streaming"}, plan_timeout)
    planned = time.monotonic()
    report: dict = {"issue": identifier, "title": issue["title"], "graph_id": gid, "plan": plan["status"]}
    if plan["status"] != "ready":
        return report | {"error": plan.get("error") or "the plan did not become ready"}
    api.call("POST", f"/graphs/{gid}/plans/{plan['id']}/apply")
    run = api.call("POST", f"/graphs/{gid}/runs", {})
    run = wait(api, f"/runs/{run['id']}", {"queued", "running"}, run_timeout)
    graph = api.call("GET", f"/graphs/{gid}")
    artifacts = api.call("GET", f"/runs/{run['id']}/artifacts")
    nodes = {n["id"]: n for n in graph["nodes"]}
    return report | {
        "plan_summary": plan.get("summary", ""),
        "plan_seconds": round(planned - started),
        "run": run["status"],
        "run_seconds": round(time.monotonic() - planned),
        "tokens_in": run.get("tokens_in", 0),
        "tokens_out": run.get("tokens_out", 0),
        "cost_usd": run.get("cost_usd", 0),
        "nodes": [
            {
                "title": nodes.get(r["node_id"], {}).get("title", r["node_id"]),
                "kind": nodes.get(r["node_id"], {}).get("kind", ""),
                "executor": nodes.get(r["node_id"], {}).get("executor", ""),
                "status": r["status"],
                "error": r.get("error"),
                "output": nodes.get(r["node_id"], {}).get("output") or "",
            }
            for r in run.get("node_runs", [])
        ],
        "artifacts": [{"path": a["path"], "mime": a["mime"], "size": a["size"]} for a in artifacts],
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("issues", nargs="+", help="issue identifiers, e.g. ENG-7")
    parser.add_argument("--base", default=os.environ.get("NEXC_PROVE_URL", "http://localhost:8080"))
    parser.add_argument("--workspace", default=os.environ.get("NEXC_PROVE_WORKSPACE", ""), help="workspace name")
    parser.add_argument("--plan-timeout", type=int, default=600)
    parser.add_argument("--run-timeout", type=int, default=2400)
    args = parser.parse_args()
    email, password = os.environ.get("NEXC_PROVE_EMAIL"), os.environ.get("NEXC_PROVE_PASSWORD")
    if not email or not password:
        raise SystemExit("set NEXC_PROVE_EMAIL and NEXC_PROVE_PASSWORD")
    api = Api(args.base, email, password)
    workspaces = api.call("GET", "/workspaces")
    workspace = next((w for w in workspaces if w["name"] == args.workspace), None) or max(
        workspaces, key=lambda w: w["member_count"]
    )
    reports = []
    for identifier in args.issues:
        print(f"== {identifier}", flush=True)
        report = prove(api, workspace["id"], identifier, args.plan_timeout, args.run_timeout)
        reports.append(report)
        done = [n for n in report.get("nodes", []) if n["status"] == "succeeded"]
        print(
            f"   {report['title']}\n   plan {report['plan']} in {report.get('plan_seconds', '?')}s; "
            f"run {report.get('run', 'not started')} in {report.get('run_seconds', '?')}s; "
            f"{len(done)}/{len(report.get('nodes', []))} nodes; "
            f"{report.get('tokens_in', 0)} tokens in, {report.get('tokens_out', 0)} out, "
            f"${report.get('cost_usd', 0):.2f}; {len(report.get('artifacts', []))} artifacts",
            flush=True,
        )
        for node in report.get("nodes", []):
            first = " ".join((node["output"] or node["error"] or "").split())[:160]
            print(f"   - [{node['status']}] {node['title']} ({node['executor']}): {first}", flush=True)
    out = pathlib.Path("data/prove-loop")
    out.mkdir(parents=True, exist_ok=True)
    path = out / f"{datetime.datetime.now():%Y%m%d-%H%M%S}.json"
    path.write_text(json.dumps(reports, indent=2))
    print(f"full results: {path}")
    if any(r.get("run") != "succeeded" for r in reports):
        sys.exit(1)


if __name__ == "__main__":
    main()
