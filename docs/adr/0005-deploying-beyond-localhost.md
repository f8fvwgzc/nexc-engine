# ADR 0005: What breaks between localhost and a cluster, and the defaults that keep it working

- **Status:** accepted, 2026-10-25. Replaces nothing; extends the edge of
  [ADR 0001](0001-http3-edge-caching-and-indexes.md) and [ADR 0002](0002-edge-hardening-and-adaptive-grpc-windows.md).
- **Built and run:** `nginx -t` and a Compose-like run of the frontend's nginx, `helm lint` and
  `helm template`, `kubectl kustomize`, server-side dry runs through ingress-nginx's admission
  webhook, `docker compose config`, and the backend's tests (How to verify).
- **Scope:** `frontend/nginx.conf`, `frontend/nginx/site.conf`,
  `frontend/docker-entrypoint.d/39-nexc-backend.sh`, `frontend/Dockerfile`, `deploy/k8s`
  (`backend.yaml`, `ingress.yaml`, `config.env`), `deploy/helm/nexc-engine` (backend, ingress,
  postgres, configmap, NOTES, values), `docker-compose.yml`, `backend/src/realtime/fanout.rs`,
  `backend/src/app/caches.rs`

<!-- #region body -->
## Context

Local development runs Vite's proxy to one backend on localhost, against PostgreSQL on localhost:
no body limit, one replica, a database round trip of 0.05 ms. A deployment adds an edge with
limits of its own, several replicas, a volume, a probe and a database that is not on the same
machine. Each of the following worked locally and failed only once deployed:

- **Uploads.** The backend and the console take 50 MiB knowledge documents; the frontend's nginx
  capped every request at 2 MiB, and so did the Ingress, where A2A tasks (16 MiB at the backend)
  hit the same 2 MiB. Anything larger got nginx's 413 page.
- **The backend's address.** nginx resolved `backend` once, at start. A recreated backend
  container in Compose has a new address, so every `/api` request answered 502 until the frontend
  restarted, and the image could not start anywhere without a host named `backend`.
- **The data volume.** `backend-data` is `ReadWriteOnce`, while two replicas were spread across
  nodes with `maxSurge: 1`: on any cluster with more than one node the second pod, or a
  rollout's new pod, waited on a Multi-Attach error and the rollout missed its deadline. minikube
  has one node, so it never showed.
- **Migrations.** `serve` applied them before listening, under a startup probe of 150 s. An index
  built `CONCURRENTLY` on a large table outlasts that; a build killed half way leaves an invalid
  index, and ADR 0004's check then keeps every later start out until someone reindexes by hand.
- **Connections.** Each backend pod keeps a pool of 20, its realtime listener one of them. The
  chart's PostgreSQL keeps the default `max_connections` of 100: two replicas and a rollout's pod
  take 60, the autoscaler at 6 takes 140, onto-intelli adds 48. Past 100 PostgreSQL refuses
  connections and requests fail.
- **Missed invalidations.** `PgListener::recv()` reconnects by itself when its connection drops
  (a failover, a NAT's idle timeout). The fan-out never knew, and the invalidations peers sent in
  the gap were lost: policies, guardrails and the rest stayed stale for up to
  `NEXC_CACHE_TTL_SECS` on that replica.
- **CI.** `main` had been red since 2026-10-20 on `cargo fmt` and on shellcheck's SC2317, so
  clippy, the tests, the Compose smoke run and the image builds had not run on anything since.

## Decision

1. **Every edge allows a little more than the backend, per route.** nginx keeps 2m for
   everything and gives `^/api/v1/workspaces/[^/]+/documents$` 51m (a nested location of
   `/api/`) and `/a2a/` 17m. In Kubernetes the two routes sit on Ingress objects of their own,
   `nexc-documents` and `nexc-a2a`, because an annotation applies to every path of its Ingress.
   The documents' path is a regular expression, which turns on regex matching for every path of
   the host in ingress-nginx; the paths still route as before. The well-known documents became
   `ImplementationSpecific`, since ingress-nginx 1.12 refuses a dot in an `Exact` path. The
   backend's own limits (1 MiB, 16 MiB, 50 MiB and the multipart envelope) stay the ones that
   answer.
2. **nginx looks the backend up while it runs.** `upstream nexc_backend` has a zone and
   `server <host>:<port> resolve` (nginx 1.27.3 and later; the image has 1.28.2), with a resolver
   asked every 10 s, IPv4 only. `39-nexc-backend.sh` writes both from `/etc/resolv.conf` and
   `NEXC_BACKEND_UPSTREAM` (`backend:8080`), and gives a bare name the first search domain when
   that is a Kubernetes namespace's, because nginx's resolver has no search list.
3. **One node for a `ReadWriteOnce` volume.** The backend's pods carry a required pod affinity to
   each other's node while the volume is `ReadWriteOnce` (the default), and the topology spread
   only with `ReadWriteMany` (Helm: `backend.dataAccessMode`; kustomize: the spread stays in a
   comment). Two replicas still cover a crash and a rolling update; the loss of the node needs
   `ReadWriteMany`, which is the documented way to spread. The replica count stays at 2.
4. **Migrations in an init container.** Each backend pod runs `nexc migrate` in a `migrate` init
   container with the backend's environment, before `serve`; no probe applies to it, so nothing
   stops an index build half way. `serve` still migrates, and finds nothing to do. A Helm hook
   was not chosen: kustomize has no hooks, and a pre-install hook would run before the chart's
   ConfigMap and Secret exist.
5. **A connection budget the chart computes.** `postgres.maxConnections` (100) less
   `postgres.reservedConnections` (10) and onto-intelli's pools is shared by the most backend pods
   that run at once (the autoscaler's maximum or the replicas, plus one for a rollout). Each pod's
   pool is that share, at most 20, unless `config.NEXC_DB_MAX_CONNECTIONS` is set; a share under 4,
   or a pool set that overflows the chart's own PostgreSQL, refuses to render. A value other than
   100 is passed to the chart's PostgreSQL as `-c max_connections`; 100 passes nothing, so an
   upgrade does not restart it.
6. **A reconnected listener drops every cached entry.** The listener reads with `try_recv()`,
   whose `None` is a dropped connection already re-established, and after it, or after a failed
   connect, calls `Caches::drop_all_local()` once `LISTEN` is open again. Vocabularies are made
   stale rather than dropped, so they stay the fallback while onto-intelli is away.
7. **CI green again.** `cargo fmt`; `disable=SC2317,SC2329` on the trap-only function (the
   runner's shellcheck and 0.11 name the same finding differently); the frontend's build and
   compression in one `RUN` (hadolint DL3059, which would have failed next).

What only bites on a host other than localhost and cannot be a default (the `Secure` refresh
cookie over plain HTTP, `NEXC_PUBLIC_URL`, CORS for another origin, a certificate nginx can read,
client addresses behind a cloud load balancer, images pushed only for release tags) is written
down in `docs/INFRASTRUCTURE.md` (Serving it on a host of its own).

## Consequences

- A `ReadWriteOnce` installation keeps its backend on one node: a node failure stops the backend
  until Kubernetes moves the pods and the volume, and draining that node waits on the
  PodDisruptionBudget until the last backend pod is deleted by hand. Before, the same cluster
  could not run two replicas at all.
- With the autoscaler at 6 the chart's pool shrinks from 20 to 12 per pod, and beside onto-intelli
  to 14: fewer queries in parallel per pod, none refused by PostgreSQL. Releases that set
  `config.NEXC_DB_MAX_CONNECTIONS` to a pool that does not fit no longer render; they did not fit
  before either.
- A failover costs every replica its caches once: the next reads of policies, guardrails, LLM
  settings and agent cards go to the database, and onto-intelli's reads to onto-intelli.
- ingress-nginx matches the host's paths as case-insensitive regular expressions, so `/API/...`
  reaches the backend (which answers 404) instead of the console.
- The frontend's nginx asks DNS every 10 s; between a backend's recreation and the next lookup,
  requests may answer 502 for up to 10 s.
- Not changed: the backend's 60 s request timeout includes reading the body, and HTTP/3's 64 KiB
  stream buffer bounds one upload per round trip, so a 50 MiB document on a slow or distant link
  can still time out. Realtime events sent while a listener was away are not replayed.

## How to verify

- `nginx -t` in `nginxinc/nginx-unprivileged:1.28-alpine` with the worktree's files mounted, with
  no `backend` host (the previous configuration fails there: host not found in upstream), with a
  Kubernetes `resolv.conf` (`backend.nexc.svc.cluster.local:8080`, an IPv6 name server bracketed,
  one with a zone left out), with TLS on.
- On a Docker network with a stand-in backend: `/api` answered, the backend was removed and
  started again at another address, and nginx answered from it after about 9 s without a restart.
  A 30 MiB document passed, 53 MiB and 30 MiB to another route got 413, a 16.5 MiB A2A task
  passed and 30 MiB got 413.
- `kubectl apply --dry-run=server` of both Ingress renders through ingress-nginx 1.11.2's
  admission webhook, which renders and tests the whole nginx configuration (the `Exact`
  well-known paths warned before, and no longer do).
- `helm lint`; every `helm template` CI runs; the pool at 20 (defaults), 12 (autoscaled to 6), 14
  (with onto-intelli), 15 (set), refusals for an overflowing pool and for too small a share; the
  affinity with `ReadWriteOnce` and the spread with `ReadWriteMany`; the client secret in both
  the `migrate` and the `backend` containers with single sign-on on; values from a release without
  the new keys. `kubectl kustomize deploy/k8s`, `docker compose config` with every override file.
- `cargo test --test caches`: `a_lost_listen_connection_drops_every_entry` ends both replicas'
  `LISTEN` connections with `pg_terminate_backend` and checks that the entries go and that the next
  write still reaches the other replica. It fails without the change.
- Not run: a multi-node cluster, a real ingress-nginx serving the three Ingress objects, the
  frontend image built from scratch (its base image, configuration and scripts were), and
  shellcheck 0.9 itself (CI's).

<!-- #endregion body -->
