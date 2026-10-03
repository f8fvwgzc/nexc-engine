# Contributing to nexc-engine

Thanks for helping! Bug reports, docs fixes, new tools for agents and whole features are all
welcome. Please read the [Code of Conduct](CODE_OF_CONDUCT.md) first; security issues go through
[SECURITY.md](SECURITY.md), never public issues.

## Ground rules

* **The contract comes first.** [docs/CONTRACT.md](docs/CONTRACT.md) is the single source of truth
  shared by the backend, frontend and agent runtime. A change to an endpoint, event, type or
  environment variable updates the contract and every implementation in the same pull request.
* **Small, focused pull requests** with tests are reviewed fastest. For larger changes open an
  issue or discussion first so we can agree on the approach.
* **Secure by default.** No secrets in code, logs, fixtures or screenshots; new settings get safe
  defaults; new endpoints get authentication, validation and limits.

## Development setup

Prerequisites: Rust 1.99+ and a C compiler, Node.js 22.12+ with pnpm 10 (`corepack enable`),
[uv](https://docs.astral.sh/uv/), Docker and `openssl`. `make doctor` verifies them.

```sh
make init     # .env with generated secrets (demo LLM provider, no key needed)
make setup    # install dependencies for all three components
make dev      # PostgreSQL + backend + runtime + frontend with live reload of the frontend
```

| Task | Command |
|---|---|
| Everything CI runs | `make check` (= `make lint test`) |
| Format all code | `make fmt` |
| One component | `make test-backend`, `make test-frontend`, `make test-runtime` |
| Regenerate frontend API types after backend changes | `make openapi` |
| Full stack in containers | `make docker-up` |
| Kubernetes smoke test | `make minikube-up` |

Backend tests need PostgreSQL (`make db-up`). Runtime tests use a fake LLM and never touch the
network; never add tests that call a real LLM API.

## Conventions

### Rust (`backend/`)
* `cargo fmt` and `cargo clippy --all-targets -- -D warnings` must be clean.
* Errors are typed (`thiserror`) inside the crate and become RFC 7807 responses at the edge.
* New endpoints are annotated for utoipa so they appear in `/api/openapi.json`.
* `unsafe` is confined to the C-kernel FFI module and every kernel has a Rust reference
  implementation that tests compare against.

### TypeScript (`frontend/`)
* `pnpm typecheck`, `pnpm lint` and `pnpm format:check` must pass.
* Use the generated API types; validate responses with zod at the boundary.
* Components are accessible (labels, focus states, keyboard navigation) and work in light and dark
  themes.

### Python (`agent-runtime/`)
* `uv run ruff check`, `uv run ruff format --check` and `uv run mypy src tests` (strict) must pass.
* Tools validate their arguments with pydantic and never touch paths outside the run workspace
  (always go through `Workspace` / `safe_path`).
* Anything that might contain an API key goes through `redact()` before it is logged or returned.

### Infrastructure
* Shell scripts pass `shellcheck`, Dockerfiles pass `hadolint`, `docker compose config` and
  `kubectl kustomize deploy/k8s` succeed (all checked in CI).
* Containers stay non-root with a read-only root filesystem.

## Commit messages

We use [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <summary in the imperative, max ~72 chars>

<optional body: what and why, wrapped at 72>

<optional footer: Closes #123, BREAKING CHANGE: ...>
```

Types: `feat`, `fix`, `docs`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`.
Scopes: `backend`, `frontend`, `runtime`, `deploy`, `docs`, `contract`.

Examples: `feat(runtime): add spreadsheet artifact tool`,
`fix(backend): reject plans with duplicate refs`.

## Pull requests

1. Fork and create a branch from `main` (`feat/runtime-xlsx-tool`).
2. Make the change with tests; run `make check`.
3. Add an entry under `## [Unreleased]` in [CHANGELOG.md](CHANGELOG.md) for user-visible changes.
4. Open the PR using the template; link the issue (`Closes #123`).
5. A maintainer reviews; CI (lint, tests, CodeQL, image builds) must be green before merging.
   We squash-merge, so the PR title becomes the commit message and should follow the convention.

## Releases

Maintainers tag `vX.Y.Z` on `main` after moving the `Unreleased` changelog entries under the new
version. The release workflow builds multi-arch images with SBOM and provenance to
`ghcr.io/f8fvwgzc/nexc-engine-{backend,frontend,runtime}` and publishes the GitHub release.

## License

By contributing you agree that your contributions are licensed under the
[Apache License 2.0](LICENSE).
