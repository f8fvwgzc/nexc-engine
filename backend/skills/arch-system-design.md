---
id: arch-system-design
name: System design
category: architecture
description: Designs scalable, reliable systems from measurable quality scenarios, sizes them with back-of-the-envelope estimates, and picks caching, queueing, partitioning and consistency models with explicit trade-offs.
keywords: system design, scalability, capacity planning, back-of-the-envelope, caching, message queue, cap theorem, pacelc, consistency model, availability, latency, throughput, sharding, replication, quality attributes, nfr
source: iSAQB - CPSA Foundation Level curriculum 2025.1 | https://public.isaqb.org/curriculum-foundation/curriculum-foundation-en.html
source: AWS Well-Architected Framework - The pillars of the framework | https://docs.aws.amazon.com/wellarchitected/latest/framework/the-pillars-of-the-framework.html
source: donnemartin - The System Design Primer | https://github.com/donnemartin/system-design-primer
---
## When to use
A new system or major change where load, availability, latency or data growth drives the design; design reviews; "will it scale" questions.

## Method
1. Clarify use cases, actors, read/write ratio, data size and growth, constraints (budget, regulation, team skills).
2. Write qualities as six-part scenarios: source, stimulus, artifact, environment, response, response measure ("at 3x peak, p99 checkout < 400 ms"). Check all nine ISO/IEC 25010:2023 characteristics: functional suitability, performance efficiency, compatibility, interaction capability, reliability, security, maintainability, flexibility, safety.
3. Estimate capacity, then choose the simplest design that meets the scenarios.
4. Find the bottleneck per tier (CPU, memory, IOPS, network, locks); scale in cost order: vertical, cache, read replicas, async queue, partition, multi-region.
5. Decide consistency per entity, not per system. Record trade-offs as ADRs; define SLOs and the metrics that prove them.

## Formulas
- Average QPS = DAU x requests per user per day / 86,400. Peak QPS = average x peak factor (2-5 typical; measure it).
- Storage = items per day x bytes per item x retention days x replication factor (add ~30% for indexes).
- Little's law L = lambda x W (L in-flight requests, lambda arrival rate, W time in system): connections ~ peak QPS x latency in seconds.
- Nodes = peak QPS / (per-node QPS x target utilisation 0.6-0.7), then add N+1 or N+2 for failure.
- Availability in series A = A1 x A2; redundant in parallel A = 1 - (1-A1)(1-A2). Downtime = (1 - A) x period: 99.9% ~8.76 h/year (~43.8 min/month); 99.99% ~52.6 min/year.
- Fan-out tail: if each of n parallel calls is slow with probability p, P(request slow) = 1 - (1-p)^n; n=100, p=1% gives ~63%. Hedge requests, cut fan-out, or set per-call budgets.
- Orders of magnitude: RAM access ~100 ns; SSD random read ~0.1 ms; same-datacentre round trip ~0.5 ms; cross-continent ~100-150 ms.

## Decision rules
- CAP: under a partition choose consistency (reject or time out) or availability (serve possibly stale). PACELC: else, with no partition, trade latency against consistency.
- Consistency ladder: linearizable > sequential > causal > read-your-writes/monotonic reads > eventual. Balances, stock decrements and uniqueness need strong; feeds, counters and recommendations tolerate eventual.
- Caching: cache-aside by default; write-through for read-after-write freshness; write-behind only if loss is acceptable. TTL with jitter, stampede protection (request coalescing, early refresh), one owner for invalidation; track hit ratio.
- Queues absorb bursts and decouple: size by arrival rate vs consumer throughput, bounded length, backpressure, retries with exponential backoff and jitter, dead-letter queue.
- Partition keys: high cardinality and even access; watch hot keys; consistent hashing limits rebalancing.
- Resilience: timeout on every remote call, retry only idempotent operations, circuit breaker, bulkhead, load shedding, graceful degradation.

## Output
Requirements and assumptions; quality scenario table; capacity estimate with arithmetic; C4 container view; data model with consistency per entity; scaling and failure modes; SLOs; cost drivers; risks and ADR list.

## Pitfalls
Designing for imagined scale; averages instead of p95/p99; fan-out tail latency; retry storms; cache as source of truth; distributed transactions across services; no headroom for failover.
