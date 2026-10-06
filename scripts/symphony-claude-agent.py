#!/usr/bin/env python3
"""Runs texc-symphony coding turns with the Claude Code CLI.

texc-symphony drives its agent over the Codex app-server protocol (JSON lines on stdio). This
adapter speaks the part of that protocol Symphony needs for a plain coding turn (initialize,
thread/start, turn/start, token usage, turn completion or failure) and runs each turn as
`claude -p` in the issue workspace, so the work uses the logged-in Claude Code CLI instead of Codex.

Use it as the Symphony agent command (`NEXC_SYMPHONY_AGENT_COMMAND`). Environment:

  NEXC_SYMPHONY_CLAUDE_BIN    claude binary (default: claude)
  NEXC_SYMPHONY_CLAUDE_MODEL  model alias or id (default: sonnet)
  NEXC_SYMPHONY_CLAUDE_TOOLS  comma-separated allowed tools (default: file tools and a few
                              read-only or build commands; there is no sandbox, so widen with care)

After a successful turn it creates `.git/nexc-turn-ok`, which tells the bridge's `after_run` hook
that the workspace holds finished work and may be merged. Tracker tools are not bridged.
"""
import json
import os
import subprocess
import sys
import uuid

CLAUDE = os.environ.get("NEXC_SYMPHONY_CLAUDE_BIN", "claude")
MODEL = os.environ.get("NEXC_SYMPHONY_CLAUDE_MODEL", "sonnet")
TOOLS = os.environ.get(
    "NEXC_SYMPHONY_CLAUDE_TOOLS",
    "Read,Write,Edit,Glob,Grep,Bash(ls:*),Bash(cat:*),Bash(mkdir:*),Bash(python3:*),Bash(node:*),Bash(npm:*)",
)
OK_MARKER = os.path.join(".git", "nexc-turn-ok")


def send(message):
    sys.stdout.write(json.dumps(message) + "\n")
    sys.stdout.flush()


def run_turn(prompt, cwd, session_id, first_turn):
    """Runs one `claude -p` turn; returns (result, error)."""
    command = [
        CLAUDE, "-p", prompt,
        "--output-format", "json",
        "--model", MODEL,
        "--permission-mode", "acceptEdits",
        "--allowedTools", TOOLS,
        # Project settings only: the user's own hooks, plugins and MCP servers stay out of the run.
        "--setting-sources", "project",
        "--strict-mcp-config",
    ]
    command += ["--session-id", session_id] if first_turn else ["--resume", session_id]
    try:
        done = subprocess.run(command, cwd=cwd, capture_output=True, text=True, check=False)
    except OSError as error:
        return None, f"cannot run {CLAUDE}: {error}"
    try:
        result = json.loads(done.stdout)
    except ValueError:
        return None, f"claude exited {done.returncode}: {(done.stderr or done.stdout)[-500:]}"
    if done.returncode != 0 or result.get("is_error"):
        return result, str(result.get("result") or result.get("subtype") or "claude failed")
    return result, None


def spent(result):
    usage = (result or {}).get("usage") or {}
    read = sum(usage.get(key) or 0 for key in ("input_tokens", "cache_creation_input_tokens", "cache_read_input_tokens"))
    return read, usage.get("output_tokens") or 0


def mark_ok(cwd):
    marker = os.path.join(cwd, OK_MARKER)
    if os.path.isdir(os.path.dirname(marker)):
        open(marker, "w").close()


def main():
    thread_id = str(uuid.uuid4())
    cwd = os.getcwd()
    totals = {"inputTokens": 0, "outputTokens": 0, "totalTokens": 0}
    turns = 0
    for line in sys.stdin:
        try:
            message = json.loads(line)
        except ValueError:
            continue
        method, request_id, params = message.get("method"), message.get("id"), message.get("params") or {}
        if method == "initialize":
            send({"id": request_id, "result": {"userAgent": "nexc-symphony-claude-agent"}})
        elif method == "thread/start":
            cwd = params.get("cwd") or cwd
            send({"id": request_id, "result": {"thread": {"id": thread_id}}})
        elif method == "turn/start":
            turn_id = str(uuid.uuid4())
            send({"id": request_id, "result": {"turn": {"id": turn_id}}})
            prompt = "\n".join(item.get("text", "") for item in params.get("input", []) if item.get("type") == "text")
            turn_cwd = params.get("cwd") or cwd
            result, error = run_turn(prompt, turn_cwd, thread_id, turns == 0)
            turns += 1
            read, written = spent(result)
            totals["inputTokens"] += read
            totals["outputTokens"] += written
            totals["totalTokens"] += read + written
            send({"method": "thread/tokenUsage/updated",
                  "params": {"threadId": thread_id, "turnId": turn_id, "tokenUsage": {"total": totals}}})
            if error:
                send({"method": "turn/failed",
                      "params": {"threadId": thread_id, "turn": {"id": turn_id, "status": "failed"},
                                 "error": {"message": error}}})
                continue
            mark_ok(turn_cwd)
            text = str(result.get("result") or "")
            send({"method": "item/completed",
                  "params": {"threadId": thread_id, "turnId": turn_id,
                             "item": {"type": "agentMessage", "id": str(uuid.uuid4()), "text": text}}})
            send({"method": "turn/completed",
                  "params": {"threadId": thread_id, "turn": {"id": turn_id, "status": "completed"}}})
        elif request_id is not None and method is not None:
            send({"id": request_id, "result": {}})


if __name__ == "__main__":
    main()
