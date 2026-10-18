"""Messages of the gRPC wire ⇄ the objects of the runner: the execute request as the dict the
NDJSON body would be (validated the same way), and the events of the `EventStream` as
`RuntimeEvent` messages."""

from __future__ import annotations

from typing import Any

from google.protobuf import json_format
from google.protobuf.struct_pb2 import Struct

from . import runtime_pb2 as pb


def _normalise(value: Any) -> Any:
    """`Struct` numbers are doubles: whole ones come back as the integers they were."""
    if isinstance(value, float) and value.is_integer() and abs(value) < 2**53:
        return int(value)
    if isinstance(value, dict):
        return {k: _normalise(v) for k, v in value.items()}
    if isinstance(value, list):
        return [_normalise(v) for v in value]
    return value


def struct_to_dict(struct: Struct) -> dict[str, Any]:
    value = _normalise(json_format.MessageToDict(struct))
    return value if isinstance(value, dict) else {}


def dict_to_struct(value: dict[str, Any]) -> Struct:
    struct = Struct()
    json_format.ParseDict(_jsonable(value), struct)
    return struct


def _jsonable(value: Any) -> Any:
    """What a `Struct` can hold: anything else becomes its text."""
    if isinstance(value, dict):
        return {str(k): _jsonable(v) for k, v in value.items()}
    if isinstance(value, list | tuple):
        return [_jsonable(v) for v in value]
    if value is None or isinstance(value, bool | int | float | str):
        return value
    return str(value)


def execute_to_dict(message: pb.ExecuteRequest) -> dict[str, Any]:
    """The execute request as the JSON body of `POST /v1/execute` (CONTRACT section 8)."""
    task: dict[str, Any] = {
        "title": message.task.title,
        "content": message.task.content,
        "kind": message.task.kind or "task",
        "kind_description": message.task.kind_description,
        "produces_artifact": (
            message.task.produces_artifact if message.task.HasField("produces_artifact") else None
        ),
    }
    return {
        "run_id": message.run_id,
        "node_id": message.node_id,
        "token": message.token or None,
        "agent": {
            "name": message.agent.name or "agent",
            "role": message.agent.role or "agent",
            "principal": message.agent.principal,
            "system_prompt": message.agent.system_prompt,
            "model": message.agent.model or None,
            "budget_tokens": message.agent.budget_tokens or 200_000,
        },
        "task": task,
        "context": {
            "goal": message.context.goal,
            "upstream": [
                {"node_id": u.node_id, "title": u.title, "output": u.output}
                for u in message.context.upstream
            ],
            "memories": list(message.context.memories),
            "documents": list(message.context.documents),
            "labels": list(message.context.labels),
        },
        "llm": {
            "provider": message.llm.provider or "anthropic",
            "api_key": message.llm.api_key or None,
            "model": message.llm.model or None,
            "base_url": message.llm.base_url or None,
        },
        "limits": {
            "max_turns": message.limits.max_turns or 12,
            "timeout_s": message.limits.timeout_s or 600,
            "allow_code_exec": message.limits.allow_code_exec,
        },
        "gateway_tools": [
            {
                "name": t.name,
                "description": t.description,
                "input_schema": struct_to_dict(t.input_schema) or {"type": "object"},
            }
            for t in message.gateway_tools
        ],
    }


def event_to_message(event: dict[str, Any]) -> pb.RuntimeEvent:
    """One `EventStream` event as its message."""
    kind = event["type"]
    if kind == "log":
        return pb.RuntimeEvent(log=pb.Log(level=event["level"], message=event["message"]))
    if kind == "delta":
        return pb.RuntimeEvent(delta=pb.Delta(text=event["text"]))
    if kind == "tokens":
        return pb.RuntimeEvent(
            tokens=pb.Tokens(input=event["input"], output=event["output"], cached=event["cached"])
        )
    if kind == "spawn":
        return pb.RuntimeEvent(
            spawn=pb.Spawn(name=event["agent"]["name"], role=event["agent"]["role"])
        )
    if kind == "span":
        return pb.RuntimeEvent(
            span=pb.Span(
                id=event["id"],
                parent_id=event["parent_id"] or "",
                kind=event["kind"],
                name=event["name"],
                agent=event["agent"],
                started_at=event["started_at"],
                ended_at=event["ended_at"] or "",
                status=event["status"],
                attributes=dict_to_struct(event["attributes"]),
            )
        )
    if kind == "artifact":
        return pb.RuntimeEvent(
            artifact=pb.Artifact(path=event["path"], mime=event["mime"], content=event["content"])
        )
    if kind == "approval_request":
        return pb.RuntimeEvent(
            approval_request=pb.ApprovalRequest(
                approval_id=event["approval_id"],
                tool=event["tool"],
                rule=event["rule"],
                reason=event["reason"],
                args_sha256=event["args_sha256"],
                span_id=event["span_id"],
                agent=event["agent"],
                turn_span_id=event["turn_span_id"] or "",
            )
        )
    if kind == "tool_request":
        return pb.RuntimeEvent(
            tool_request=pb.ToolRequest(
                request_id=event["request_id"],
                tool=event["tool"],
                arguments=dict_to_struct(event["arguments"]),
                agent=event["agent"],
                turn_span_id=event["turn_span_id"] or "",
            )
        )
    if kind == "payload":
        return pb.RuntimeEvent(
            payload=pb.Payload(
                span_id=event["span_id"], kind=event["kind"], content=event["content"]
            )
        )
    if kind == "result":
        return pb.RuntimeEvent(
            result=pb.Result(
                output=event["output"],
                tokens_in=event["tokens_in"],
                tokens_out=event["tokens_out"],
                tokens_cached=event["tokens_cached"],
            )
        )
    if kind == "error":
        return pb.RuntimeEvent(
            error=pb.Error(message=event["message"], retryable=event["retryable"])
        )
    raise ValueError(f"unknown event type {kind!r}")
