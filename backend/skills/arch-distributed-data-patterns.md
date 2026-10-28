---
id: arch-distributed-data-patterns
name: Distributed data patterns
category: architecture
description: Keeps data consistent across services with events, transactional outbox, sagas, CQRS and idempotent consumers, and evolves message schemas safely under at-least-once delivery.
keywords: event-driven, transactional outbox, saga, cqrs, event sourcing, idempotent consumer, exactly-once, at-least-once, kafka, schema evolution, schema registry, compensating transaction, cdc, microservices data
source: microservices.io - Pattern: Transactional outbox | https://microservices.io/patterns/data/transactional-outbox.html
source: microservices.io - Pattern: Saga | https://microservices.io/patterns/data/saga.html
source: Confluent - Schema evolution and compatibility | https://docs.confluent.io/platform/current/schema-registry/fundamentals/schema-evolution.html
source: Confluent - Kafka message delivery guarantees | https://docs.confluent.io/kafka/design/delivery-semantics.html
---
## When to use
Services that own their data yet must agree: orders, payments, inventory, integrations, read models, audit feeds.

## Method
1. Assign each aggregate to exactly one owning service; no shared writable database.
2. Per interaction choose a synchronous call (caller needs the answer now), an event (a fact happened) or a command (a request to act).
3. Make each state change and its outgoing message atomic: transactional outbox or event sourcing. Never dual-write database then broker.
4. Assume at-least-once delivery and make every consumer idempotent.
5. Model multi-service transactions as sagas with compensations.
6. Version schemas under a compatibility mode enforced in CI or a registry.

## Patterns
- Transactional outbox: business row and outbox row in one local transaction; a relay publishes by polling (simple, adds latency and load) or log tailing / CDC such as Debezium (low latency, more operations). The relay can publish twice, so consumers dedupe. Use the aggregate id as partition key to keep per-aggregate order.
- Saga: local transactions in sequence; on failure run compensations in reverse. Orchestration (central state machine, easy to trace) vs choreography (events only; fine for 2-4 steps). Classify steps as compensatable, pivot (point of no return) or retriable (after the pivot, must eventually succeed). Missing isolation needs countermeasures: semantic lock (PENDING state), commutative updates, pessimistic view (reorder steps), reread value, version file, by value (pick per request risk).
- CQRS: write model separate from read models built from events; use when read shapes or scale differ. Expose staleness (version or as-of time). Event sourcing makes events the source of truth: plan snapshots, upcasting of old events, and erasure (crypto-shredding for personal data).
- Idempotent consumer: record processed message ids under a unique constraint in the same transaction as the side effect, or make the effect naturally idempotent (upsert, set-to-value, conditional update on version). API POSTs take an idempotency key kept at least as long as the client retry window.

## Delivery semantics
- At-most-once: commit offset before processing; can lose. At-least-once: process, then commit; can duplicate (common default). Effectively-once = at-least-once + idempotent handling.
- Kafka: the idempotent producer stops broker-side duplicates per partition; transactions plus isolation.level=read_committed give exactly-once for read-process-write inside Kafka only. For an external store, commit the offset in the same transaction as the output.
- Order holds only within a partition; carry a sequence or version so consumers drop stale events.
- Poison messages: bounded retries with backoff, then a dead-letter topic with alerting and a replay tool.

## Schema evolution
Modes (Schema Registry naming): BACKWARD, the default (new readers read old data; upgrade consumers first); FORWARD (old readers read new data; upgrade producers first); FULL (both); *_TRANSITIVE (checked against all versions); NONE. FULL lets either side upgrade first; Protobuf: prefer BACKWARD_TRANSITIVE. Safe: add optional fields with defaults. Never reuse field names or Protobuf tag numbers (reserve them). Rename = add new + deprecate old. Breaking change = new event version or topic with a dual-publish window.

## Output
Event catalogue (name, owner, schema, key, ordering, retention, consumers); saga state diagram with compensations; failure matrix (crash at each step and outcome); idempotency design; compatibility mode per subject.

## Pitfalls
2PC across services; events that mirror internal tables; personal data copied into every consumer; non-idempotent handlers; unbounded retries; assuming global order.
