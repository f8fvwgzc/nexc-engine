# Architecture decision records

Decisions that shape more than one part of nexc-engine, with what was there before, what was
decided, what it costs and how it was checked. A record is never rewritten after the fact: a later
decision that changes one gets a record of its own that says which one it replaces.

| Record | Decision |
| --- | --- |
| [0001](0001-http3-edge-caching-and-indexes.md) | HTTP/3 between the browser and the edge (HTTP/2 between the services), caching in layers including the gRPC reads of onto-intelli, and an index for every foreign key |
| [0002](0002-edge-hardening-and-adaptive-grpc-windows.md) | Extends 0001: TLS 1.2 limited to forward-secret AEAD suites, a scheme the client cannot claim on the HTTPS listener, every proxied location on the upstream keepalive pool, HTTP/3 with kustomize and `externalTrafficPolicy: Local`, and adaptive HTTP/2 windows on the gRPC channels |
| [0003](0003-no-store-secrets-and-narrower-refreshes.md) | Extends 0001: secrets and downloads `no-store` and untagged, `Vary: Authorization` on the caller's own reads, the agent-token key set and the API description public, the edge's static headers, and refreshes narrowed to what changed, in the console and between replicas |
| [0004](0004-poll-indexes-hot-updates-and-concurrent-builds.md) | Extends 0001: indexes for the polls and lists that read whole tables, HOT updates on runs, agents and nodes, invariants as unique indexes, cheaper export triggers, index builds that hold no writes, and a schema test that catches every repeated index |
| [0005](0005-deploying-beyond-localhost.md) | What breaks between localhost and a cluster: per-route body limits at every edge, nginx looking the backend up while it runs, one node for a `ReadWriteOnce` volume, migrations in an init container, a connection budget the chart computes, and caches dropped when the realtime listener reconnects |

Records 0002 to 0004 were implemented without building or running anything; each one's "How to
verify" says what has to pass before relying on it.
