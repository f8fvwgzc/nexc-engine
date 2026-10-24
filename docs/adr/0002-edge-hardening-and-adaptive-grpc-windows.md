# ADR 0002: A tighter edge, HTTP/3 without Helm, and adaptive HTTP/2 windows between the services

- **Status:** accepted, 2026-10-24. **Extends ADR 0001** (Decision 1) and replaces nothing in it.
- **Not built or run yet:** implemented without building or running anything (no compiler,
  test, `nginx -t`, `kubectl kustomize` or `helm template` has seen it); "How to verify" lists
  what has to pass before relying on it.
- **Scope:** `frontend/nginx.conf`, `frontend/nginx/site.conf`,
  `frontend/nginx/tls-server.conf.template`, `deploy/k8s/http3/`, `deploy/k8s/kustomization.yaml`,
  `deploy/helm/nexc-engine` (`frontend.http3.service.externalTrafficPolicy`, `NOTES.txt`),
  `backend/src/rpc.rs`, `backend/src/firewall/mod.rs`

<!-- #region body -->
## Context

[ADR 0001](0001-http3-edge-caching-and-indexes.md) put HTTP/3 at the edge: the frontend's nginx
serves the site a second time on 8443, TLS with HTTP/2 over TCP and HTTP/3 over QUIC, once a
certificate is mounted at `/etc/nginx/tls` (`41-nexc-tls.sh`); Compose publishes it
(`NEXC_HTTPS_PORT`, `NEXC_TLS_DIR`), and the Helm chart's `frontend.http3` puts the pods behind the
LoadBalancer `frontend-https` on TCP and UDP. It kept gRPC over HTTP/2 between the services. Left
as they were after it:

- The HTTPS listener took TLS 1.2 with OpenSSL's default suites, CBC and static RSA key exchange
  among them.
- The scheme nginx passes to the backend (`X-Forwarded-Proto`, which also decides HSTS) was the
  request's own header when it had one, so a client of the HTTPS listener could claim `http`.
- The upgrade map sent `Connection: upgrade` for any `Upgrade` header, and the locations of
  `/a2a/`, the agent cards and the key set set no `Connection` header, so nginx sent
  `Connection: close` there and those requests never reused the keepalive pool of
  `upstream nexc_backend`.
- Without Helm (`deploy/k8s`) there was no way to serve HTTP/3, and the chart's `frontend-https`
  kept Kubernetes' default `externalTrafficPolicy: Cluster`, under which every client arrives
  with a node's address.
- Every gRPC channel the backend opens (`rpc::channel`) and the Agent Firewall server ran on
  hyper 1.11's fixed HTTP/2 windows: 2 MiB per stream and 5 MiB per connection on a client, 1 MiB
  each on a server.

## Decision

1. **TLS 1.2 with forward-secret AEAD suites only.** `tls-server.conf.template` lists ECDHE key
   exchange with AES-GCM or ChaCha20-Poly1305 (`ssl_ciphers`), and lets the client pick among
   them (`ssl_prefer_server_ciphers off`), since every one is sound and the client knows which its
   hardware runs fast. TLS 1.3, and QUIC with it, keep their own suites.
2. **The scheme cannot be claimed on the HTTPS listener.** `$forwarded_proto` is `https` whenever
   nginx terminated TLS itself (`$https` is `on`), whatever the request says; on plain HTTP it is
   still what a TLS-terminating ingress reports, else the connection's scheme. HSTS
   (ADR 0001) follows the same variable.
3. **Every proxied location reuses the keepalive pool.** `$connection_upgrade` is `upgrade` for
   `Upgrade: websocket` only and empty otherwise, and `/a2a/`, the agent cards and the key sets
   set `Connection ""`.
4. **HTTP/3 with kustomize too.** The component `deploy/k8s/http3`, listed (commented out) under
   `components:` in `deploy/k8s/kustomization.yaml`, does what `frontend.http3` does in the chart:
   ports 8443/TCP and 8443/UDP on the frontend container, the TLS Secret `nexc-frontend-tls` at
   `/etc/nginx/tls`, `NEXC_HTTPS_PUBLIC_PORT=443`, the LoadBalancer `frontend-https` on TCP and
   UDP 443, and the NetworkPolicy `frontend-from-load-balancer`, which admits 8443 on both.
5. **`externalTrafficPolicy: Local`** on `frontend-https`, in the component and in the chart
   (`frontend.http3.service.externalTrafficPolicy`, for a LoadBalancer or NodePort type). With
   `Cluster`, the backend's per-address limits (five failed sign-ins a minute) would count every
   client behind one node as one. The chart's `NOTES.txt` says where to point DNS when
   `frontend.http3` is on.
6. **No QUIC Retry** (`quic_retry` is not set, so nginx's default, off, holds). A Retry costs
   every new connection the round trip HTTP/3 saves, and QUIC already limits what a server sends
   to an address it has not validated to three times what it received (RFC 9000, section 8), so
   a forged source address cannot make the edge an amplifier. Turn it on if the edge sees a flood
   of handshakes from forged addresses.
7. **Adaptive HTTP/2 flow control between the services.** Every client endpoint
   (`rpc.rs`, `http2_adaptive_window(true)`) and the Agent Firewall server (`firewall/mod.rs`,
   `http2_adaptive_window(Some(true))`) let hyper size the window from the bandwidth-delay product
   it measures with its pings, starting from HTTP/2's 65,535 bytes, in place of the fixed windows
   above. The channels stay gRPC over HTTP/2 for ADR 0001's reasons (the runtime's gRPC server is
   grpclib, built on the Python `h2` package, which has no HTTP/3 either).

## Consequences

- A TLS 1.2 client without ECDHE and AEAD (nothing a current browser is) can no longer connect
  over HTTPS; plain HTTP on 8080 is unchanged.
- A proxy that terminates TLS and then reaches nginx's HTTPS listener over TLS again is reported
  as `https`, as it should be; one that reaches the plain listener is believed, as before.
- Fewer upstream connections: `/a2a/` and the public documents stop opening one per request.
- With `Local`, a node without a frontend pod fails the load balancer's health check and gets no
  traffic, and traffic is spread by node rather than by pod.
- The adaptive window starts smaller than hyper's fixed one and grows only as far as the
  measured bandwidth-delay product asks. On a short, fast link it may stay under 2 MiB, so a large
  artifact on the runtime's `Execute` stream could move no faster than before, or slower. Nothing
  here was measured.

## How to verify

Nothing in this record has been built, rendered or run. Before relying on it:

1. The image: `docker compose up -d --build`, then `docker compose exec frontend nginx -t`, with a
   certificate in `NEXC_TLS_DIR` (`make tls-cert`) and without one.
2. The suites: `openssl s_client -connect localhost:8443 -tls1_2 -cipher AES128-SHA` fails, and
   `-cipher ECDHE-RSA-AES128-GCM-SHA256` (or the ECDSA one, by the certificate's key) succeeds.
3. The scheme: `curl -sk -D - -o /dev/null -H 'Host: nexc.example' -H 'X-Forwarded-Proto: http'
   https://localhost:8443/` still answers with `Strict-Transport-Security`.
4. Reuse: under a loop of `curl http://localhost:8080/.well-known/agent.json?agent=…`, the
   backend's established connections from nginx (`docker compose exec backend ss -tn`) stay
   about as many as before the loop.
5. Kubernetes: `kubectl kustomize deploy/k8s` with `components: [http3]` uncommented, and
   `helm template` with `frontend.http3.enabled=true` (CI renders it), both show
   `externalTrafficPolicy: Local` on `frontend-https`.
6. The services: `make test-backend`, and the gRPC rows of `make prove-api` (the runtime's
   `Execute` stream, onto-intelli's `Ask`, the Agent Firewall's `AuthorizeAction`) against the
   figures in [PERFORMANCE.md](../../docs/PERFORMANCE.md). If the adaptive window raises the
   per-call cost or slows a large artifact, take `http2_adaptive_window` out of `rpc.rs` and
   `firewall/mod.rs` again.
<!-- #endregion body -->
