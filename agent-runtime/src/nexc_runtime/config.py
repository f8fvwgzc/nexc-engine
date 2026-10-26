"""Runtime configuration, read from the environment (see the repo-root `.env.example`)."""

from __future__ import annotations

import ssl
from functools import lru_cache
from pathlib import Path
from typing import Literal

from pydantic import Field, SecretStr, field_validator, model_validator
from pydantic_settings import BaseSettings, SettingsConfigDict

from .keys import BadKey, KeyRing, parse_public_key

MIN_TOKEN_LENGTH = 32


class Settings(BaseSettings):
    """All knobs of the agent runtime.

    Field names map to upper-case environment variables (`runtime_port` -> `RUNTIME_PORT`).
    """

    # An empty variable (`NEXC_AGENT_TOKEN_PUBLIC_KEYS=` in `.env`) counts as unset.
    model_config = SettingsConfigDict(
        case_sensitive=False, extra="ignore", frozen=True, env_ignore_empty=True
    )

    # --- network -----------------------------------------------------------------------------
    runtime_host: str = "0.0.0.0"  # noqa: S104 - the container binds every interface on purpose
    runtime_port: int = Field(default=8090, ge=1, le=65535)
    # The gRPC stream (`AgentRuntime/Execute`), on the same host; 0 turns it off.
    runtime_grpc_port: int = Field(default=8095, ge=0, le=65535)
    # TLS of the gRPC stream, which carries the LLM keys of workspaces that bring their own (PEM
    # files): the certificate and key it serves, and the CA whose client certificates it then
    # requires (the backend's NEXC_GRPC_CLIENT_CERT_FILE). Plaintext when unset, for a hop that a
    # network policy or a mesh keeps private.
    runtime_grpc_tls_cert_file: Path | None = None
    runtime_grpc_tls_key_file: Path | None = None
    runtime_grpc_tls_client_ca_file: Path | None = None

    # --- security ----------------------------------------------------------------------------
    nexc_runtime_token: SecretStr = Field(
        description="Shared bearer secret between the backend and this runtime (>= 32 chars).",
    )
    # The per-run agent tokens are signed by the backend alone (EdDSA); this runtime gets public
    # keys only: pinned here (base64url Ed25519 keys, comma separated), or fetched from the
    # backend's key set. Exactly one of the two.
    nexc_agent_token_public_keys: str | None = Field(
        default=None,
        description="Ed25519 public keys of the agent tokens (base64url, comma separated).",
    )
    nexc_agent_token_jwks_url: str | None = Field(
        default=None,
        description="The backend's key set: http://backend:8080/api/v1/agent-tokens/jwks.json",
    )
    # The HS256 key the backend and this runtime shared before: refused by name, so an old
    # deployment learns it is gone instead of running with a secret nothing reads.
    nexc_agent_token_key: SecretStr | None = Field(default=None, exclude=True, repr=False)
    runtime_max_request_bytes: int = Field(default=4 * 1024 * 1024, ge=1024)
    # /v1/parse takes a whole uploaded file as its body, so it has its own, larger limit.
    runtime_max_parse_bytes: int = Field(default=50 * 1024 * 1024, ge=1024)

    # --- execution ---------------------------------------------------------------------------
    runtime_workspace: Path = Path("/tmp/nexc-runtime")  # noqa: S108 - per-run dirs are mkdtemp'd
    runtime_keep_workspaces: bool = False
    runtime_allow_code_exec: bool = False
    runtime_max_concurrent_runs: int = Field(default=8, ge=1, le=256)
    runtime_max_turns_cap: int = Field(default=40, ge=1, le=200)
    runtime_timeout_cap_s: int = Field(default=1800, ge=10)
    runtime_python_timeout_s: int = Field(default=60, ge=1, le=600)
    runtime_python_memory_mb: int = Field(default=512, ge=64)
    # Lets a policy's `injection.model_check` ask the run's model to confirm a finding.
    runtime_injection_model_check: bool = False

    # --- workspace / artifact limits ---------------------------------------------------------
    runtime_max_file_bytes: int = Field(default=20 * 1024 * 1024, ge=1024)
    runtime_max_workspace_bytes: int = Field(default=100 * 1024 * 1024, ge=1024)
    runtime_max_artifacts: int = Field(default=64, ge=1)

    # --- LLM defaults ------------------------------------------------------------------------
    nexc_llm_model: str = "claude-opus-5"
    runtime_llm_max_retries: int = Field(default=2, ge=0, le=10)
    runtime_claude_bin: str = "claude"  # Claude Code CLI for the `claude_code` provider

    # --- logging -----------------------------------------------------------------------------
    runtime_log_level: Literal["debug", "info", "warning", "error"] = "info"

    @field_validator("nexc_runtime_token")
    @classmethod
    def _token_strength(cls, value: SecretStr) -> SecretStr:
        if len(value.get_secret_value()) < MIN_TOKEN_LENGTH:
            raise ValueError(
                f"must be at least {MIN_TOKEN_LENGTH} characters (generate one with `make init`)"
            )
        return value

    @field_validator("nexc_agent_token_public_keys")
    @classmethod
    def _public_keys(cls, value: str | None) -> str | None:
        for part in (value or "").split(","):
            if part.strip():
                try:
                    parse_public_key(part)
                except BadKey as exc:
                    raise ValueError(f"not an Ed25519 public key: {exc}") from None
        return value

    @field_validator("nexc_agent_token_jwks_url")
    @classmethod
    def _jwks_url(cls, value: str | None) -> str | None:
        if value is not None and not value.startswith(("http://", "https://")):
            raise ValueError("must be an http(s) URL")
        return value

    @model_validator(mode="after")
    def _agent_token_keys(self) -> Settings:
        if self.nexc_agent_token_key is not None:
            raise ValueError(
                "NEXC_AGENT_TOKEN_KEY is no longer read: agent tokens are signed with Ed25519 by "
                "the backend alone. Remove it and set NEXC_AGENT_TOKEN_JWKS_URL (or "
                "NEXC_AGENT_TOKEN_PUBLIC_KEYS); see the upgrade note in CHANGELOG.md"
            )
        if self.nexc_agent_token_public_keys and self.nexc_agent_token_jwks_url:
            raise ValueError(
                "set NEXC_AGENT_TOKEN_PUBLIC_KEYS or NEXC_AGENT_TOKEN_JWKS_URL, not both: pinned "
                "keys mean nothing if a key set fetched over the network may add others"
            )
        if not self.nexc_agent_token_public_keys and not self.nexc_agent_token_jwks_url:
            raise ValueError(
                "set NEXC_AGENT_TOKEN_JWKS_URL (the backend's "
                "/api/v1/agent-tokens/jwks.json) or NEXC_AGENT_TOKEN_PUBLIC_KEYS: without "
                "the backend's public keys no agent token verifies"
            )
        return self

    @model_validator(mode="after")
    def _grpc_tls(self) -> Settings:
        cert, key = self.runtime_grpc_tls_cert_file, self.runtime_grpc_tls_key_file
        if (cert is None) != (key is None):
            raise ValueError(
                "RUNTIME_GRPC_TLS_CERT_FILE and RUNTIME_GRPC_TLS_KEY_FILE must be set together"
            )
        if self.runtime_grpc_tls_client_ca_file is not None and cert is None:
            raise ValueError(
                "RUNTIME_GRPC_TLS_CLIENT_CA_FILE needs RUNTIME_GRPC_TLS_CERT_FILE and "
                "RUNTIME_GRPC_TLS_KEY_FILE"
            )
        for path in (cert, key, self.runtime_grpc_tls_client_ca_file):
            if path is not None and not path.is_file():
                raise ValueError(f"{path} is not a readable file")
        return self

    def grpc_ssl_context(self) -> ssl.SSLContext | None:
        """The TLS the gRPC stream serves (HTTP/2 by ALPN), or None for plaintext."""
        if self.runtime_grpc_tls_cert_file is None or self.runtime_grpc_tls_key_file is None:
            return None
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        context.minimum_version = ssl.TLSVersion.TLSv1_2
        context.load_cert_chain(self.runtime_grpc_tls_cert_file, self.runtime_grpc_tls_key_file)
        context.set_alpn_protocols(["h2"])
        if self.runtime_grpc_tls_client_ca_file is not None:
            context.verify_mode = ssl.CERT_REQUIRED
            context.load_verify_locations(cafile=self.runtime_grpc_tls_client_ca_file)
        return context

    def key_ring(self) -> KeyRing:
        """A fresh ring of the configured public keys and the key set URL."""
        return KeyRing.from_config(
            self.nexc_agent_token_public_keys, self.nexc_agent_token_jwks_url
        )


@lru_cache(maxsize=1)
def get_settings() -> Settings:
    return Settings()  # values come from the environment
