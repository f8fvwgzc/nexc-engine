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
| Renewal              | The holder renews every lease it holds every third of `NEXC_RUN_LEASE_SECS` (default 30 s, so every 10 s), in one statement for all of them. A renewal that finds the epoch moved tells the run it lost its lease. |
| Takeover             | A run whose lease ran out is claimed by another replica with one atomic `UPDATE` that raises the epoch. The new holder resumes the run from what is in the database.                                          |
| Fencing              | Every write the executor makes for a run (node status, outputs, spans, usage, the run's status) runs in a transaction that first locks the run row at the epoch it holds. A former holder's write is refused. |
| Release              | A replica that shuts down stops its runs, flushes their traces and releases their leases, so a peer takes over at once instead of after the lease runs out.                                                    |

Resuming a run keeps everything that was committed:

| A node that was    | After the takeover                                                                                                                                                         |
| ------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Succeeded          | Keeps its output; its successors build on it. Nothing is run again.                                                                                                        |
| Failed, skipped    | Stays as it ended; its descendants are skipped.                                                                                                                            |
| Running            | Is queued again with `attempt + 1` and a `policy` span named `interrupted` on its trace. A node interrupted on its last attempt fails.                                      |
| Waiting for approval | Stays waiting and is re-attached: the decision, taken on any replica, resumes it. The new attempt asks again and is answered with the decision already taken.            |
| Queued             | Runs when its upstream nodes have succeeded, as before.                                                                                                                    |

The trace's hash chain continues from the last span written, so `GET /runs/{rid}/trace/verify`
stays `ok` across a takeover. Commands sent into the run (`run_commands`, the cancel of one
sub-agent) are addressed to whoever holds the lease: a command the former holder never took is
taken and delivered by the new one.

Execution is at least once, recording is exactly once. A node attempt cut short by a crash is
run again, so a tool it called before the crash may be called twice; its success, its output and
its usage are recorded once, because the former holder's writes are refused.

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
| Detection | The replica cannot renew; its `/readyz` fails, so the load balancer stops sending it requests. Its peers see the lease run out.                                                                                                                                                                    |
| Recovery  | A peer takes the run over as after a crash. When the partition heals, the cut-off replica's first write for the run is refused by the fence and recorded (`fenced`), and its next renewal finds the epoch moved: it stops its copy of the run without writing anything else. A replica that keeps running nodes while cut off cannot record their results. |
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
| Detection | Connections drop; writes and renewals fail; `/readyz` fails on every replica until the new primary answers. The pool reconnects on the next use.                                                                                                                                                                                 |
| Recovery  | A replica whose write for a run fails stops executing that run rather than going on with a state the database does not have. Span batches are kept and retried with back-off for up to two minutes. When the new primary answers, the stopped runs' leases run out and a replica takes each over from what was committed. |
| RTO       | The database's failover time plus `L + 5 s`.                                                                                                                                                                                                                                                                                    |
| RPO       | Committed data: nothing with a synchronous standby, the replication lag with an asynchronous one. Node attempts in flight are run again.                                                                                                                                                                                         |

### A rolling update

| Aspect    | Behaviour                                                                                                                                                                                                                                                                                                                        |
| --------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Detection | The replica receives `SIGTERM`.                                                                                                                                                                                                                                                                                                  |
| Recovery  | The replica drains: `/readyz` answers 503, its dispatcher stops claiming, each run it executes is stopped, its chained spans are written and its lease is released with the next epoch. A peer takes each run over within one dispatcher poll. HTTP requests in flight finish; then the process exits. Migrations are additive, so old and new replicas share the database during the rollout. |
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
