# Demo mode

Demo mode runs the whole product offline with a deterministic provider, so you can explore plans, runs, agents and the AgentSecOps console before you add an API key.

## How it is switched on

`make init` writes `NEXC_LLM_PROVIDER=demo` into `.env`, so a fresh installation starts in demo mode. Every output is labelled as demo content, and the app shows a banner while the installation is in this mode. In Kubernetes, demo mode is the default of `config.env` and of the Helm chart's `config.NEXC_LLM_PROVIDER`.

## The scripted demos

| Command             | What it does                                                                                                                                                                                                                      |
| ------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `make demo`         | Starts the Compose stack with the demo provider pinned, whatever `.env` says.                                                                                                                                                     |
| `make seed-demo`    | Fills the running app with a demo workspace, five accounts and their inboxes.                                                                                                                                                     |
| `make smoke`        | Walks register, template, plan, apply, run, cached re-run, artifacts and SSE against a running stack. CI runs it against Docker Compose on every pull request.                                                                    |
| `make demo-secops`  | Starts the stack and plays the [five-minute AgentSecOps demo](/secops/demo): an agent from a template, a traced run, an approval, a withheld document, a denial, an alert, a quarantine, the audit questions, the latency table and the chain verified. |
| `make demo-onto`    | Demo mode plus [onto-intelli](https://github.com/f8fvwgzc/onto-intelli) as the audit's query engine, built from a checkout next to this repository: a demo graph, then the six audit questions with mode, tokens and latency.      |

## What the demo provider does

The planner is a deterministic offline generator and node outputs are deterministic too, so the same graph gives the same result on every run and in CI. A demo run still exercises the tools gateway: the provider reads the http(s) URLs a node's content names through `fetch_url` when the policy offers the tool (three at most), so egress decisions, spans and alerts are real. See [Tools gateway](/secops/gateway).

## Leaving demo mode

Set `NEXC_LLM_PROVIDER` to `anthropic`, `claude_code` or `openai_compatible` in `.env` and start the stack again, or store a personal key under **Settings, AI accounts** in the app. [Claude and local models](/start/models) says what each provider needs.
