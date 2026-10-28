# High availability

How nexc-engine keeps a run going when the machine executing it goes away, and what each failure
costs in time (RTO) and in work (RPO).

<!-- #region model -->
## The model

A run is executed by one backend replica at a time. That replica holds a **lease** on the run in
PostgreSQL: the run row carries the instance that holds it (`claimed_by`), when the lease runs out
(`lease_expires_at`) and a counter that goes up every time the run changes hands (`lease_epoch`).

| Moment               | What happens                                                                                                                                                                                                  |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Claim                | A dispatcher takes a queued run with `FOR UPDATE SKIP LOCKED`, sets the lease and raises the epoch.                                                                                                           |
| Renewal              | The holder renews every lease it holds every third of `NEXC_RUN_LEASE_SECS` (default 30 s, so every 10 s), in one statement for all of them. A renewal that finds the epoch moved tells the run it lost its lease. A holder whose leases the database has not confirmed for two thirds of `L`, counted from when it sent the last renewal that went through, stops those runs by itself: a third of `L` before a peer can take them over. |
| Takeover             | A run whose lease ran out is claimed by another replica with one atomic `UPDATE` that raises the epoch. The new holder resumes the run from what is in the database.                                          |
| Fencing              | Every write the executor makes for a run (node status, outputs, spans, usage, artifacts, approval requests, the run's status) runs in a transaction that first locks the run row at the epoch it holds. A former holder's write is refused. An artifact's bytes go to a file of their own and its record is pointed at them under the fence, so a late artifact of a former holder replaces neither the record nor the bytes of the holder's. |
| Release              | A replica that shuts down stops its runs, flushes their traces and releases their leases, so a peer takes over at once instead of after the lease runs out.                                                    |

Resuming a run keeps everything that was committed:

| A node that was    | After the takeover                                                                                                                                                         |
| ------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Succeeded          | Keeps its output; its successors build on it. Nothing is run again.                                                                                                        |
| Failed, skipped    | Stays as it ended; its descendants are skipped.                                                                                                                            |
| Running            | Is queued again with `attempt + 1` and a `policy` span named `interrupted` on its trace. A node interrupted on its last attempt fails. An attempt cut short by a hand-off on shutdown (a rolling update, a node drain) does not count against `NEXC_MAX_ATTEMPTS`, so a deploy never uses up a node's attempts. |
| Waiting for approval | Stays waiting and is re-attached: the decision, taken on any replica, resumes it. The new attempt asks again and is answered with the decision already taken, when it asks for the same call (the same tool and argument digest) and the decider may still decide it; otherwise the approvers are asked again. |
| Queued             | Runs when its upstream nodes have succeeded, as before; one queued again by a holder that stopped before it started it runs the attempt after the interrupted one.       |

The trace's hash chain continues from the last span written, so `GET /runs/{rid}/trace/verify`
stays `ok` across a takeover. Commands sent into the run (`run_commands`, the cancel of one
sub-agent) are addressed to whoever holds the lease: a command the former holder never took is
taken and delivered by the new one. A run cancelled while nobody held it is cancelled by the new
holder before any node is queued again: nothing more is sent to the runtime or the provider.

Execution is at least once, recording is exactly once. A node attempt cut short by a crash is run
again, so a tool it called before the crash may be called twice; its success and its output are
recorded once, because the former holder's writes are refused. What each attempt spent at the
provider is booked once to the usage ledger and the agent's budget, a refused attempt's too (tokens
it spent are spent); the run's totals count the attempts of its holders.

<!-- #endregion model -->
<!-- #region failure-modes -->
## Failure modes

`L` is the lease (`NEXC_RUN_LEASE_SECS`, 30 s by default). The dispatcher of every replica looks
for expired leases every 2 s.

### A replica crashes while a node runs

| Aspect    | Behaviour                                                                                                                                                                                                                                                                 |
| --------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Detection | The lease is not renewed and runs out at most `L` after the last renewal. The runtime sees its stream to the dead replica close and stops the attempt.                                                                                                                  |
| Recovery  | A peer takes the run over with the next epoch. The running node is queued again with `attempt + 1` and an `interrupted` span; succeeded nodes keep their outputs; the trace continues from the last span written.                                                       |
| RTO       | `L + 5 s` from the crash to the node running again (35 s by default).                                                                                                                                                                                                    |
| RPO       | The work of the interrupted attempt, and the spans it had not written yet (at most one batch, normally milliseconds of spans). Succeeded nodes, approvals and usage already written: nothing.                                                                            |

### A replica crashes while a node waits for an approval

| Aspect    | Behaviour                                                                                                                                                                                                                                                              |
| --------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Detection | As above: the lease runs out.                                                                                                                                                                                                                                          |
| Recovery  | The node stays `waiting_approval` and the new holder re-attaches it. Approvers decide in the console as usual, on any replica: the approvals table is the source of truth. When the decision lands, the node runs again with `attempt + 1`; the runtime asks again for the same tool and arguments and is answered at once with the decision already taken. A request that expires fails the node, as it would have without the crash. |
| RTO       | `L + 5 s` to re-attach; once re-attached, the node resumes within a second of the decision.                                                                                                                                                                             |
| RPO       | Nothing: the request and the decision are rows in `approvals`.                                                                                                                                                                                                       |

### A replica loses PostgreSQL (network partition)

| Aspect    | Behaviour                                                                                                                                                                                                                                                                                          |
| --------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Detection | The replica cannot renew; its `/readyz` fails, so the load balancer stops sending it requests. Two thirds of `L` after it sent its last renewal that went through, it stops its runs by itself (a `lost` event); its peers see the lease run out a third of `L` later.                                                                                                                                                                    |
| Recovery  | A peer takes the run over as after a crash. A write the cut-off replica had begun keeps the run row locked only until PostgreSQL ends its idle transaction, `L` after its last statement (the executor's transactions set `idle_in_transaction_session_timeout` for themselves), so it does not hold the takeover off until TCP notices the dead peer. The cut-off replica has stopped its copy of the run before the takeover: its node attempts in flight end when their stream to the runtime closes, so two replicas do not execute the run at once (a tool call the runtime had already started may still finish). If it could not stop in time (its process stalled for a third of `L`), its writes are refused: when the partition heals, its first write for the run is refused by the fence and recorded (`fenced`) and its next renewal finds the epoch moved. |
| RTO       | `L + 5 s`.                                                                                                                                                                                                                                                                                         |
| RPO       | As after a crash. Nothing is recorded twice.                                                                                                                                                                                                                                                      |

### The agent runtime crashes

| Aspect    | Behaviour                                                                                                                                                                                                                                                                  |
| --------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Detection | The node attempt's gRPC stream fails at once, or after the HTTP/2 keepalive (`NEXC_GRPC_KEEPALIVE_SECS` + `NEXC_GRPC_KEEPALIVE_TIMEOUT_SECS`, 40 s by default) when the pod vanished without closing its connections.                                                         |
| Recovery  | The attempt fails as transient and is retried with full-jitter back-off (2 s to 60 s) up to `NEXC_MAX_ATTEMPTS`. The runtime is stateless, so any runtime replica takes the retry. The backend keeps its lease the whole time: no takeover happens. An approval the crashed attempt was waiting for expires with the attempt. |
| RTO       | The back-off of the next attempt, at most 60 s.                                                                                                                                                                                                                            |
| RPO       | The work of the attempt.                                                                                                                                                                                                                                                   |

### The LLM provider has an outage

| Aspect    | Behaviour                                                                                                                                                                                                                                                       |
| --------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Detection | Calls fail with 429, 5xx or no connection. After `NEXC_LLM_BREAKER_FAILURES` consecutive failures (5) the circuit of that endpoint opens.                                                                                                                       |
| Recovery  | Retryable failures are retried with back-off; an open circuit fails calls at once for `NEXC_LLM_BREAKER_OPEN_SECS` (30 s) and then lets one probe through; `NEXC_LLM_FALLBACKS` moves calls to the next configured provider. A node that runs out of attempts fails and its descendants are skipped. Starting the run again later serves every node that succeeded from the content-hash cache. |
| RTO       | The provider's. nexc adds at most the breaker's open time (30 s) after the provider is back.                                                                                                                                                                    |
| RPO       | Nothing that finished: succeeded nodes are reused, not recomputed.                                                                                                                                                                                              |

### PostgreSQL fails over

| Aspect    | Behaviour                                                                                                                                                                                                                                                                                                                       |
| --------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Detection | Connections drop; writes and renewals fail; `/readyz` fails on every replica until the new primary answers. The pool reconnects on the next use, and so does each replica's realtime listener (`LISTEN`).                                                                                                                       |
| Recovery  | A replica whose write for a run fails stops executing that run rather than going on with a state the database does not have. Span batches are kept and retried with back-off for up to two minutes. When the new primary answers, the stopped runs' leases run out and a replica takes each over from what was committed. The cache invalidations other replicas sent while a listener was away are lost, so once it listens again the replica drops every entry of its caches (policies, guardrails, LLM settings, agent cards, onto-intelli's reads; a vocabulary is only made stale, still the fallback while onto-intelli is away), and nothing stale is served until `NEXC_CACHE_TTL_SECS` runs out. Realtime events sent in the gap are not replayed: a console catches up on its next refetch. |
| RTO       | The database's failover time plus `L + 5 s`.                                                                                                                                                                                                                                                                                    |
| RPO       | Committed data: nothing with a synchronous standby, the replication lag with an asynchronous one. Node attempts in flight are run again.                                                                                                                                                                                         |

### A rolling update

| Aspect    | Behaviour                                                                                                                                                                                                                                                                                                                        |
| --------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Detection | The replica receives `SIGTERM`.                                                                                                                                                                                                                                                                                                  |
| Recovery  | The replica drains: `/readyz` answers 503, its dispatcher stops claiming, each run it executes is stopped, its chained spans are written and its lease is released with the next epoch. A peer takes each run over within one dispatcher poll. HTTP requests in flight finish; then the process exits. Migrations are additive, so old and new replicas share the database during the rollout; on Kubernetes a new pod applies them in its `migrate` init container before it serves, with no probe that could stop an index build half way. During the first rollout to a version with leases, a replica of the earlier version still cancels its runs when it stops; a run it holds (one it claimed has no lease) is not taken over while it sends heartbeats, and is taken over once its heartbeat is older than `L`. |
| RTO       | About 5 s per run: the drain plus one poll of a peer.                                                                                                                                                                                                                                                                           |
| RPO       | The node attempts in flight, which run again. No span is lost.                                                                                                                                                                                                                                                                  |

<!-- #endregion failure-modes -->
<!-- #region targets -->
## Targets at a glance

| Failure                                  | RTO                          | RPO                                      |
| ---------------------------------------- | ---------------------------- | ---------------------------------------- |
| Replica crash mid-node                   | `L + 5 s` (35 s)             | the interrupted attempt                  |
| Replica crash while waiting for approval | `L + 5 s` to re-attach       | nothing                                  |
| Partition between a replica and PostgreSQL | `L + 5 s`                  | the interrupted attempt; nothing twice   |
| Agent runtime crash                      | ≤ 60 s (next attempt)        | the attempt                              |
| Provider outage                          | the provider's + ≤ 30 s      | nothing finished                         |
| PostgreSQL failover                      | failover + `L + 5 s`         | the database's own RPO; attempts in flight |
| Rolling update                           | ≈ 5 s                        | the attempts in flight                   |

<!-- #endregion targets -->
<!-- #region deploy -->
## Deploying for high availability

Run at least two backend replicas and two runtime replicas against one PostgreSQL that has a
standby, with one data volume (`NEXC_DATA_DIR`: run artifacts, uploaded documents) that every
backend replica mounts, which takes a `ReadWriteMany` volume. A `ReadWriteOnce` volume attaches
to one node, so on it the manifests and the chart run one backend replica (the chart refuses to
render more, or the autoscaler) and keep a rollout's new pod on that replica's node (a required
pod affinity): placed on another node it would wait on a Multi-Attach error and the rollout would
stall. That replica comes back after a crash and survives a rolling update, not the loss of its
node; a drain moves it (its PodDisruptionBudget lets one pod go) and the volume follows it. When
that node fails instead, nothing moves by itself: the pod stays `Terminating` while the node is
`NotReady`, the scheduler still counts it for the affinity, so its replacement stays `Pending`
waiting for the dead node, and the volume stays attached to it. Once the node is really off
(powered down, its VM stopped; not merely unreachable, or two nodes would write to one volume),
taint it `kubectl taint node <node> node.kubernetes.io/out-of-service=nodeshutdown:NoExecute`
(Kubernetes 1.28+), or delete the pod with `kubectl delete pod <pod> --force --grace-period=0`:
the replacement then starts on another node and the volume is detached and follows it. Remove the
taint when the node comes back. With a `ReadWriteMany` volume the chart spreads the replicas
across nodes (`topologySpreadConstraints`), and a peer that takes a run over sees its artifacts.
Every replica gets the same `NEXC_RUN_LEASE_SECS`; each run records the lease its holder works under
(`runs.lease_secs`) and a takeover waits for that one, so a replica set differently still takes a
dead peer's runs over in time, and logs a warning when it does.

| Setting                                  | Where                                    | What it does                                                                                                                       |
| ---------------------------------------- | ---------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `NEXC_RUN_LEASE_SECS`                    | `.env`, Helm `backend.runLeaseSecs`      | The lease (1 to 3600 s, default 30). Shorter takes runs over sooner after a crash and renews more often; `NEXC_ENV=production` refuses less than 10 s, which a slow answer of the database would turn into takeovers. |
| `replicas.backend`, `replicas.runtime`   | Helm values                              | One backend (its `ReadWriteOnce` volume carries one; more need `ReadWriteMany`) and two runtimes by default.                       |
| `backend.dataAccessMode`, `backend.dataStorageClass`, `backend.dataExistingClaim` | Helm values | The backend's data volume and where its pods run. `ReadWriteOnce` (the default) carries one backend replica and keeps a rollout's new pod on its node; `ReadWriteMany` with a class that offers it (NFS, EFS, Filestore, CephFS) takes several, or the autoscaler, spread across nodes. A claim you made is mounted instead of `backend-data`; set `dataAccessMode` to its access mode. The chart's notes say which placement you got. |
| `autoscaling.backend`, `autoscaling.runtime` | Helm values                          | Optional HorizontalPodAutoscalers on CPU (off by default). When one is on, `replicas` no longer sets that Deployment's size; keep `minReplicas` at 2 or more, and give the backend a `ReadWriteMany` data volume. |
| `podDisruptionBudgets.maxUnavailable`    | Helm values                              | How many pods of each tier a node drain may evict at once (default 1).                                                             |
| `backend.terminationGracePeriodSeconds`  | Helm values                              | Time a backend pod gets to drain on `SIGTERM` (default 60 s; the hand-off itself takes at most 15 s).                              |
| `NEXC_BACKEND_REPLICAS=2`                | `.env`, Docker Compose                   | Two backend replicas on one host, kept by every later `docker compose up -d`. The frontend's nginx looks `backend` up again every 10 s, so it reaches a new or recreated replica without a restart. Each gets 30 s to drain (`stop_grace_period`). |
| `postgres.maxConnections`, `postgres.reservedConnections`, `config.NEXC_DB_MAX_CONNECTIONS` | Helm values | The connection budget, below. |

Every write the executor makes for a run is one short transaction that sets
`idle_in_transaction_session_timeout` to `L` for itself (`set_config(…, true)`, as `SET LOCAL`
does), so nothing needs configuring on PostgreSQL or a managed database. A connection pooler in
front of it must keep each transaction on one server connection, as PgBouncer does in session and
transaction pooling (not in statement pooling); the realtime listener and migrations need a session
of their own besides (Connections to PostgreSQL, below).

The backend's readiness probe is `/api/v1/readyz`. It answers 503 while the database does not
answer and from the moment the replica is told to stop, so a load balancer stops sending it
requests while it hands its runs over. The Helm chart probes it every 2 s (5 s to answer) and
takes the pod out of the Service after three failures in a row, so a database that is slow for a
moment does not take every replica out at once; a pod being deleted leaves the Service at once
anyway.

### Connections to PostgreSQL

Every backend pod keeps a pool of `NEXC_DB_MAX_CONNECTIONS` connections (20 by default), and its
realtime listener holds one of them for as long as it runs. Two of them (from a setting of 4 up)
form a small pool of their own that renews the leases of the pod's runs, so a burst of requests
never holds a renewal up; the pod opens no more than the setting. Under load each pool fills, so the
pods that can run at once must fit under PostgreSQL's `max_connections` together:

```
(most backend pods + 1 during a rollout) x NEXC_DB_MAX_CONNECTIONS
  + onto-intelli replicas x its maxConnections (its rollouts add no pod)
  + a few reserved (psql, backups, the superuser's own three)   <=   max_connections
```

Past it PostgreSQL refuses new connections (`too many clients`) and requests fail with 5xx. The
Helm chart does the sum: unless `config.NEXC_DB_MAX_CONNECTIONS` is set, each pod's pool is its
share of `postgres.maxConnections` (100, PostgreSQL's default) less `postgres.reservedConnections`
(10) and onto-intelli's pools, among the autoscaler's maximum (or `replicas.backend`) plus one,
at most 20: 20 for the one replica, 12 with the autoscaler at 6, 14 for two replicas beside
onto-intelli. With the
chart's PostgreSQL a pool that does not fit refuses to render, and `postgres.maxConnections`
other than 100 is passed to it as `-c max_connections` (which restarts it). With your own
database, set `postgres.maxConnections` to its limit. onto-intelli's rollout stops a pod before
it starts its replacement (`maxSurge: 0`), so its pools never overlap; with one replica it is away
for that moment, and the backend serves the vocabularies it holds meanwhile. The
kustomize manifests compute nothing: the one replica plus one at 20 each is 40 of 100 there
(`deploy/k8s/config.env`). Compose runs `NEXC_BACKEND_REPLICAS` pools of 20 against 100.

A pooler in front of PostgreSQL has to give the backend sessions: its realtime `LISTEN` and the
advisory lock `nexc migrate` takes hold a session, which PgBouncer in transaction pooling does not
keep. Point `NEXC_DATABASE_URL` at a session pool or at PostgreSQL itself.

## Shutting down

On `SIGTERM` (or Ctrl-C) a backend replica drains, in this order:

1. `/readyz` answers 503 and the dispatcher claims and takes over nothing more.
2. Every run it executes stops at once. The node attempts in flight are cancelled on the runtime.
3. Every span chained so far is written, then the lease is released: it runs out now and its
   epoch goes up, so nothing the replica still sends is accepted.
4. A peer's dispatcher takes each run over at its next look (every 2 s) and resumes it.
5. After the runs are handed off (at most 15 s), the HTTP server stops accepting connections and
   finishes the requests in flight. Then the process exits.

A replica that is killed without draining (`SIGKILL`, a node that vanished) loses nothing that
was written: its runs are taken over once their lease runs out.

## Seeing it happen

| Where                                    | What it shows                                                                                                                                                     |
| ---------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| The run's trace                          | A `policy` span named `interrupted` on each node whose attempt was cut short, and `reattached` on each node that waits for an approval again, with the former and the new instance and the epoch. |
| `nexc_run_lease_events_total{event}`     | Leases `claimed`, `taken_over`, `released`, `lost` and `fenced`, per replica (`/metrics`). A steady rate of `taken_over` means replicas die; any `fenced` means one came back after it was replaced. |
| `run_lease_events`                       | The table behind it: every event of every run's lease with the instance, the epoch, the former holder and a detail.                                                 |
| `GET /api/v1/runs/{rid}/trace/verify`    | Still `ok` after a takeover: the chain continues from the last span written.                                                                                        |

## Retrying writes

A client whose request was cut off by a failover may not know whether it took effect. Sending
`Idempotency-Key` with the write makes the retry safe: starting a run, creating an issue or a
node, and approving or denying an approval answer a retry with the first response instead of
doing it twice (the rules are under "Idempotent retries" in [the contract](CONTRACT.md), section 3).

<!-- #endregion deploy -->
