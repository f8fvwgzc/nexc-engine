# syntax=docker/dockerfile:1.7
# nexc agent runtime: python:3.12-slim + uv, non-root, read-only-root friendly.
# Build: docker build -t nexc-engine-runtime ./agent-runtime

ARG PYTHON_IMAGE=python:3.12-slim-bookworm

FROM ghcr.io/astral-sh/uv:0.12.22 AS uv

# ---------------------------------------------------------------------------------------------
FROM ${PYTHON_IMAGE} AS builder
COPY --from=uv /uv /usr/local/bin/uv
ENV UV_COMPILE_BYTECODE=1 \
    UV_LINK_MODE=copy \
    UV_PYTHON_DOWNLOADS=never \
    UV_PYTHON=python3.12 \
    UV_PROJECT_ENVIRONMENT=/app/.venv
WORKDIR /src

# Dependencies first (cached until pyproject.toml / uv.lock change).
RUN --mount=type=cache,target=/root/.cache/uv \
    --mount=type=bind,source=uv.lock,target=uv.lock \
    --mount=type=bind,source=pyproject.toml,target=pyproject.toml \
    uv sync --locked --no-dev --no-install-project

COPY pyproject.toml uv.lock README.md LICENSE ./
COPY src ./src
RUN --mount=type=cache,target=/root/.cache/uv \
    uv sync --locked --no-dev --no-editable

# ---------------------------------------------------------------------------------------------
FROM ${PYTHON_IMAGE} AS runtime
LABEL org.opencontainers.image.title="nexc-engine-runtime" \
      org.opencontainers.image.description="nexc-engine Python agent runtime" \
      org.opencontainers.image.source="https://github.com/f8fvwgzc/nexc-engine" \
      org.opencontainers.image.licenses="MIT"

RUN groupadd --system --gid 10001 nexc \
 && useradd --system --uid 10001 --gid nexc --no-create-home --home-dir /nonexistent \
        --shell /usr/sbin/nologin nexc \
 && install -d -o nexc -g nexc -m 0700 /workspace

COPY --from=builder /app/.venv /app/.venv

ENV PATH=/app/.venv/bin:$PATH \
    PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1 \
    RUNTIME_HOST=0.0.0.0 \
    RUNTIME_PORT=8090 \
    RUNTIME_WORKSPACE=/workspace

USER 10001:10001
WORKDIR /workspace
EXPOSE 8090
# Mount a tmpfs or volume at /workspace when running with a read-only root filesystem.

HEALTHCHECK --interval=15s --timeout=5s --start-period=10s --retries=3 \
  CMD ["python", "-c", "import os, urllib.request; urllib.request.urlopen('http://127.0.0.1:' + os.environ.get('RUNTIME_PORT', '8090') + '/healthz', timeout=3)"]

ENTRYPOINT ["nexc-runtime"]
