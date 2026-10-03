# nexc-engine developer commands. Run `make` or `make help` for the list.

SHELL := /usr/bin/env bash
.SHELLFLAGS := -euo pipefail -c
.DEFAULT_GOAL := help

PNPM    ?= pnpm
CARGO   ?= cargo
UV      ?= uv
COMPOSE ?= docker compose

BACKEND_MANIFEST := backend/Cargo.toml
# Load the root .env into a recipe's environment (secrets never appear on the command line).
LOAD_ENV := set -a; [ -f .env ] && . ./.env; set +a;

.PHONY: help init setup dev dev-backend dev-frontend dev-runtime db-up db-down db-shell \
        build test test-backend test-frontend test-runtime lint fmt check \
        docker-build docker-up docker-down docker-logs demo \
        minikube-up minikube-down k8s-render openapi doctor clean

help: ## Show this help
	@awk 'BEGIN {FS = ":.*##"; printf "\nUsage: make \033[36m<target>\033[0m\n"} \
	  /^##@/ { printf "\n\033[1m%s\033[0m\n", substr($$0, 5) } \
	  /^[a-zA-Z0-9_-]+:.*##/ { printf "  \033[36m%-15s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

##@ Getting started

init: ## Create .env from .env.example and generate secrets (idempotent)
	@scripts/init-env.sh

setup: ## Install dependencies: cargo fetch, frontend packages, runtime venv
	$(CARGO) fetch --manifest-path $(BACKEND_MANIFEST)
	cd frontend && $(PNPM) install --frozen-lockfile
	cd agent-runtime && $(UV) sync --locked

##@ Local development

dev: db-up ## Run PostgreSQL + backend + runtime + frontend together (Ctrl-C stops all)
	@scripts/dev.sh

dev-backend: ## Run only the backend (nexc serve) on :8080
	@$(LOAD_ENV) $(CARGO) run --manifest-path $(BACKEND_MANIFEST) -- serve

dev-frontend: ## Run only the Vite dev server on :5173
	cd frontend && $(PNPM) dev

dev-runtime: ## Run only the agent runtime on :8090
	@$(LOAD_ENV) cd agent-runtime && $(UV) run nexc-runtime

db-up: ## Start PostgreSQL 17 in docker on localhost:5432 (data in a named volume)
	@scripts/db.sh up

db-down: ## Stop the dev PostgreSQL container (data is kept)
	@scripts/db.sh down

db-shell: ## Open psql in the dev database
	@scripts/db.sh shell

##@ Build, test, quality

build: ## Release build of backend, frontend and runtime
	$(CARGO) build --release --locked --manifest-path $(BACKEND_MANIFEST)
	cd frontend && $(PNPM) build
	cd agent-runtime && $(UV) build --wheel

test: test-backend test-frontend test-runtime ## Run every test suite

test-backend: ## Backend tests (needs PostgreSQL: make db-up)
	@$(LOAD_ENV) $(CARGO) test --locked --manifest-path $(BACKEND_MANIFEST)

test-frontend: ## Frontend unit tests (vitest)
	cd frontend && $(PNPM) test

test-runtime: ## Agent runtime tests (pytest, fake LLM, no network)
	cd agent-runtime && $(UV) run pytest

lint: ## Lint + typecheck everything (clippy -D warnings, eslint/tsc, ruff/mypy)
	$(CARGO) fmt --manifest-path $(BACKEND_MANIFEST) --check
	$(CARGO) clippy --manifest-path $(BACKEND_MANIFEST) --all-targets --locked -- -D warnings
	cd frontend && $(PNPM) typecheck && $(PNPM) lint && $(PNPM) format:check
	cd agent-runtime && $(UV) run ruff check && $(UV) run ruff format --check && $(UV) run mypy src tests

fmt: ## Format all code
	$(CARGO) fmt --manifest-path $(BACKEND_MANIFEST)
	cd frontend && $(PNPM) format
	cd agent-runtime && $(UV) run ruff check --fix && $(UV) run ruff format

check: lint test ## lint + test (what CI runs)

##@ Docker Compose

docker-build: ## Build the three images from source
	$(COMPOSE) build

docker-up: ## Build and start the full stack on http://localhost:8080
	@[ -f .env ] || scripts/init-env.sh
	$(COMPOSE) up -d --build
	@echo "nexc is starting on http://localhost:$${NEXC_HTTP_PORT:-8080} (make docker-logs to follow)"

smoke: ## End-to-end smoke test against a running stack (SMOKE_URL, default http://localhost:8080)
	scripts/smoke.sh $${SMOKE_URL:-http://localhost:8080}

demo: ## Start the stack in demo mode (offline LLM, no API key needed)
	@[ -f .env ] || scripts/init-env.sh
	$(COMPOSE) -f docker-compose.yml -f docker-compose.demo.yml up -d --build
	@echo "demo running on http://localhost:$${NEXC_HTTP_PORT:-8080}"

docker-down: ## Stop the stack (volumes are kept; add -v manually to wipe data)
	$(COMPOSE) --profile symphony down

docker-logs: ## Follow logs of every service
	$(COMPOSE) logs -f --tail=100

##@ Kubernetes (minikube)

minikube-up: ## Start minikube, build images inside it and deploy (http://nexc.local)
	@scripts/minikube-up.sh

minikube-down: ## Remove the nexc namespace from minikube (data included)
	@scripts/minikube-down.sh

k8s-render: ## Render the kustomize manifests to stdout
	@[ -f deploy/k8s/secret.env ] || scripts/init-env.sh --k8s >/dev/null
	@kubectl kustomize deploy/k8s

##@ Tooling

openapi: ## Export the OpenAPI spec from the backend and regenerate frontend types
	@mkdir -p .cache
	@$(LOAD_ENV) $(CARGO) run --quiet --manifest-path $(BACKEND_MANIFEST) -- openapi > .cache/openapi.json
	cd frontend && $(PNPM) exec openapi-typescript ../.cache/openapi.json -o src/lib/api/schema.d.ts

doctor: ## Check local tooling and configuration (nexc doctor)
	@scripts/doctor.sh

clean: ## Remove build outputs (keeps .env, data/ and the dev database volume)
	rm -rf backend/target frontend/dist frontend/node_modules/.vite agent-runtime/dist .cache
	find agent-runtime -type d \( -name __pycache__ -o -name .pytest_cache -o -name .mypy_cache -o -name .ruff_cache \) -prune -exec rm -rf {} +
