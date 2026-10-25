# Infrastructure

What nexc-engine runs on, what can be changed, and how.

<!-- #region database -->
## One database: PostgreSQL

Everything is stored in one PostgreSQL 17 database. There is no SQLite mode and none is planned:
the data layer uses PostgreSQL's full-text search (`tsvector`, GIN), `jsonb`, row claiming with
`FOR UPDATE SKIP LOCKED`, array `UNNEST` writes and, when present, the `pgvector` extension.

The connection is `NEXC_DATABASE_URL`, read when the server starts. It is deliberately not
editable from the browser: the settings that would say where the database is live in that
database, and a web form that repoints a server at another database is a way to take it over.

### Moving to another PostgreSQL (hosted, local, another container)

1. Settings -> Infrastructure -> **Check a connection**: paste the new URL. The server reports
   whether it can reach it, its version, whether pgvector is available, and whether it already
   holds nexc tables. The URL is used once; it is neither stored nor logged.
2. Copy the data: `pg_dump -Fc` from the old database, `pg_restore --no-owner` into the new one.
   Never point a different PostgreSQL image at an existing data volume (text indexes built under
   musl are not valid under glibc and the other way round).
3. Set `NEXC_DATABASE_URL` and restart. An empty database is set up on start: the server applies
   its migrations (`backend/migrations/`, versioned by timestamp, applied once each in order).

For vector search use an image with pgvector: `deploy/postgres/pgvector.Dockerfile` builds one;
`NEXC_DB_IMAGE` and `NEXC_DB_VOLUME` in `.env` select it for `make dev`. `make db-up` builds
`nexc-postgres-pgvector:<tag>` from that Dockerfile when the machine does not have it (it is on
no registry) and pulls any other image it lacks.

<!-- #endregion database -->
## Transferring one workspace to its owner's database

<!-- #region transfer -->
An organisation that wants to hold its own data does not need the whole installation moved:
Settings -> **Data transfer** (the workspace's owner only) copies one workspace to a PostgreSQL
the owner names, and then offers to remove it from this server.

1. The owner pastes `postgres://user:password@host:5432/database` of an empty database and,
   optionally, the `redis://` of the Redis that server will use. **Test connection** reaches both
   without moving anything. The strings are used for that transfer only; what is recorded is the
   target without credentials.
2. The server applies its migrations there and copies the workspace's rows table by table,
   parents before children, in one transaction on the target (`engine/transfer.rs`): the copy
   is there completely or not at all. Per-table counts are kept in `workspace_transfers`.
3. Files kept on disk travel inside the same database: uploaded originals
   (`documents/<document id>`) and run artifacts (`artifacts/<run id>/<node id>/<write id>/<path>`,
   or `artifacts/<run id>/<node id>/<path>` for artifacts written before high availability) are
   written to `workspace_files`, one row per file, with the path they had in the data folder
   (`NEXC_DATA_DIR`). The nexc server that owns the target database writes them back out to its
   own data folder when it next starts (`transfer::restore_files`) and clears the rows' content.
   Nothing to unpack by hand.
4. With a Redis given, the handover is recorded there under `nexc:workspace:<wid>` (the
   target's location and the time), so the receiving side can see that the workspace arrived.
   nexc keeps no state in Redis otherwise — queues are rows and travel with the data.
5. After checking the copy, the owner removes the workspace here. Removal deletes every row of
   the workspace, the uploaded files of its documents and the artifact files of its runs.

Not copied, on purpose: password hashes (accounts are created on the target with a hash nothing
matches), stored API keys (sealed with this server's master key), and sessions.
In production a target that resolves to a loopback, private or link-local address is refused, so
a workspace owner cannot make the server connect into its own network.

### What this does and does not give you for GDPR or HIPAA

It gives a workspace **portability** (a complete copy in the standard schema) and **erasure from
the live database**. It is not, by itself, compliance. Outside what it reaches: this server's
database backups and logs, and whatever was sent to LLM and embedding
providers while the workspace ran here. Encryption in transit to the target is the connection
string's `sslmode`; encryption at rest is the target's. Agreements such as a BAA are between the
organisations, not something the software provides.

What the software does provide, for whoever is building a compliance case on it:

| Requirement                     | Where it is                                                                                                                                                              |
| ------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Access control                  | Workspace roles, team visibility, platform administrators walled off from content on the server                                                                          |
| Strong authentication           | Argon2 passwords, short-lived tokens, lockout after failed attempts, two-factor sign-in with recovery codes                                                              |
| Accountability                  | Per-workspace audit log, platform activity log, each account's own security activity (sign-ins with address, password and factor changes, what administrators did to it) |
| Right of access and portability | A person downloads a copy of their data; a workspace moves to its own database                                                                                           |
| Right of erasure                | Account deletion that empties the account and replaces its name in the logs; workspace deletion that removes files too                                                   |
| Session control                 | Suspension and sign-out-everywhere end live tokens, open streams and sockets at once                                                                                     |
| Backups                         | `make backup`, `make backup-check`, `make restore`                                                                                                                       |
| Encryption at rest              | Stored AI keys and two-factor secrets are sealed with `NEXC_MASTER_KEY`; everything else relies on disk or database encryption of the host                               |

What it does not provide: a log of who _read_ what (only changes are logged), encryption of the
database itself, and anything about the LLM provider's handling of what was sent to it.

<!-- #endregion transfer -->
<!-- #region secrets -->
## Secrets

| Secret                                | How it is kept                                                                                                             |
| ------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| Passwords                             | Argon2id hashes (`security/password.rs`); never recoverable                                                                |
| Access tokens                         | Signed JWTs, 15 minutes; refresh tokens stored as digests and rotated on use                                               |
| LLM and embedding API keys            | AES-256-GCM (`security/secret_box.rs`) with the master key, bound to their owner; only a 4-character hint is ever returned |
| Master key, JWT secret, runtime token | Environment only (`NEXC_MASTER_KEY`, …), generated by `make init`                                                          |

The master key is not changeable from the browser either. Rotating it means decrypting every
stored key with the old one and encrypting it with the new one; a server started with a different
master key cannot read the stored API keys (they must be entered again) but nothing else is lost.
Moving an installation therefore means moving the database **and** the same `.env` secrets.

## Queues and caches

No other service is needed.

- **Background work** (runs, document ingestion) is queued in PostgreSQL: workers claim rows with
  `SKIP LOCKED`, so several server instances share the work and a crashed worker's rows return to
  the queue.
- **Caches** are in process. The hot reads (agent cards, an agent's resolved policy per policy
  version, a workspace's guardrails and LLM settings) are kept per instance until they are
  written: the write drops the entry locally and tells every other replica over PostgreSQL
  `NOTIFY`, the same channel the realtime events travel on; `NEXC_CACHE_TTL_SECS` (60 s) only
  bounds a missed notification, and `0` turns the caches off. A replica whose `LISTEN`
  connection dropped (a failover, an idle timeout on the way, which a `SELECT 1` after 30 s with
  no notification finds when no reset came) drops every entry once it listens again, since what
  was sent meanwhile never reached it. The smaller ones (a workspace's
  memory set for 30 seconds, the list of common words for 10 minutes) expire on their own. Every
  JSON read also carries an `ETag`, so a console that asks again gets a `304` with no body: the
  caller's own `private` and varying by `Authorization`, the documents that are the same for
  everyone `public` by their handler's choice, and sign-in, credentials, tokens and downloads
  `no-store` and never tagged (CONTRACT §3). onto-intelli's reads are kept per caller and request
  for `NEXC_ONTO_READ_CACHE_SECS` (10 s), a read on the wire shared by everyone asking the same,
  and a write nexc makes to a tenant drops them on every replica. At the edge, nginx keeps the
  public documents (the key sets, agent cards) for their `max-age` and serves the build
  pre-compressed; nothing private is kept there. The layers, and why the firewall's token checks
  are never cached, are in [ADR 0001](adr/0001-http3-edge-caching-and-indexes.md), what is never
  kept in [ADR 0003](adr/0003-no-store-secrets-and-narrower-refreshes.md); every layer, with its
  key, lifetime and metric, is tabled in `docs/PERFORMANCE.md` (Caching layers).

Redis or an external queue would be additions, not requirements. The Infrastructure page can
check that a Redis is reachable (`redis://…`), which is as far as the server uses one today.

## What the Infrastructure page shows

Platform administrators (the `admin` role of the installation, not workspace admins) see: the
database location without credentials, its version and size, the pgvector version, applied versus
known migrations, whether the agent runtime answers, the server's embedding model, and where
queues and caches live. API: `GET /api/v1/admin/infrastructure`,
`POST /api/v1/admin/infrastructure/check`.

Below it, **Latency, last 5 minutes**: p50, p99 and max per endpoint from the instance's own
in-process histograms, every HTTP route by its template and every gRPC call the backend made
(`GET /api/v1/admin/latency`; behind a load balancer the answer is one replica's). The same
figures over time, for every replica, come from Prometheus: `/metrics` (admin bearer or
`NEXC_METRICS_TOKEN`) serves `nexc_http_request_duration_seconds`,
`nexc_grpc_client_latency_seconds{service, method}`, `nexc_cache_lookups_total{cache, result}`,
`nexc_llm_breaker_total{provider, event}` beside the run, token, span, firewall, gateway, alert
and A2A counters, and `deploy/grafana/nexc-engine.json` is a dashboard over them (import it into
Grafana with a Prometheus data source; the variable `datasource` picks it).

## gRPC channels, keepalive and the LLM circuit breaker

The backend opens one channel to each gRPC service it talks to (the agent runtime, onto-intelli,
the OTLP collector) when it starts and keeps it for the life of the process: `NEXC_GRPC_POOL_SIZE`
connections (2), each with HTTP/2 keepalive (`NEXC_GRPC_KEEPALIVE_SECS` 30, dropped after
`NEXC_GRPC_KEEPALIVE_TIMEOUT_SECS` 10 without an answer), requests and streams spread over them.
The Agent Firewall server pings its clients the same way. `docker-compose.yml` and the Helm chart
(`backend.grpc`) carry the three settings with the reason to shorten the interval behind a NAT or
a mesh. The `make prove-api` table (`docs/PERFORMANCE.md`) measures the round trips.

Calls to a network LLM provider (Anthropic, an OpenAI-compatible server) go through a circuit
breaker per endpoint: after `NEXC_LLM_BREAKER_FAILURES` (5) consecutive failures that say the
provider is in trouble (429, 5xx, no connection) the circuit opens and every call to that endpoint
fails at once with a retryable error for `NEXC_LLM_BREAKER_OPEN_SECS` (30), instead of each
waiting on its own retries and timeouts; then one probe goes out and its outcome closes the
circuit or opens it again. `nexc_llm_breaker_total` counts the events; `0` failures turns it off.

## HTTP/3 and TLS at the edge

Browsers reach nexc through one edge: the frontend's nginx in Docker Compose (it serves the SPA
and proxies `/api`, `/a2a` and the well-known documents to the backend), the Ingress or the
frontend's own load balancer in Kubernetes. HTTP/3 runs over QUIC, which carries TLS 1.3 inside
it, and a browser only tries it after an HTTPS answer advertised it in an `Alt-Svc` header. So
HTTP/3 is offered by whatever terminates TLS for browsers, never by the services behind it
([ADR 0001](adr/0001-http3-edge-caching-and-indexes.md) says why).

The frontend image is that edge once a certificate is mounted. With `tls.crt` and `tls.key` in
`/etc/nginx/tls`, `/docker-entrypoint.d/41-nexc-tls.sh` writes a second listener from
`nginx/tls-server.conf.template`, serving the same site as plain HTTP on 8080
(`nginx/site.conf`), on port 8443:

- TCP with TLS 1.2 (forward-secret AEAD suites only) and 1.3, serving HTTP/2 and HTTP/1.1;
- UDP with QUIC, serving HTTP/3 (TLS 1.3 only). There is no Retry before the handshake
  (`quic_retry` stays off): it would cost every new connection the round trip HTTP/3 saves, and
  QUIC itself keeps what the server sends an address it has not validated to three times what it
  received (RFC 9000, section 8), so a forged source address cannot turn it into an amplifier;
- `Alt-Svc: h3=":<NEXC_HTTPS_PUBLIC_PORT>"; ma=86400` on every HTTPS answer, which moves browsers
  to HTTP/3 from their next request; where UDP is blocked they stay on HTTP/2 by themselves;
- no 0-RTT (`ssl_early_data off`): early data can be replayed, and a replayed POST is a second
  write. Sessions resume from a cache the workers share, for a day, without session tickets;
- `Strict-Transport-Security` on answers over HTTPS, never for a loopback name (`localhost`,
  `127.0.0.1`, `[::1]`): a browser that trusts a local certificate would otherwise pin https on
  every port of localhost, the plain console on 8080 included.

Without a certificate there is no such listener: plain HTTP on 8080, no `Alt-Svc`. Half of one,
or one nginx (uid 101) cannot read, stops the container rather than leaving it on plain HTTP.
nginx reads the pair when it starts: restart it after a renewal. On the HTTPS listener the scheme
nginx passes to the backend (`X-Forwarded-Proto`) is `https` whatever the request claims. What
was tightened after ADR 0001 (the TLS 1.2 suites, that scheme, the reused upstream connections,
the kustomize component, `externalTrafficPolicy`) is recorded in
[ADR 0002](adr/0002-edge-hardening-and-adaptive-grpc-windows.md).

WebSockets (`/api/v1/graphs/{id}/ws`) stay on TCP. nginx has no WebSocket over HTTP/2 or HTTP/3
(RFC 8441, RFC 9220), so a browser that talks HTTP/3 to the console opens its WebSocket on an
HTTP/1.1 connection of its own, to the same port over TCP, and nginx upgrades it as before; an
`Upgrade` reaches the backend only for a WebSocket. Server-sent events gain the most: every
stream of a page shares one HTTP/2 or HTTP/3 connection instead of taking one of the browser's
six per host, and nginx still passes them through unbuffered.

| Setting                  | Where                | Default      | What it does                                                                                    |
| ------------------------ | -------------------- | ------------ | ----------------------------------------------------------------------------------------------- |
| `/etc/nginx/tls`         | frontend image       | empty        | `tls.crt` (with its chain) and `tls.key`: the HTTPS listener exists while both are there        |
| `NEXC_HTTPS_PUBLIC_PORT` | frontend image       | `443`        | The port browsers reach, named in `Alt-Svc`; the container listens on 8443 whatever it is       |
| `NEXC_HTTPS_PORT`        | `docker-compose.yml` | `8443`       | The host port, published on TCP and UDP; its origin is among the backend's `NEXC_CORS_ORIGINS` |
| `NEXC_TLS_DIR`           | `docker-compose.yml` | `./data/tls` | The directory mounted read-only at `/etc/nginx/tls`                                             |
| `frontend.http3`         | Helm chart           | off          | The TLS Secret, the port and the `frontend-https` Service (below)                               |

### Docker Compose

`make init` writes a certificate for `localhost`, `127.0.0.1` and `::1` into `data/tls` when there
is none, and `make tls-cert` writes it again (`scripts/tls-cert.sh`): signed by mkcert's local CA
when mkcert is installed (`mkcert -install` once), self-signed otherwise, so a browser asks once
before it opens the page. `docker compose up` then serves `https://localhost:8443` over HTTP/2 and
HTTP/3 beside plain HTTP on `NEXC_HTTP_PORT`. For a real host name, put its certificate (the full
chain) and key in a directory uid 101 can read, set `NEXC_TLS_DIR` and `NEXC_HTTPS_PORT=443`, and
add `https://<host>` to the backend's `NEXC_CORS_ORIGINS` in `docker-compose.yml`, which names
the localhost origins only.

### Kubernetes and Helm

ingress-nginx terminates TLS over TCP alone, so the chart's Ingress serves HTTP/1.1 and HTTP/2,
never HTTP/3. Two ways to add it:

1. **The frontend as its own edge.** `frontend.http3.enabled=true` with
   `frontend.http3.secretName` (a `kubernetes.io/tls` Secret, cert-manager's for instance): the
   frontend pods mount it at `/etc/nginx/tls` (mode 0440, the pod's group) and serve HTTPS and
   HTTP/3 on 8443, the Service `frontend-https` publishes them as a LoadBalancer with TCP and UDP
   on `frontend.http3.port` (443), and the frontend's NetworkPolicy admits TCP and UDP 8443. A
   LoadBalancer with mixed protocols is generally available since Kubernetes 1.26, and the
   cloud's load balancer must offer it too. Point the host's DNS at the Service's address instead
   of the Ingress (the frontend proxies `/api` itself), set `NEXC_CORS_ORIGINS` and
   `NEXC_COOKIE_SECURE=true` in `config`, and roll the Deployment after the certificate renews
   (by hand, or with a tool that watches Secrets). `frontend.http3.service` sets the type, the
   annotations and `externalTrafficPolicy`. Keep that `Local`, which keeps the client's address:
   with `Cluster` every client arrives with a node's address, so the backend's per-address limits
   (five failed sign-ins a minute) lock out every client behind that node at once. Without Helm,
   the kustomize component `deploy/k8s/http3` does the same.

   ```sh
   kubectl -n nexc create secret tls nexc-frontend-tls --cert=tls.crt --key=tls.key
   helm upgrade nexc deploy/helm/nexc-engine -n nexc --reuse-values \
     --set frontend.http3.enabled=true --set frontend.http3.secretName=nexc-frontend-tls
   ```

2. **HTTP/3 in front of the Ingress**, which stays as it is: a Gateway API implementation that
   terminates QUIC (Envoy Gateway and Istio's gateways can), or a CDN or cloud load balancer that
   speaks HTTP/3 to browsers and HTTP/1.1 or HTTP/2 to the Ingress (CloudFront, Cloudflare,
   Fastly, Google Cloud's external Application Load Balancer, among others). That edge sends
   `Alt-Svc` itself and nothing in nexc changes; leave `frontend.http3` off.

### Behind a CDN

What nexc answers already says what an edge cache may keep, so a CDN in front (of the Ingress,
or of the frontend in Compose) needs no rules of its own beyond respecting `Cache-Control`:

| Path                                                                              | `Cache-Control`                                                                                       | At the edge                                       |
| --------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- | ------------------------------------------------- |
| `/assets/*`, Vite's build files with a hash in the name                           | `public, max-age=31536000, immutable`; `no-store` on a miss                                           | Kept for a year; a new build has new names        |
| `/`, `/index.html` and every client-side route                                    | `no-cache`                                                                                            | Kept, and revalidated (ETag) on every request     |
| `/favicon.svg`, `/theme-init.js`                                                  | `no-cache`                                                                                            | The same                                          |
| `/api/*`, `/a2a/*`, but the public reads below                                    | the backend's: `no-store`, or `private, no-cache` with `Vary: Authorization` on a read it tags        | Never kept                                        |
| `/.well-known/jwks.json`, `/api/v1/agent-tokens/jwks.json`, `/api/openapi.json`   | the backend's: `public, max-age=300, stale-while-revalidate=60`, with an ETag                         | Five minutes (one more stale)                     |
| `/.well-known/agent.json`, `/a2a/agents/{principal}/card`                         | the backend's: `public, max-age=30`, with an ETag                                                     | 30 seconds                                        |

The chart's Ingress sends `/api`, `/a2a` and the well-known documents straight to the backend, so
there they get no nginx copy; the rest of the table holds either way. Never add a rule that keeps
any of `/api` longer than its `Cache-Control` says or rewrites that header, let WebSockets through
(`/api/v1/graphs/*/ws`) and do not buffer `text/event-stream`.

The frontend's nginx serves the build compressed once, when the image is built (`gzip_static`),
compresses the rest with gzip (never `text/event-stream`, which would buffer the stream), keeps
the descriptors of its static files open (`open_file_cache`), and keeps up to 32 idle HTTP/1.1
connections to the backend: every proxied path clears the `Connection` header, except on a
WebSocket upgrade. Its one cache of responses holds the key sets and the agent cards
(`nginx/public-cache.conf`, on its `/tmp`): a copy lives for the `max-age` the backend gave it,
one request refreshes an expired copy by its `ETag` while the others are answered from it, the
copy answers while the backend is down, a request with an `Authorization` header neither reads
nor fills it, no answer from there carries an `X-Request-Id` (a kept copy's would be the
request's that filled it), and `X-Cache-Status` says what happened. nginx sends no `Age` with
what it kept, so a cache behind it may hold a key set up to twice its lifetime: put the next key
in the set well before anything is signed with it (`docs/CONTRACT.md` §3).

### Checking it

```sh
curl -sI https://localhost:8443/ | grep -i alt-svc    # h3=":8443"; ma=86400 (-k unless curl trusts the certificate)
curl --http3-only -sI https://localhost:8443/         # HTTP/3 200, from a curl built with HTTP/3
```

`curl -V` lists `HTTP3` among its features when it can speak it (many packaged builds cannot,
macOS's own among them; the first command works with any curl). In Chrome, open devtools,
Network, right-click the column headers and show **Protocol**: `h3` from the second load on (the
first goes over TCP and brings the `Alt-Svc`), `h2` before. Chrome never uses QUIC with a
certificate it does not trust, nor with one from a locally installed CA such as mkcert's
(`ERR_QUIC_CERT_ROOT_NOT_KNOWN`; it stays on HTTP/2). To see `h3` locally, start a separate
profile that trusts this one certificate for QUIC (the hash changes with every certificate):

```sh
spki="$(openssl x509 -in data/tls/tls.crt -noout -pubkey | openssl pkey -pubin -outform der |
  openssl dgst -sha256 -binary | openssl base64)"
open -na "Google Chrome" --args --user-data-dir=/tmp/nexc-chrome \
  --origin-to-force-quic-on=localhost:8443 --ignore-certificate-errors-spki-list="$spki" \
  https://localhost:8443    # Linux: google-chrome with the same flags
```

A public certificate needs nothing of the kind.

### Between the services: gRPC over HTTP/2

HTTP/3 stays at the edge; the backend's channels to the agent runtime, onto-intelli and the OTLP
collector stay gRPC over HTTP/2. What QUIC fixes is the browser's last mile: a lossy, changing
network where one lost packet stalls every stream of a TCP connection, each new connection costs
handshake round trips, and a phone moves between networks. Between containers or pods none of
that holds: the network is a data-centre one, the channels open once when the backend starts and
live as long as it does, kept by HTTP/2 keepalive (`NEXC_GRPC_KEEPALIVE_SECS`), which is also how
a replica notices a peer that vanished (`docs/HA.md`). The gRPC libraries on both ends (tonic in
the backend, grpclib in the runtime) speak HTTP/2 only, and UDP is what NetworkPolicies, service
meshes and L4 load balancers handle least well. Moving inside would cost CPU and moving parts and
buy nothing measurable. [ADR 0001](adr/0001-http3-edge-caching-and-indexes.md) records this
decision, the choices about WebSockets and 0-RTT, and what would make it change. Every channel,
and the Agent Firewall server, sizes its HTTP/2 flow-control window from the bandwidth-delay
product it measures instead of hyper's fixed windows
([ADR 0002](adr/0002-edge-hardening-and-adaptive-grpc-windows.md)).

## Serving it on a host of its own

Everything works on `http://localhost` as it comes. On any other host name a few settings decide
whether a sign-in lasts, whether links point back to the console and whether uploads get through
the edge; none of them shows on localhost.

| Setting | As it comes | On a host of its own |
| --- | --- | --- |
| `NEXC_ENV`, `NEXC_COOKIE_SECURE` | `development` and `false` in `.env` and the manifests; the backend image alone defaults to `production` | Production (or `NEXC_COOKIE_SECURE=true`) marks the refresh cookie `Secure`. A browser keeps such a cookie on `http://localhost` but drops it on `http://<host>`, so users are signed out on the next reload, or when the 15-minute access token runs out. Serve the host over HTTPS; keep `development` and `false` only on a network you trust. |
| `NEXC_PUBLIC_URL` | `http://localhost:8080` | Where the console is reached from outside: A2A agent cards, alert links and the single sign-on callback carry it. Set it to `https://<host>`. |
| `NEXC_CORS_ORIGINS` | Compose: the localhost origins, set under the backend's `environment:` in `docker-compose.yml`, which wins over `.env` | The console calls its own origin (`/api/v1`), so this matters only for a console served from another origin: list it, in `docker-compose.yml` itself. Production refuses a plain `http://` origin other than localhost. A console on another site never gets the refresh cookie back either (`SameSite=Strict`): keep the console and the API on one site. |
| The certificate | `make init` writes one for localhost | Browsers use HTTP/3 only with a certificate they trust: mount the host's (full chain) with a key uid 101 can read. certbot's keys are root's and 0600, and a key nginx cannot read stops the frontend container, plain HTTP included. |

### Request sizes along the path

A request crosses up to three limits on its way in, and the smallest one answers:

| Request | Backend | Frontend nginx (`nginx/site.conf`) | Ingress |
| --- | --- | --- | --- |
| Anything else | 1 MiB (`BODY_LIMIT`, CONTRACT §3) | 2m | 2m (`Ingress nexc`) |
| A knowledge document, `POST /api/v1/workspaces/{wid}/documents` | 50 MiB and 64 KiB of multipart envelope | 51m | 51m (`Ingress nexc-documents`) |
| An A2A task, `POST /a2a/agents/...` | 16 MiB (files inline) | 17m | 17m (`Ingress nexc-a2a`) |

Each edge allows a little more than the backend, so the backend's `413` (a problem document) is
the answer to a body that is too large, not an nginx page. In Kubernetes the Ingress sends `/api`
and `/a2a` straight to the backend; an annotation applies to every path of its Ingress, so the
two larger routes sit on Ingress objects of their own (Helm: `ingress.documentsBodySize`,
`ingress.a2aBodySize`), which ingress-nginx merges with `nexc` into one server. The documents'
path holds the workspace, so it is a regular expression, and ingress-nginx then matches every path
of the host as one (case-insensitive, longest first), which routes them as before. The well-known
documents are `ImplementationSpecific` paths, since ingress-nginx 1.12 and later refuse a dot in
an `Exact` one.

Two limits are left as they are. The backend gives a request 60 s, reading its body included, so a
50 MiB document needs about 7 Mbit/s up. And over HTTP/3, nginx's stream buffer
(`http3_stream_buffer_size`, 64 KiB) bounds one upload to 64 KiB per round trip, about 1.3 MiB/s
at 50 ms, so a large document on a distant link can run out of those 60 s; a larger buffer costs
memory for every stream, so it stays as nginx sets it.

### The backend's address in nginx

The frontend's nginx looks the backend up while it runs, every 10 s (`server ... resolve` in the
`upstream`, nginx 1.27.3 and later; the image has 1.28). `/docker-entrypoint.d/39-nexc-backend.sh`
writes the server and the resolver when the container starts: the name servers of
`/etc/resolv.conf` (Docker's `127.0.0.11` under Compose, the cluster DNS in Kubernetes) and
`NEXC_BACKEND_UPSTREAM`, `backend:8080` by default. nginx's resolver has no search list, so in
Kubernetes a bare name gets the namespace's domain (`backend.<namespace>.svc.<cluster domain>`,
the first search domain there); an address is used as it is. So a backend container that is
recreated or scaled in Compose is reached at its new address within 10 s with no restart of the
frontend, and the image starts where no host is called `backend` (an `/api` request answers 502
until the name resolves).

### Client addresses

With `NEXC_TRUST_PROXY=true` the backend takes a request's address from the last
`X-Forwarded-For` hop, the one the proxy in front of it wrote, and counts sign-in failures per
address (five a minute). That hop must be the client:

- **Compose**: the frontend's nginx writes it from the connection, the client's.
- **The `frontend-https` LoadBalancer** (`frontend.http3`, `deploy/k8s/http3`): keep its
  `externalTrafficPolicy: Local`; `Cluster` replaces every client's address with a node's.
- **ingress-nginx**: it writes the address of whoever connected to it. Behind a cloud load
  balancer that is the load balancer or a node, so everyone behind it shares one address and one
  sign-in budget. Give the controller's own Service `externalTrafficPolicy: Local` (an L4 load
  balancer that passes addresses through), or turn the PROXY protocol on at the load balancer and
  in the controller (`use-proxy-protocol: "true"`), or, behind an L7 load balancer that sends
  `X-Forwarded-For`, set `use-forwarded-headers: "true"` and `proxy-real-ip-cidr` to its ranges
  in the controller's ConfigMap. These are the controller's settings, outside this chart.

### Images

Images are pushed only for release tags (`v*`, `.github/workflows/release.yml`); CI builds them
without pushing. So `latest` and the chart's `appVersion` are the last release, not `main`:
`docker compose up -d` without `--build` (`pull_policy: missing` pulls when there is no local
image) and `helm install` with the default tag run that release, without the issue pages,
projects, artifacts or migrations merged since. A console built from `main` in front of it
answers 404 on those endpoints. Build the images (`docker compose up -d --build`,
`scripts/minikube-up.sh`), push your own and set `NEXC_IMAGE_TAG` (Compose) or `images.*.tag`
(Helm), or tag a release once CI is green.

<!-- #endregion secrets -->
## onto-intelli (optional): the audit's query engine

<!-- #region onto -->
[onto-intelli](https://github.com/f8fvwgzc/onto-intelli) is where the ontology lives
(`docs/ONTOLOGY.md`): graph vocabularies, the workspace map, schema proposals, and the audit's
facts. For the audit, the backend exports the trace to it as ontology facts (ids, names, hosts,
counts and decisions, never content), into each workspace's own tenant, and answers audit
questions from them as the member who asks. Set `NEXC_ONTO_URL` (its gRPC and HTTP endpoint) and
`NEXC_ONTO_TOKEN` (the impersonating token its `onto-intelli.toml` knows the backend by); with
`NEXC_ONTO_MODE=off` (the default without a URL) nothing is exported and the ask routes answer 503.

- In Compose: `make demo-onto`, or `docker compose -f docker-compose.yml -f docker-compose.onto.yml
  --profile onto up -d --build`, builds it from a checkout next to this repository, on a
  PostgreSQL server of its own with pgvector (`onto-postgres`, volume `onto-pgdata`, login `onto`),
  and wires the backend and the console's frame-src to it; `deploy/onto/onto-intelli.toml` is its configuration
  (tenants per workspace, the impersonating token, governance, the nexc-execution package, the
  audit's schema in `deploy/onto/nexc-audit.toml`, the workspace's in
  `deploy/onto/nexc-workspace.toml`, the mock model unless `ANTHROPIC_API_KEY` is set).
- Elsewhere: run `onto serve` with a configuration like `deploy/onto/onto-intelli.toml`.
- `nexc doctor` and Settings → Infrastructure report whether it answers, its version and the
  model it explains with.

What goes out and how far it got: the tables queue every span written, run started and finished,
agent changed and every change of the projected workspace (teams, people, projects, cycles,
issues, graphs, documents) in `onto_export_queue`; the worker claims rows with `SKIP LOCKED`,
streams them and deletes them once onto-intelli acknowledged, so a span reaches it at least once
and instances share the work; `onto_export_cursors` holds each workspace's backfill and
projection (each done once, the first time the feature sees the workspace) and its last error,
and `onto_projection` what the projection put there. Moving to another onto-intelli is a restart
with a new `NEXC_ONTO_URL`, `DELETE FROM onto_export_cursors`, `DELETE FROM onto_projection` and
`nexc onto migrate` after `DELETE FROM onto_vocabulary_migrations` (graph vocabularies live in
onto-intelli: copy its tenants with it, or the graphs start from the starter set there).

<!-- #endregion onto -->