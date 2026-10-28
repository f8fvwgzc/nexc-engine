---
id: ops-infrastructure-as-code
name: Infrastructure as code
category: devops
description: Builds and runs infrastructure as code with Terraform or OpenTofu: plan and apply workflow, remote state and locking, module design, policy as code, drift detection and keeping secrets out of state.
keywords: terraform, opentofu, infrastructure as code, iac, hcl, terraform state, state locking, backend, modules, policy as code, opa, sentinel, drift detection, secrets
source: HashiCorp - Terraform Associate (004) exam content list | https://developer.hashicorp.com/terraform/tutorials/certification-004/associate-review-004
source: HashiCorp - Manage sensitive data in Terraform | https://developer.hashicorp.com/terraform/language/manage-sensitive-data
source: HashiCorp - S3 backend | https://developer.hashicorp.com/terraform/language/backend/s3
source: OpenTofu - State and plan encryption | https://opentofu.org/docs/language/state/encryption/
---
## When to use
Writing or reviewing Terraform/OpenTofu, pipelines for infrastructure, state trouble, imports, drift, guardrails. Versions in October 2026: Terraform 1.17 (released 2026-10-21; the Terraform Associate 004 exam tests 1.12), OpenTofu 1.13. Syntax mostly overlaps but features diverge; check before using version-specific blocks.

## Workflow
1. fmt and validate locally and in CI; tflint plus static checks (Checkov, Trivy) on every pull request.
2. init with pinned providers (required_providers with ~> constraints) and the committed .terraform.lock.hcl.
3. plan -out=tfplan in CI; post it to the pull request; a human reviews every destroy and replace (-/+).
4. Apply exactly the saved plan after approval, only from the pipeline; no laptop applies to shared environments.
5. Automation: plan -detailed-exitcode returns 0 no changes, 1 error, 2 changes pending.

## State
- Remote, encrypted, versioned backend: S3 with use_lockfile = true (S3-native lock file; DynamoDB locking is deprecated, set both only while migrating), azurerm blob (lease lock), GCS, or HCP Terraform. force-unlock only after confirming no run is active.
- Split state by blast radius and lifecycle (environment x component: network, data, app). Share values through published outputs or parameters rather than reading whole remote states.
- Refactor safely: moved blocks for renames (a moved block with from = aws_db_instance.main and to = aws_db_instance.primary, each argument on its own line; the plan must then show no destroy), import blocks (plan -generate-config-out) to adopt resources, removed blocks to forget without destroying; state mv/rm only as a last resort, after a backup.
- State holds secrets in plain text: restrict and audit access.

## Modules
Small and single-purpose; versioned and pinned (registry version or git tag); typed variables with validation blocks; preconditions and postconditions on resources, check blocks for post-apply assertions; documented outputs; no provider blocks in reusable modules; root modules compose; examples plus terraform test or tofu test.

## Policy as code
Check the plan JSON (show -json tfplan) with OPA/Conftest, Sentinel in HCP Terraform, or Checkov/Trivy: required tags, allowed regions and sizes, no public buckets or 0.0.0.0/0 ingress, encryption on. Start advisory, then mandatory; exceptions documented with an expiry.

## Drift
Scheduled plan -refresh-only or -detailed-exitcode per state; alert on exit 2; then codify the change or revert it; read-only console roles in production.

## Secrets
Never commit secrets or tfvars containing them; read them from a secret manager at run time. sensitive only redacts output; the value still lands in state. Use ephemeral variables and ephemeral resources (1.10+) and write-only arguments (1.11+) to keep secrets out of state and plans. OpenTofu can encrypt state and plan client-side (key providers pbkdf2, AWS KMS, GCP KMS, Azure Key Vault, OpenBao or external; method aes_gcm). CI reaches clouds through OIDC federation, not stored keys.

## Output
Change summary, plan excerpt with risky actions highlighted, policy results, state layout, rollback approach.

## Pitfalls
Unpinned providers; count over lists (an index shift destroys resources; use for_each with stable keys); routine -target; giant states; ignore_changes hiding drift; no prevent_destroy on data stores.
