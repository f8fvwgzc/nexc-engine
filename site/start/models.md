# Claude and local models

This page lists the LLM providers nexc-engine plans and runs with, what each one needs, and where the setting lives.

<!--@include: ../../README.md#providers-->

## The providers

`NEXC_LLM_PROVIDER` in `.env` selects the server-wide provider and `ANTHROPIC_API_KEY` the server-wide key; a key a person stores under **Settings, AI accounts** wins for that person. Agents in the Python runtime use the same provider; the key travels with each request and the runtime never stores it.

| Provider          | `NEXC_LLM_PROVIDER` | Needs                                                                                            | Notes                                                                                                                                                                   |
| ----------------- | ------------------- | ------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Anthropic         | `anthropic`         | `ANTHROPIC_API_KEY`                                                                              | The default model is `claude-opus-5` (`NEXC_LLM_MODEL`), with adaptive thinking and server-side refusal fallbacks (`NEXC_LLM_FALLBACKS`).                               |
| Claude Code CLI   | `claude_code`       | A `claude` CLI that is installed and logged in on the machine (`NEXC_CLAUDE_BIN`, `RUNTIME_CLAUDE_BIN`) | No API key. Meant for `make dev`. Each call is an isolated `claude -p` with built-in tools, MCP, hooks and sessions disabled and a scrubbed environment.      |
| OpenAI-compatible | `openai_compatible` | `NEXC_LLM_BASE_URL`, for example `http://localhost:11434/v1` for Ollama                          | Any endpoint that speaks the OpenAI API: Ollama, vLLM, LM Studio. Set `NEXC_LLM_MODEL` to the model the endpoint serves.                                                |
| Demo              | `demo`              | Nothing                                                                                          | Deterministic and offline; see [Demo mode](/start/demo).                                                                                                                |

The complete list of variables is on [Environment variables](/reference/environment). What an agent may use is narrowed further by its policy: a per-agent model allow-list is part of its capabilities, see [Identity](/secops/identity).

## Knowledge base embeddings

The knowledge base embeds passages with an OpenAI-compatible `/embeddings` endpoint when one is configured, and with a built-in embedding that matches words rather than meaning when none is. Each workspace can set its own endpoint and model under **Settings, Knowledge**; see [Knowledge](/concepts/knowledge).
