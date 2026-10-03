# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-10-03

First public release.

### Added

- Visual graph editor with typed nodes, `[[wikilink]]` auto-edges, dependency suggestions and live
  collaboration over WebSocket.
- LLM plan refinement streamed over SSE, reviewed and applied atomically.
- DAG scheduler with concurrency limits, retries, timeouts, cancellation, critical-path analysis
  and a content-hash result cache.
- Executors: direct LLM calls, the Python agent runtime and the optional texc-symphony bridge.
- Python agent runtime: agents born from a spec per request with role personas, token budgets,
  sub-agents (depth 2, up to 4 children), workspace-confined tools, `.docx` generation and an
  opt-in sandboxed `run_python`.
- LLM providers: Anthropic (default `claude-opus-5`, adaptive thinking, server-side refusal
  fallbacks), OpenAI-compatible endpoints, and an offline deterministic `demo` provider.
- Memory with BM25 retrieval; C11 kernels for hashing, MinHash, embeddings and SHA-256.
- Authentication with Argon2, rotating refresh tokens with reuse detection, CSRF guard and
  AES-256-GCM encrypted API keys.
- PostgreSQL 17 storage with realtime fan-out over `LISTEN/NOTIFY`.
- OpenAPI document and Scalar API reference; generated frontend types.
- Docker images (non-root, read-only), Docker Compose stack with a demo overlay, Kubernetes
  manifests with NetworkPolicies, Makefile workflows, CI, CodeQL and release automation.

[Unreleased]: https://github.com/f8fvwgzc/nexc-engine/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/f8fvwgzc/nexc-engine/releases/tag/v0.1.0
