# nexc-engine on Kubernetes

Kustomize manifests for a complete deployment: PostgreSQL 17, backend, agent runtime and frontend
behind an nginx Ingress, with NetworkPolicies and the `restricted` Pod Security Standard. They are
tested on minikube and are a sound starting point for any cluster.

## Quick start (minikube)

```sh
make minikube-up        # = scripts/minikube-up.sh
```

The script:

1. starts a minikube profile `nexc` with the **calico** CNI (so NetworkPolicies are enforced),
2. enables the ingress addon,
3. builds the three images inside minikube's Docker daemon (`eval $(minikube docker-env)`),
4. creates `secret.env` with generated secrets (`scripts/init-env.sh --k8s`),
5. runs `kubectl apply -k deploy/k8s` and waits for every rollout,
6. prints how to map `nexc.local` in `/etc/hosts`.

Open http://nexc.local. Demo mode is on by default (`NEXC_LLM_PROVIDER=demo` in `config.env`).
Remove everything with `make minikube-down` (deletes the namespace **and its data**) or
`scripts/minikube-down.sh --delete` to delete the minikube profile too.

Tunables: `MINIKUBE_PROFILE`, `MINIKUBE_CPUS` (4), `MINIKUBE_MEMORY` (8192), `MINIKUBE_DRIVER`,
`MINIKUBE_CNI` (calico).

## What gets deployed

| Object | Notes |
|---|---|
| `Namespace nexc` | `pod-security.kubernetes.io/enforce: restricted` |
| `ConfigMap nexc-config` | from `config.env` (non-secret settings) |
| `Secret nexc-secrets` | from `secret.env` (git-ignored; template `secret.example.env`) |
| `StatefulSet postgres` + headless `Service` | `postgres:17-alpine`, 5 Gi PVC, runs as uid 70, read-only root |
| `Deployment backend` (2 replicas) + `Service` + `PVC backend-data` | startup/liveness `/api/v1/healthz`, readiness `/api/v1/readyz` |
| `Deployment agent-runtime` (2 replicas) + `Service` | gets only `NEXC_RUNTIME_TOKEN` and runtime settings, workspace on an `emptyDir` |
| `Deployment frontend` (2 replicas) + `Service` | nginx-unprivileged on 8080 |
| `Ingress nexc` | class `nginx`, host `nexc.local`; `/api` → backend, `/` → frontend; SSE/WebSocket timeouts and no buffering |
| `NetworkPolicy` ×5 | default deny ingress; frontend ← ingress; backend ← frontend + ingress; runtime ← backend; postgres ← backend |
| `PodDisruptionBudget` ×3 | `minAvailable: 1` for backend, runtime and frontend |

Every pod runs as non-root with `readOnlyRootFilesystem`, `allowPrivilegeEscalation: false`,
`capabilities.drop: [ALL]`, seccomp `RuntimeDefault` and no service-account token.

## Configuration

* `config.env` – edit, then `kubectl apply -k deploy/k8s` (the ConfigMap name gets a content
  hash, so pods roll automatically).
* `secret.env` – created by `scripts/init-env.sh --k8s`; existing values are never overwritten.
  Put `ANTHROPIC_API_KEY` here and set `NEXC_LLM_PROVIDER=anthropic` in `config.env` to leave demo
  mode. Optionally set `NEXC_ADMIN_EMAIL` / `NEXC_ADMIN_PASSWORD` to bootstrap an admin.
* Images – `kustomization.yaml` pins `ghcr.io/f8fvwgzc/nexc-engine-*:local` (built by the script).
  For released images: `cd deploy/k8s && kustomize edit set image ghcr.io/f8fvwgzc/nexc-engine-backend:0.1.0`
  (same for `-frontend` and `-runtime`).

Render without applying: `make k8s-render`.

## Production checklist

* **TLS** – add a `tls:` block to `ingress.yaml` (for example with cert-manager), enable
  `force-ssl-redirect`, then set `NEXC_ENV=production`, `NEXC_COOKIE_SECURE=true` and
  `NEXC_CORS_ORIGINS=https://<host>` in `config.env`.
* **Database** – prefer a managed PostgreSQL 17 or an operator such as CloudNativePG with backups;
  point `NEXC_DATABASE_URL` at it (`sslmode=require`) and drop `postgres.yaml` from the resources.
* **Artifact storage** – `backend-data` is `ReadWriteOnce`, which is fine on one node. With several
  nodes and more than one backend replica use a `ReadWriteMany` storage class.
* **Scaling** – the backend scales horizontally (realtime fan-out over PostgreSQL
  `LISTEN/NOTIFY`); a run executes on the replica that accepted it (see
  [docs/ARCHITECTURE.md](../../docs/ARCHITECTURE.md#scaling-and-state)). The runtime is stateless.
* **Egress** – the policies restrict ingress only. To restrict egress as well, allow DNS, the
  database, the runtime and your LLM provider's endpoints explicitly.
* **Signups** – set `NEXC_ALLOW_SIGNUP=false` once your users exist.
* **Metrics** – `/metrics` is not routed by the Ingress; scrape `backend:8080/metrics` from inside
  the cluster with `NEXC_METRICS_TOKEN` (add a NetworkPolicy rule for your Prometheus namespace).

## Troubleshooting

```sh
kubectl -n nexc get pods
kubectl -n nexc describe pod <pod>
kubectl -n nexc logs deploy/backend -f
kubectl -n nexc get events --sort-by=.lastTimestamp
```

* `ImagePullBackOff` for `…:local` – the images were built outside minikube's daemon; rerun
  `make minikube-up` (it runs `eval $(minikube docker-env)` before building).
* Pods rejected by Pod Security – a custom image or patch broke the restricted profile (root user,
  writable root filesystem or extra capabilities).
* Everything is `Running` but the site does not load – check the `/etc/hosts` entry, and with the
  docker driver on macOS/Windows keep `minikube tunnel` running.
