"""Runtime configuration, read from the environment (see the repo-root `.env.example`)."""

from __future__ import annotations

from functools import lru_cache
from pathlib import Path
from typing import Literal

from pydantic import Field, SecretStr, field_validator
from pydantic_settings import BaseSettings, SettingsConfigDict

MIN_TOKEN_LENGTH = 32


class Settings(BaseSettings):
    """All knobs of the agent runtime.

    Field names map to upper-case environment variables (`runtime_port` -> `RUNTIME_PORT`).
    """

    model_config = SettingsConfigDict(case_sensitive=False, extra="ignore", frozen=True)

    # --- network -----------------------------------------------------------------------------
    runtime_host: str = "0.0.0.0"  # noqa: S104 - the container binds every interface on purpose
    runtime_port: int = Field(default=8090, ge=1, le=65535)

    # --- security ----------------------------------------------------------------------------
    nexc_runtime_token: SecretStr = Field(
        description="Shared bearer secret between the backend and this runtime (>= 32 chars).",
    )
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


@lru_cache(maxsize=1)
def get_settings() -> Settings:
    return Settings()  # values come from the environment
