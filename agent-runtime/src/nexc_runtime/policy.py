"""The per-run agent token and the in-process policy it carries (CONTRACT section 8).

The backend mints a signed token for every node it dispatches: the agent's principal, the run
and node, a snapshot of the agent's capabilities and the policy version. This runtime refuses
to start without a valid one, decides every tool call and spawn from it in microseconds without
leaving the process, and hands sub-agents a narrowed copy. The backend re-checks every span it
receives against the same capabilities, so nothing here is the last line of defence; it is the
first, and the one the model sees.

The rules mirror `backend/src/domain/policy.rs` and `capabilities.rs` exactly: the backend
flags any decision that disagrees with its own as a `policy_mismatch`.
"""

from __future__ import annotations

import base64
import hashlib
import hmac
import json
import os
import time
from collections.abc import Mapping
from dataclasses import dataclass
from typing import Any

ISSUER = "nexc-engine"
AUDIENCE = "nexc-runtime"
# Clock skew tolerated on `exp` and `nbf`, as the backend tolerates it.
LEEWAY_S = 5
# Prefix of the key derivation from the runtime's shared secret (see `security/agent_token.rs`).
DERIVATION_TAG = b"nexc-agent-token-v1:"

LABELS = ("public", "internal", "confidential", "restricted")
MEMORY_NAMESPACES = ("user", "graph", "node")
# Tools whose output leaves the run as an artifact: the ones a label-based approval holds.
OUTBOUND_TOOLS = ("write_file", "make_docx")
APPROVAL_TIMEOUT_DEFAULT_S = 600


class TokenError(Exception):
    """The token is missing, forged, expired or for another run."""


def derive_key(runtime_token: str) -> bytes:
    """The HMAC key both sides use when no `NEXC_AGENT_TOKEN_KEY` is configured."""
    return hashlib.sha256(DERIVATION_TAG + runtime_token.encode("utf-8")).digest()


# --- capabilities -------------------------------------------------------------------------------


def _meet(a: tuple[str, ...] | None, b: tuple[str, ...] | None) -> tuple[str, ...] | None:
    """Whatever both allow; `None` restricts nothing."""
    if a is None:
        return b
    if b is None:
        return a
    return tuple(x for x in a if x in b)


def _strings(
    raw: Mapping[str, Any], key: str, default: tuple[str, ...] | None
) -> tuple[str, ...] | None:
    value = raw.get(key, default)
    if value is None:
        return None
    if not isinstance(value, list | tuple) or not all(isinstance(v, str) for v in value):
        raise TokenError(f"capabilities.{key} is not a list of strings")
    return tuple(value)


def _as_list(value: tuple[str, ...] | None) -> list[str] | None:
    return None if value is None else list(value)


def _small(raw: Mapping[str, Any], key: str, default: int) -> int:
    value = raw.get(key, default)
    if isinstance(value, bool) or not isinstance(value, int) or not 0 <= value <= 255:
        raise TokenError(f"capabilities.delegation.{key} is not a small integer")
    return value


@dataclass(frozen=True, slots=True)
class Capabilities:
    """What an agent may do. `None` on a list means no restriction."""

    tools: tuple[str, ...] | None = None
    topics: tuple[str, ...] | None = None
    labels: tuple[str, ...] = ("public", "internal")
    memory_namespaces: tuple[str, ...] | None = None
    egress_hosts: tuple[str, ...] | None = None
    counterparts: tuple[str, ...] | None = None
    max_depth: int = 2
    max_fan_out: int = 4
    models: tuple[str, ...] | None = None

    @classmethod
    def from_dict(cls, raw: Mapping[str, Any] | None) -> Capabilities:
        """The document as the backend stores it (`{}` is the default policy)."""
        raw = raw or {}
        if not isinstance(raw, Mapping):
            raise TokenError("capabilities is not an object")
        data = raw.get("data") or {}
        delegation = raw.get("delegation") or {}
        if not isinstance(data, Mapping) or not isinstance(delegation, Mapping):
            raise TokenError("capabilities.data or .delegation is not an object")
        labels = _strings(data, "labels", ("public", "internal"))
        return cls(
            tools=_strings(raw, "tools", None),
            topics=_strings(data, "topics", None),
            labels=labels if labels is not None else (),
            memory_namespaces=_strings(data, "memory_namespaces", None),
            egress_hosts=_strings(raw, "egress_hosts", None),
            counterparts=_strings(raw, "counterparts", None),
            max_depth=_small(delegation, "max_depth", 2),
            max_fan_out=_small(delegation, "max_fan_out", 4),
            models=_strings(raw, "models", None),
        )

    def to_dict(self) -> dict[str, Any]:
        """The same shape, for a token this runtime mints for a sub-agent."""
        return {
            "tools": _as_list(self.tools),
            "data": {
                "topics": _as_list(self.topics),
                "labels": list(self.labels),
                "memory_namespaces": _as_list(self.memory_namespaces),
            },
            "egress_hosts": _as_list(self.egress_hosts),
            "counterparts": _as_list(self.counterparts),
            "delegation": {"max_depth": self.max_depth, "max_fan_out": self.max_fan_out},
            "models": _as_list(self.models),
        }

    def narrow(self, child: Capabilities) -> Capabilities:
        """What `child` ends up with under `self`: never more than either allows."""
        return Capabilities(
            tools=_meet(self.tools, child.tools),
            topics=_meet(self.topics, child.topics),
            labels=tuple(label for label in self.labels if label in child.labels),
            memory_namespaces=_meet(self.memory_namespaces, child.memory_namespaces),
            egress_hosts=_meet(self.egress_hosts, child.egress_hosts),
            counterparts=_meet(self.counterparts, child.counterparts),
            max_depth=min(self.max_depth, child.max_depth),
            max_fan_out=min(self.max_fan_out, child.max_fan_out),
            models=_meet(self.models, child.models),
        )

    def is_within(self, other: Capabilities) -> bool:
        """Whether every permission here is also in `other`."""
        return other.narrow(self) == self


# --- rules --------------------------------------------------------------------------------------


def _small_or_none(raw: Mapping[str, Any], key: str) -> int | None:
    value = raw.get(key)
    if value is None:
        return None
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise TokenError(f"rules.{key} is not a whole number")
    return value


@dataclass(frozen=True, slots=True)
class Rules:
    """What the resolved policy says beyond the capabilities, as far as this runtime enforces
    it: deny-lists, which calls wait for a human, the ceilings per run, and what the
    prompt-injection screen does. Mirrors `backend/src/domain/policy.rs::Rules`."""

    tools_denied: tuple[str, ...] = ()
    egress_denied: tuple[str, ...] = ()
    models_denied: tuple[str, ...] = ()
    approval_tools: tuple[str, ...] = ()
    approval_labels: tuple[str, ...] = ()
    approval_timeout_s: int = APPROVAL_TIMEOUT_DEFAULT_S
    tokens_per_run: int | None = None
    tool_calls_per_minute: int | None = None
    injection_action: str = "flag"
    injection_model_check: bool = False

    @classmethod
    def from_dict(cls, raw: Mapping[str, Any] | None) -> Rules:
        """The rules as the backend minted them; a token without any reads as the defaults."""
        raw = raw or {}
        if not isinstance(raw, Mapping):
            raise TokenError("rules is not an object")
        approval = raw.get("require_approval") or {}
        limits = raw.get("limits") or {}
        injection = raw.get("injection") or {}
        if not all(isinstance(part, Mapping) for part in (approval, limits, injection)):
            raise TokenError("rules.require_approval, .limits or .injection is not an object")
        timeout = _small_or_none(approval, "timeout_s")
        action = injection.get("action", "flag")
        if action not in ("flag", "block"):
            raise TokenError("rules.injection.action is not flag or block")
        return cls(
            tools_denied=_strings(raw, "tools_denied", ()) or (),
            egress_denied=_strings(raw, "egress_denied", ()) or (),
            models_denied=_strings(raw, "models_denied", ()) or (),
            approval_tools=_strings(approval, "tools", ()) or (),
            approval_labels=_strings(approval, "labels", ()) or (),
            approval_timeout_s=timeout if timeout else APPROVAL_TIMEOUT_DEFAULT_S,
            tokens_per_run=_small_or_none(limits, "tokens_per_run"),
            tool_calls_per_minute=_small_or_none(limits, "tool_calls_per_minute"),
            injection_action=action,
            injection_model_check=bool(injection.get("model_check", False)),
        )


# --- decisions ----------------------------------------------------------------------------------


@dataclass(frozen=True, slots=True)
class Decision:
    allowed: bool
    rule: str
    reason: str
    # "" means: as `allowed` says; "require_approval" holds the action for a human.
    effect: str = ""

    @property
    def verdict(self) -> str:
        if self.effect:
            return self.effect
        return "allow" if self.allowed else "deny"

    @property
    def needs_approval(self) -> bool:
        return self.effect == "require_approval"


def tool_matches(listed: tuple[str, ...], tool: str) -> bool:
    """Whether `tool` is one of `listed`: exact, or every tool of an MCP server under
    `mcp:<server>/*` (mirrors `policy::tool_matches` in the backend)."""
    for entry in listed:
        if entry == tool:
            return True
        if entry.startswith("mcp:") and entry.endswith("/*") and tool.startswith("mcp:"):
            server = entry[len("mcp:") : -len("/*")]
            rest = tool[len("mcp:") :]
            if "/" in rest and rest.split("/", 1)[0] == server:
                return True
    return False


def decide_tool(caps: Capabilities, name: str) -> Decision:
    if caps.tools is None:
        return Decision(True, "tools.allow", "no tool restriction")
    if tool_matches(caps.tools, name):
        return Decision(True, "tools.allow", f"{name} is in the allow-list")
    return Decision(False, "tools.allow", f"tool {name} is not allowed")


def decide_spawn(caps: Capabilities, role: str, *, depth: int, children: int) -> Decision:
    """`depth` is the child's (the root is 0); `children` how many were spawned already."""
    if depth > caps.max_depth:
        return Decision(
            False, "delegation.max_depth", f"depth {depth} exceeds the limit of {caps.max_depth}"
        )
    if children >= caps.max_fan_out:
        return Decision(
            False, "delegation.max_fan_out", f"already spawned {caps.max_fan_out} sub-agents"
        )
    if caps.counterparts is not None and role not in caps.counterparts:
        return Decision(False, "counterparts", f"role {role} is not a counterpart")
    return Decision(True, "delegation", f"may spawn a {role} at depth {depth}")


def decide_tool_with(
    caps: Capabilities, rules: Rules, labels: tuple[str, ...], name: str
) -> Decision:
    """A deny-list refuses first, then the capabilities, then an approval rule holds the call.
    `labels` are the classifications of the data the node was given."""
    if tool_matches(rules.tools_denied, name):
        return Decision(False, "tools.deny", f"tool {name} is denied by policy")
    base = decide_tool(caps, name)
    if not base.allowed:
        return base
    if tool_matches(rules.approval_tools, name):
        return Decision(
            False, "approvals.tools", f"tool {name} needs an approval", effect="require_approval"
        )
    if name in OUTBOUND_TOOLS:
        held = next((label for label in labels if label in rules.approval_labels), None)
        if held is not None:
            return Decision(
                False,
                "approvals.labels",
                f"the node read {held} data; {name} needs an approval",
                effect="require_approval",
            )
    return base


def decide_model_with(caps: Capabilities, rules: Rules, model: str) -> Decision:
    if model in rules.models_denied:
        return Decision(False, "models.deny", f"model {model} is denied by policy")
    return decide_model(caps, model)


def decide_model(caps: Capabilities, model: str) -> Decision:
    if caps.models is None:
        return Decision(True, "models.allow", "no model restriction")
    if model in caps.models:
        return Decision(True, "models.allow", f"{model} is in the allow-list")
    return Decision(False, "models.allow", f"model {model} is not allowed")


def _host_allowed(allowed: tuple[str, ...], host: str) -> bool:
    host = host.lower()
    for raw in allowed:
        entry = raw.lower()
        if entry.startswith("*."):
            if host.endswith("." + entry[2:]):
                return True
        elif entry == host:
            return True
    return False


def decide_egress(caps: Capabilities, host: str) -> Decision:
    if caps.egress_hosts is None:
        return Decision(True, "egress.hosts", "no host restriction")
    if _host_allowed(caps.egress_hosts, host):
        return Decision(True, "egress.hosts", f"{host} is in the allow-list")
    return Decision(False, "egress.hosts", f"host {host} is not allowed")


# --- tokens -------------------------------------------------------------------------------------


def _b64url(data: bytes) -> str:
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode("ascii")


def _unb64url(text: str) -> bytes:
    try:
        return base64.urlsafe_b64decode(text + "=" * (-len(text) % 4))
    except (ValueError, TypeError) as exc:
        raise TokenError("token is not base64url") from exc


def encode_token(claims: Mapping[str, Any], key: bytes) -> str:
    """An HS256 JWT of `claims` (what the backend produces; used here for sub-agents and tests)."""
    header = _b64url(json.dumps({"typ": "JWT", "alg": "HS256"}, separators=(",", ":")).encode())
    payload = _b64url(json.dumps(dict(claims), separators=(",", ":")).encode("utf-8"))
    signed = f"{header}.{payload}".encode("ascii")
    signature = _b64url(hmac.new(key, signed, hashlib.sha256).digest())
    return f"{header}.{payload}.{signature}"


@dataclass(frozen=True, slots=True)
class AgentToken:
    """A verified token: what one principal may do during one node of one run."""

    principal: str
    run_id: str
    node_id: str
    depth: int
    policy_version: int
    capabilities: Capabilities
    expires_at: int
    claims: Mapping[str, Any]
    raw: str
    # The resolved rules beside the capabilities, and the labels of the data the node was given.
    rules: Rules = Rules()
    labels: tuple[str, ...] = ()

    def child(self, principal: str, key: bytes, *, now: float | None = None) -> AgentToken:
        """A token for a sub-agent: the same run and node, one level deeper, and never wider
        than this one (its capabilities are this token's narrowed by themselves)."""
        now_s = int(time.time() if now is None else now)
        capabilities = self.capabilities.narrow(self.capabilities)
        claims = {
            **self.claims,
            "sub": principal,
            "jti": _new_jti(),
            "iat": now_s,
            "nbf": now_s,
            "depth": self.depth + 1,
            "capabilities": capabilities.to_dict(),
        }
        return AgentToken(
            principal=principal,
            run_id=self.run_id,
            node_id=self.node_id,
            depth=self.depth + 1,
            policy_version=self.policy_version,
            capabilities=capabilities,
            expires_at=self.expires_at,
            claims=claims,
            raw=encode_token(claims, key),
            rules=self.rules,
            labels=self.labels,
        )


def _new_jti() -> str:
    return _b64url(os.urandom(16))


def verify_token(
    token: str | None,
    key: bytes,
    *,
    run_id: str,
    node_id: str,
    now: float | None = None,
) -> AgentToken:
    """Checks the signature (constant time), the algorithm, issuer, audience, times and that the
    token was minted for this run and node. Raises `TokenError` with the reason."""
    if not token:
        raise TokenError("no agent token in the request")
    parts = token.split(".")
    if len(parts) != 3:
        raise TokenError("token is not a JWT")
    header_b64, payload_b64, signature_b64 = parts
    try:
        header = json.loads(_unb64url(header_b64))
    except ValueError as exc:
        raise TokenError("token header is not JSON") from exc
    if not isinstance(header, dict) or header.get("alg") != "HS256":
        raise TokenError("token algorithm is not HS256")
    expected = hmac.new(key, f"{header_b64}.{payload_b64}".encode("ascii"), hashlib.sha256)
    if not hmac.compare_digest(_unb64url(signature_b64), expected.digest()):
        raise TokenError("token signature does not verify")
    try:
        claims = json.loads(_unb64url(payload_b64))
    except ValueError as exc:
        raise TokenError("token payload is not JSON") from exc
    if not isinstance(claims, dict):
        raise TokenError("token payload is not an object")
    if claims.get("iss") != ISSUER or claims.get("aud") != AUDIENCE:
        raise TokenError("token is not for this runtime")
    now_s = time.time() if now is None else now
    exp, nbf = claims.get("exp"), claims.get("nbf")
    if not isinstance(exp, int | float) or exp + LEEWAY_S < now_s:
        raise TokenError("token has expired")
    if isinstance(nbf, int | float) and nbf - LEEWAY_S > now_s:
        raise TokenError("token is not valid yet")
    if claims.get("run_id") != run_id or claims.get("node_id") != node_id:
        raise TokenError("token was minted for another run or node")
    principal = claims.get("sub")
    if not isinstance(principal, str) or not principal:
        raise TokenError("token names no principal")
    depth = claims.get("depth", 0)
    version = claims.get("policy_version", 0)
    if not isinstance(depth, int) or not isinstance(version, int):
        raise TokenError("token depth or policy version is not an integer")
    labels = claims.get("labels") or []
    if not isinstance(labels, list) or not all(isinstance(label, str) for label in labels):
        raise TokenError("token labels are not a list of strings")
    return AgentToken(
        principal=principal,
        run_id=run_id,
        node_id=node_id,
        depth=depth,
        policy_version=version,
        capabilities=Capabilities.from_dict(claims.get("capabilities")),
        expires_at=int(exp),
        claims=claims,
        raw=token,
        rules=Rules.from_dict(claims.get("rules")),
        labels=tuple(labels),
    )
