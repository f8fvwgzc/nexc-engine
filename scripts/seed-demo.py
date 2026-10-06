#!/usr/bin/env python3
"""Fills a running nexc-engine with a demo workspace through its public API.

    python3 scripts/seed-demo.py            # against http://localhost:8080

Creates five accounts with different workspace roles, the workspace "Acme Robotics" with three
teams, labels, projects, cycles, issues, sub-issues and comments. Everything goes through the
API as the account that would do it, so the inboxes it leaves behind are real notifications.
Running it again changes nothing.

All demo accounts share one password. It is generated on first use and kept in `.env`
(gitignored) as NEXC_DEMO_PASSWORD; it is never printed. Standard library only.
"""

from __future__ import annotations

import datetime
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
WORKSPACE = "Acme Robotics"

# (key, e-mail, name, role in the workspace)
PEOPLE = [
    ("owner", "acme-owner@example.com", "Olivia Owner", "owner"),
    ("admin", "acme-admin@example.com", "Adam Admin", "admin"),
    ("dev", "acme-dev@example.com", "Dana Developer", "member"),
    ("design", "acme-design@example.com", "Diego Designer", "member"),
    ("guest", "acme-guest@example.com", "Gina Guest", "guest"),
]


def demo_password() -> str:
    """The shared demo password from `.env`, created there on first use."""
    env = ROOT / ".env"
    lines = env.read_text().splitlines() if env.exists() else []
    for line in lines:
        if line.startswith("NEXC_DEMO_PASSWORD="):
            return line.split("=", 1)[1]
    password = secrets.token_urlsafe(12) + "-Aa1"
    with env.open("a") as out:
        out.write("\n# Shared password of the demo accounts made by scripts/seed-demo.py\n")
        out.write(f"NEXC_DEMO_PASSWORD={password}\n")
    return password


def api(method: str, path: str, token: str | None = None, body: object = None, ok=(200, 201, 202, 204)):
    """Calls the API; returns (status, parsed body). Waits and retries when rate limited."""
    data = None if body is None else json.dumps(body).encode()
    for _ in range(6):
        request = urllib.request.Request(API + path, data=data, method=method)
        request.add_header("Accept", "application/json")
        if data is not None:
            request.add_header("Content-Type", "application/json")
        if token:
            request.add_header("Authorization", f"Bearer {token}")
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                raw = response.read()
                return response.status, (json.loads(raw) if raw else None)
        except urllib.error.HTTPError as error:
            raw = error.read()
            parsed = json.loads(raw) if raw else None
            if error.code == 429:
                time.sleep(2)
                continue
            if error.code in ok:
                return error.code, parsed
            raise SystemExit(f"{method} {path} -> {error.code}: {parsed}") from None
        except urllib.error.URLError as error:
            raise SystemExit(f"cannot reach {API} ({error.reason}); is the app running?") from None
    raise SystemExit(f"{method} {path}: still rate limited")


def sign_in(email: str, name: str, password: str) -> str:
    """An access token for the account, registering it when it does not exist yet."""
    status, body = api("POST", "/auth/register", body={"email": email, "password": password, "name": name}, ok=(201, 409))
    if status == 409:
        _, body = api("POST", "/auth/login", body={"email": email, "password": password})
    return body["access_token"]


def main() -> None:
    password = demo_password()
    token, user_id = {}, {}
    for key, email, name, _ in PEOPLE:
        token[key] = sign_in(email, name, password)
        user_id[key] = api("GET", "/auth/me", token[key])[1]["id"]

    owner = token["owner"]
    workspaces = api("GET", "/workspaces", owner)[1]
    home = next((w for w in workspaces if w["name"] == WORKSPACE), None) or workspaces[0]
    wid = home["id"]
    ws = f"/workspaces/{wid}"
    if home["name"] == WORKSPACE and api("GET", f"{ws}/teams", owner)[1]:
        print(f"{WORKSPACE} is already there; nothing was changed.")
    else:
        build(ws, token, user_id)
    report(ws, token)


def build(ws: str, token: dict, user_id: dict) -> None:
    owner = token["owner"]
    api("PATCH", ws, owner, {"name": WORKSPACE})
    for key, email, _, role in PEOPLE[1:]:
        api("POST", f"{ws}/members", owner, {"email": email, "role": role}, ok=(201, 409))

    def team(name, key, private=False, members=()):
        created = api("POST", f"{ws}/teams", owner, {"name": name, "key": key, "private": private})[1]
        for member in members:
            api("PUT", f"{ws}/teams/{created['id']}/members/{user_id[member]}", owner, {"role": "member"})
        states = api("GET", f"{ws}/teams/{created['id']}/states", owner)[1]
        return created["id"], {s["name"]: s["id"] for s in states}

    eng, eng_state = team("Engineering", "ENG", members=("admin", "dev"))
    des, des_state = team("Design", "DES", members=("design", "guest"))
    lead, _ = team("Leadership", "LEAD", private=True, members=("admin",))

    label = {
        name: api("POST", f"{ws}/labels", owner, {"name": name, "color": color})[1]["id"]
        for name, color in [("Bug", "#ef4444"), ("Feature", "#6366f1"), ("Research", "#10b981")]
    }
    today = datetime.date.today()
    day = lambda offset: (today + datetime.timedelta(days=offset)).isoformat()  # noqa: E731
    project = {}
    for name, description, target in [
        ("Arm controller v2", "Rewrite the motion controller for the six-axis arm.", day(45)),
        ("Customer portal", "Self-service portal for orders, firmware and support.", day(80)),
        ("Factory pilot", "First deployment on a customer's line.", day(120)),
    ]:
        body = {"name": name, "description": description, "target_date": target}
        project[name] = api("POST", f"{ws}/projects", owner, body)[1]["id"]
    api("PATCH", f"{ws}/projects/{project['Arm controller v2']}", owner, {"status": "started"})
    sprint = api("POST", f"{ws}/teams/{eng}/cycles", owner, {"name": "Sprint 14", "starts_on": day(-4), "ends_on": day(9)})[1]["id"]
    api("POST", f"{ws}/teams/{eng}/cycles", owner, {"name": "Sprint 15", "starts_on": day(10), "ends_on": day(23)})

    def issue(team_id, by, title, **fields):
        return api("POST", f"{ws}/teams/{team_id}/issues", token[by], {"title": title, **fields})[1]["id"]

    def comment(issue_id, by, text):
        api("POST", f"/issues/{issue_id}/comments", token[by], {"body": text})

    def change(issue_id, by, **fields):
        api("PATCH", f"/issues/{issue_id}", token[by], fields)

    arm = project["Arm controller v2"]
    controller = issue(eng, "owner", "Ship the new trajectory planner", priority=2, assignee_id=user_id["dev"],
                       project_id=arm, cycle_id=sprint, label_ids=[label["Feature"]],
                       description="Replace the trapezoid profiles with jerk-limited S-curves.")
    for title, who in [("Port the kinematics solver", "dev"), ("Bench test on axis 3", "admin"), ("Tune the feed-forward gains", "dev")]:
        issue(eng, "dev", title, parent_id=controller, assignee_id=user_id[who], project_id=arm, cycle_id=sprint)
    change(controller, "dev", state_id=eng_state["In Progress"])
    comment(controller, "dev", "Solver is ported; axis 3 overshoots by 0.4 mm at full speed.")
    comment(controller, "admin", "Bench is free on Thursday, I can run the sweep then.")
    comment(controller, "owner", "Good. Keep the old profile behind a flag until the pilot.")

    estop = issue(eng, "admin", "Emergency stop latency above 12 ms", priority=1, assignee_id=user_id["dev"],
                  label_ids=[label["Bug"]], cycle_id=sprint, project_id=project["Factory pilot"])
    comment(estop, "dev", "Reproduced: the watchdog shares an interrupt with the encoder.")
    change(estop, "dev", state_id=eng_state["In Review"])
    firmware = issue(eng, "dev", "Signed firmware updates", priority=3, assignee_id=user_id["admin"], label_ids=[label["Feature"]])
    change(firmware, "admin", state_id=eng_state["Done"])
    issue(eng, "owner", "Evaluate CAN FD for the wrist joint", priority=4, label_ids=[label["Research"]])
    issue(eng, "dev", "Flaky encoder test on CI", priority=3, assignee_id=user_id["dev"], label_ids=[label["Bug"]])

    portal = project["Customer portal"]
    dashboard = issue(des, "owner", "Order status dashboard", priority=2, assignee_id=user_id["design"], project_id=portal,
                      description="One screen: what was ordered, where it is, what is next.")
    comment(dashboard, "design", "First wireframes are in the shared folder.")
    comment(dashboard, "guest", "From the customer side: we mostly check delivery dates.")
    change(dashboard, "design", state_id=des_state["In Progress"])
    issue(des, "design", "Firmware download page", priority=3, assignee_id=user_id["design"], project_id=portal)
    onboarding = issue(des, "admin", "Onboarding checklist for new customers", priority=3, assignee_id=user_id["guest"], project_id=portal)
    comment(onboarding, "guest", "Drafted eight steps; step 5 needs a safety sign-off.")
    issue(des, "design", "Icon set for joint states", priority=4)

    budget = issue(lead, "owner", "Approve the pilot budget", priority=1, assignee_id=user_id["admin"], project_id=project["Factory pilot"])
    comment(budget, "admin", "Numbers are in; travel is the open item.")
    issue(lead, "admin", "Hire a second controls engineer", priority=2, assignee_id=user_id["owner"])
    print(f"Created {WORKSPACE}: 3 teams, 3 projects, 2 cycles, 3 labels, 14 issues with comments.")


def report(ws: str, token: dict) -> None:
    """Shows that each account has its own inbox, filled by what the others did."""
    print(f"\n{'Account':28} {'Role':8} Inbox (unread / all), latest")
    for key, email, _, role in PEOPLE:
        inbox = api("GET", f"{ws}/inbox", token[key])[1]
        unread = sum(1 for n in inbox if n["read_at"] is None)
        latest = ""
        if inbox:
            n = inbox[0]
            who = (n["actor"] or {}).get("name", "someone")
            latest = f"{who} · {n['kind']} · {n['issue']['identifier']}"
        print(f"{email:28} {role:8} {unread} / {len(inbox)}   {latest}")
    print("\nPassword of all five: NEXC_DEMO_PASSWORD in .env")


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        sys.exit(130)
