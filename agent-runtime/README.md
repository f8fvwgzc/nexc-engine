# nexc agent runtime

The Python service that executes `executor: agent` nodes for the nexc-engine backend. For every
request an agent is **born** from the spec in the request (role persona, system prompt, model,
token budget), runs a tool loop inside a private workspace, may **spawn sub-agents** with a share
of its budget, and streams progress back as NDJSON. Every file left in the workspace is returned
as an artifact.

It is an internal service: only the backend talks to it, authenticated with `NEXC_RUNTIME_TOKEN`.
The protocol is specified in [docs/CONTRACT.md §8](../docs/CONTRACT.md#8-agent-runtime-internal-api-backend--runtime-only).

## Run it

```sh
uv sync                                    # installs Python 3.12 and the locked dependencies
NEXC_RUNTIME_TOKEN=$(openssl rand -hex 32) uv run nexc-runtime   # or: python -m nexc_runtime
```

From the repo root `make dev-runtime` loads `.env` for you. Container: `docker build -t nexc-runtime .`
(non-root, works with a read-only root filesystem and a tmpfs at `/workspace`).

```sh
curl -s localhost:8090/healthz
# {"status":"ok","version":"0.1.0","agents_loaded":0}

curl -sN localhost:8090/v1/execute \
  -H "Authorization: Bearer $NEXC_RUNTIME_TOKEN" -H 'content-type: application/json' \
  -d '{"run_id":"r1","node_id":"n1","task":{"title":"Launch plan","kind":"document"},
       "llm":{"provider":"demo"}}'
```

## Configuration

| Variable | Default | Meaning |
|---|---|---|
| `NEXC_RUNTIME_TOKEN` | – (required, ≥ 32 chars) | shared bearer secret with the backend |
| `RUNTIME_HOST` / `RUNTIME_PORT` | `0.0.0.0` / `8090` | bind address |
| `RUNTIME_WORKSPACE` | `/tmp/nexc-runtime` | parent of the per-run scratch directories |
| `RUNTIME_ALLOW_CODE_EXEC` | `false` | registers the sandboxed `run_python` tool (the request must also set `limits.allow_code_exec`) |
| `NEXC_LLM_MODEL` | `claude-opus-5` | model when the request names none |
| `RUNTIME_MAX_CONCURRENT_RUNS` | `8` | further requests get `429` |
| `RUNTIME_MAX_TURNS_CAP` / `RUNTIME_TIMEOUT_CAP_S` | `40` / `1800` | upper bounds for the request's `limits` |
| `RUNTIME_MAX_REQUEST_BYTES` | `4194304` | request body limit (`413` above) |
| `RUNTIME_MAX_FILE_BYTES` / `RUNTIME_MAX_WORKSPACE_BYTES` / `RUNTIME_MAX_ARTIFACTS` | 20 MiB / 100 MiB / 64 | workspace quotas |
| `RUNTIME_PYTHON_TIMEOUT_S` / `RUNTIME_PYTHON_MEMORY_MB` | `60` / `512` | `run_python` limits |
| `RUNTIME_KEEP_WORKSPACES` | `false` | keep run directories for debugging |
| `RUNTIME_LLM_MAX_RETRIES` | `2` | SDK-level retries before an error is reported |
| `RUNTIME_LOG_LEVEL` | `info` | JSON logs on stderr |

## How a request runs

1. The request is validated (pydantic); the API key is a `SecretStr`, is never logged or stored,
   and is stripped from every error message. Validation errors never echo input.
2. A fresh workspace is created with `mkdtemp`; the provider is chosen from `llm.provider`:
   * `anthropic` – official SDK, `client.beta.messages.stream(...)` + `get_final_message()`,
     adaptive thinking, strict tool schemas with eager input streaming, server-side refusal
     fallback (`fallbacks="default"`) on `claude-opus-5`. `stop_reason` is checked before content
     is used; tools never run on a refused or truncated turn. Errors map to
     `retryable` (429, 5xx, connection) or not (other 4xx).
   * `openai_compatible` – streaming Chat Completions over httpx (Ollama, vLLM, LM Studio...).
   * `demo` – deterministic and offline; drives the real tool loop (a real `.docx` for document
     nodes) and labels its output as demo content.
3. The root agent is born and loops: model turn → all tool calls of the turn → **one** message with
   all `tool_result`s (failures are `is_error` results the model can react to) → … until it calls
   `finish`, ends its turn, exhausts `max_turns` or its budget.
4. Artifacts (every regular file, symlinks ignored, ≤ 20 MiB, sanitised paths, MIME type) are
   emitted, then exactly one `result`. Any failure ends the stream with exactly one `error`.
5. The workspace is deleted.

### Tools

| Tool | Notes |
|---|---|
| `write_file(path, content)` | quotas per file and per workspace; never follows symlinks |
| `read_file(path)` / `list_files(path)` | read/list inside the workspace only |
| `make_docx(filename, title, sections[{heading, paragraphs[], bullets[], table_rows[][]}])` | real Word document via python-docx |
| `spawn_subagent(role, task)` | child gets half of the remaining budget; max depth 2, max 4 children per agent |
| `run_python(code)` | only when enabled: `python -I`, cwd = workspace, scrubbed env, rlimits (CPU, address space, file size, processes, files), timeout kills the process group, output truncated |
| `finish(answer)` | final answer of the agent |

Every path goes through `safe_path`, which rejects empty, absolute and drive-letter paths, `..`,
NUL bytes, backslashes and anything that resolves outside the workspace through symlinks.

## Develop

```sh
uv run pytest              # fake LLM provider, no network
uv run ruff check && uv run ruff format --check
uv run mypy src tests      # strict
```

Layout:

```text
src/nexc_runtime/
  main.py, __main__.py   entry points (`nexc-runtime`, `python -m nexc_runtime`)
  config.py              pydantic-settings
  api/                   FastAPI app, schemas, bearer auth, body-size limit
  runner.py              one /v1/execute run: workspace -> agents -> artifacts -> terminal event
  agents/                AgentSpec, Agent (tool loop), AgentRegistry, Budget
  llm/                   anthropic, openai_compatible and demo providers
  tools/                 workspace + safe_path, file tools, make_docx, run_python, agent tools
  prompts.py             personas, system and task prompts
  streaming.py           NDJSON event stream and artifact collection
tests/                   fake provider, API, tools, agents, demo and Anthropic (mock transport)
```
