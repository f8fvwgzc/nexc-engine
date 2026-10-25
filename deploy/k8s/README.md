# nexc-engine on Kubernetes

<!-- #region intro -->
Kustomize manifests for a complete deployment: PostgreSQL 17, backend, agent runtime and frontend
behind an nginx Ingress, with NetworkPolicies and the `restricted` Pod Security Standard. They are
tested on minikube and are a sound starting point for any cluster.

<!-- #endregion intro -->
<!-- #region kustomize -->
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
| `Deployment backend` (2 replicas) + `Service` + `PVC backend-data` | a `migrate` init container applies migrations first (`nexc migrate`, no probe to stop it); startup/liveness `/api/v1/healthz`, readiness `/api/v1/readyz`; port 8081 serves the agent-token key set alone (`NEXC_KEYS_ADDR`); every pod on one node, since `backend-data` is `ReadWriteOnce` |
| `Deployment agent-runtime` (2 replicas) + `Service` | gets only `NEXC_RUNTIME_TOKEN`, the backend's agent-token key set URL and runtime settings, workspace on an `emptyDir` |
| `Deployment runtime-egress` (2 replicas) + `Service` | `nexc egress-proxy` from the backend image, no secrets: the runtime's only way out, to the `host:port` entries of `NEXC_RUNTIME_EGRESS_ALLOW` |
| `Deployment frontend` (2 replicas) + `Service` | nginx-unprivileged on 8080; with the optional `http3` component also TLS and QUIC on 8443, published by the LoadBalancer `Service frontend-https` (TCP and UDP 443) |
| `Ingress nexc`, `nexc-a2a`, `nexc-documents` | class `nginx`, host `nexc.local`; `/api` → backend, `/` → frontend; SSE/WebSocket timeouts and no buffering; bodies up to 2m, A2A tasks (`/a2a`) 17m, knowledge documents 51m (an annotation applies to every path of its Ingress, hence three) |
| `NetworkPolicy` ×8 | default deny ingress; frontend ← ingress; backend ← frontend + ingress (8080), runtime (8081, the agent-token key set alone); runtime ← backend (HTTP 8090 and the gRPC stream 8095); runtime → backend 8081 and runtime-egress only, no DNS (it finds both by their Service environment variables, so apply the Services first: `kubectl apply -k` does); runtime-egress ← runtime; runtime-egress → DNS and TLS on public addresses (IPv4 and IPv6); postgres ← backend. The Agent Firewall API (`NEXC_GRPC_ADDR`) is off by default and documented in the file for when you open it to another namespace |
| `PodDisruptionBudget` ×4 | `minAvailable: 1` for backend, runtime, runtime-egress and frontend |

Every pod runs as non-root with `readOnlyRootFilesystem`, `allowPrivilegeEscalation: false`,
`capabilities.drop: [ALL]`, seccomp `RuntimeDefault` and no service-account token.

## Configuration

* `config.env` – edit, then `kubectl apply -k deploy/k8s` (the ConfigMap name gets a content
  hash, so pods roll automatically).
* `secret.env` – created by `scripts/init-env.sh --k8s`; existing values are never overwritten.
  Put `ANTHROPIC_API_KEY` here and set `NEXC_LLM_PROVIDER=anthropic` in `config.env` to leave demo
  mode. Optionally set `NEXC_ADMIN_EMAIL` / `NEXC_ADMIN_PASSWORD` to bootstrap a platform
  administrator (an account for the platform console; it does not work inside workspaces).
* Images – `kustomization.yaml` pins `ghcr.io/f8fvwgzc/nexc-engine-*:local` (built by the script).
  For released images: `cd deploy/k8s && kustomize edit set image ghcr.io/f8fvwgzc/nexc-engine-backend:0.1.0`
  (same for `-frontend` and `-runtime`).

Render without applying: `make k8s-render`.

<!-- #endregion kustomize -->
## Helm

<!-- #region helm -->
`deploy/helm/nexc-engine` is the same deployment as a chart, for clusters that are managed with
Helm: the objects above, templated. `values.yaml` sets the images and their tag (the chart's
`appVersion` by default, `local` for images built by `scripts/minikube-up.sh`), the replicas,
the non-secret settings (`config`), the runtime's ports (`runtime.httpPort`, `runtime.grpcPort`:
the gRPC stream the backend opens, the only port the NetworkPolicy admits besides HTTP), the
Agent Firewall API (`backend.grpcAddr` turns it on; `backend.firewall.port` is its Service port
and `networkPolicies.firewallFrom` the namespaces admitted to it), the OTLP collector
(`backend.otlpEndpoint`) and onto-intelli (`backend.ontoUrl`, its token in the Secret as
`NEXC_ONTO_TOKEN`), the ingress host and TLS, the frontend's own TLS and HTTP/3 behind a
LoadBalancer (`frontend.http3`, off by default), whether the chart runs PostgreSQL
(`postgres.enabled=false` for a managed database, its URL in the Secret) and the connection
budget (`postgres.maxConnections`: each backend pod's pool is its share unless
`config.NEXC_DB_MAX_CONNECTIONS` is set, and the chart refuses to render pools that overflow its
own PostgreSQL). The backend's pods share one node while `backend.dataAccessMode` is
`ReadWriteOnce` and spread across nodes with `ReadWriteMany`. Secrets come from a
Secret you create (`secrets.existingSecret`) or from `secrets.values`; the chart refuses to
render while a required one is empty. One release per namespace: the object names are fixed.

```sh
scripts/init-env.sh --k8s                      # secret.env with generated secrets
kubectl create namespace nexc
kubectl label namespace nexc pod-security.kubernetes.io/enforce=restricted
kubectl -n nexc create secret generic nexc-secrets --from-env-file=deploy/k8s/secret.env
helm install nexc deploy/helm/nexc-engine -n nexc --set secrets.existingSecret=nexc-secrets
helm template nexc deploy/helm/nexc-engine -n nexc --set secrets.existingSecret=nexc-secrets   # render only
```

CI lints the chart and renders it with the firewall, the collector and onto-intelli on.

<!-- #endregion helm -->
<!-- #region production -->
## Production checklist

* **TLS** – add a `tls:` block to `ingress.yaml` (for example with cert-manager), enable
  `force-ssl-redirect` on its three Ingress objects, then set `NEXC_ENV=production`,
  `NEXC_COOKIE_SECURE=true`, `NEXC_CORS_ORIGINS=https://<host>` and
  `NEXC_PUBLIC_URL=https://<host>` in `config.env`. Without TLS, production's `Secure` refresh
  cookie is dropped by browsers on any host but localhost, which signs users out
  ([docs/INFRASTRUCTURE.md](../../docs/INFRASTRUCTURE.md), Serving it on a host of its own).
* **HTTP/3** – ingress-nginx terminates TLS over TCP alone, so the Ingress serves HTTP/1.1 and
  HTTP/2. For HTTP/3 either put a CDN or a Gateway API implementation that speaks it in front, or
  list the `http3` component in `kustomization.yaml` (Helm: `frontend.http3`): the frontend's own
  nginx then terminates TLS and QUIC behind a LoadBalancer Service with TCP and UDP 443
  (`http3/kustomization.yaml` lists what it needs). See
  [docs/INFRASTRUCTURE.md](../../docs/INFRASTRUCTURE.md).
* **Database** – prefer a managed PostgreSQL 17 or an operator such as CloudNativePG with backups;
  point `NEXC_DATABASE_URL` at it (`sslmode=require`) and drop `postgres.yaml` from the resources.
* **Artifact storage** – `backend-data` is `ReadWriteOnce`, so `backend.yaml` keeps every backend
  pod on one node (a required pod affinity): the replicas cover a crash and a rollout, not the
  loss of that node, and draining it waits until the last backend pod is deleted. If that node
  fails, its pods stay `Terminating` and their replacements `Pending`; once the node is really off,
  taint it `node.kubernetes.io/out-of-service=nodeshutdown:NoExecute` or force-delete those pods
  (`kubectl delete pod --force --grace-period=0`) and they start elsewhere, the volume with them
  ([docs/HA.md](../../docs/HA.md)). To spread them across nodes, use a `ReadWriteMany` storage
  class and the topology spread in `backend.yaml`'s comment instead of the affinity.
* **Scaling** – run two or more backend replicas: the backend scales horizontally (realtime
  fan-out over PostgreSQL `LISTEN/NOTIFY`), any replica's dispatcher claims a queued run and holds
  it under a lease (`NEXC_RUN_LEASE_SECS`, the same on every replica), and when that replica dies
  or drains a peer takes the run over and resumes it (see [docs/HA.md](../../docs/HA.md) and
  [docs/ARCHITECTURE.md](../../docs/ARCHITECTURE.md#scaling-and-state)). Replicas on several
  nodes need the `ReadWriteMany` artifact storage above. The runtime is stateless. Every backend
  pod, a rollout's included, fills a pool of `NEXC_DB_MAX_CONNECTIONS` (20): keep pods x pool
  under PostgreSQL's `max_connections` (100) less a few ([docs/HA.md](../../docs/HA.md),
  Connections to PostgreSQL).
* **Client addresses** – the backend counts failed sign-ins per address. Behind a cloud load
  balancer, ingress-nginx sees the balancer's or a node's address unless its Service has
  `externalTrafficPolicy: Local`, the PROXY protocol is on, or (behind an L7 balancer)
  `use-forwarded-headers` and `proxy-real-ip-cidr` are set in its ConfigMap
  ([docs/INFRASTRUCTURE.md](../../docs/INFRASTRUCTURE.md), Client addresses).
* **Images** – `:local` is what `scripts/minikube-up.sh` builds. Released images exist only for
  `v*` tags, so a release tag runs that release, not `main`.
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

<!-- #endregion production -->