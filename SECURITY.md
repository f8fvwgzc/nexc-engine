# Security policy

nexc-engine runs LLM agents, stores encrypted API keys and executes user-defined graphs, so we
take security reports seriously and appreciate responsible disclosure.

## Supported versions

| Version | Supported |
|---------|-----------|
| 0.1.x (latest release) | yes |
| `main` | best effort |
| older | no |

## Reporting a vulnerability

**Please do not open public issues, discussions or pull requests for security problems.**

Report privately through GitHub:
[**Report a vulnerability**](https://github.com/f8fvwgzc/nexc-engine/security/advisories/new)
(Security tab -> Advisories -> "Report a vulnerability").

Include as much as you can:

- affected component (backend, frontend, agent runtime, deployment manifests) and version/commit,
- a description of the issue and its impact,
- step-by-step reproduction or a proof of concept,
- any suggested fix or mitigation.

### What to expect

- Acknowledgement within **3 business days**.
- An initial assessment (severity, affected versions) within **7 days**.
- A fix or mitigation plan, coordinated with you, typically within **30 days** for high and
  critical issues. We will credit you in the advisory unless you prefer to stay anonymous.
- Please give us a reasonable window to release a fix before any public disclosure.

## Scope

In scope: authentication and session handling, authorization between users and graphs, the
encryption of stored LLM keys, the agent runtime sandbox (workspace confinement, `run_python`),
SSRF via provider base URLs, injection in any API, and insecure defaults in the Docker Compose or
Kubernetes manifests.

Out of scope: findings that require an already-compromised host or admin account, missing
hardening headers without a demonstrated impact, denial of service by volumetric traffic, and
model behaviour that does not cross a security boundary (for example a prompt injection that only
changes the text of an output the same user can already edit).

## Hardening checklist for operators

- Run `make init` (or `nexc init`) so every secret is random; never reuse the examples.
- Set `NEXC_ENV=production`, serve over HTTPS and keep `NEXC_COOKIE_SECURE=true`.
- Keep the agent runtime and PostgreSQL on private networks (the provided Compose file and
  Kubernetes NetworkPolicies do this) and leave `RUNTIME_ALLOW_CODE_EXEC=false` unless needed.
- Back up `NEXC_MASTER_KEY` together with the database; rotate `NEXC_RUNTIME_TOKEN` and
  `NEXC_JWT_SECRET` if they may have leaked.
