---
id: data-pipeline-design
name: Data pipeline design
category: data
description: Designs batch and streaming pipelines (ELT, orchestration, CDC, idempotent loads, backfills, late data and watermarks) and specifies how each stage recovers from failure.
keywords: data pipeline, etl, elt, batch, streaming, orchestration, airflow, cdc, change data capture, backfill, idempotent, watermark, late data, kafka, exactly-once
source: Apache Spark - Structured Streaming programming guide | https://spark.apache.org/docs/latest/streaming/apis-on-dataframes-and-datasets.html
source: Databricks - Medallion lakehouse architecture | https://docs.databricks.com/aws/en/lakehouse/medallion
source: Debezium - Architecture | https://debezium.io/documentation/reference/stable/architecture.html
source: Google Cloud - Professional Data Engineer certification | https://cloud.google.com/learn/certification/data-engineer
---
## When to use
Designing, reviewing or debugging a pipeline from sources to a warehouse, lakehouse or serving store; choosing batch or streaming; planning a CDC feed, a backfill or a recovery.

## Decision rules
- **Freshness SLA picks the mode**: daily/hourly -> scheduled batch; minutes -> triggered incremental (micro-batch); seconds -> continuous streaming. Cost rises as latency falls, so ask what decision the freshness serves.
- **ELT by default** when the target computes well (BigQuery, Snowflake, Databricks): land raw, transform in SQL/dbt. **ETL** when data must be masked or reduced before landing (PII, egress cost).
- **Log-based CDC** (Postgres logical replication, MySQL binlog via Debezium/Kafka Connect, or a managed service) beats `updated_at > last_watermark` polling, which misses hard deletes and same-timestamp updates.
- **Layers**: bronze = raw, append-only, plus `_source_file`, `_ingested_at`, `_batch_id`; silver = typed, deduplicated, conformed; gold = business aggregates or star schemas. Never ingest straight into silver: schema drift or corrupt rows then break the clean layer.

## Method
1. Contract first: sources, rows/day and peak, freshness SLA, retention, data class, consumers, owner.
2. DAG: extract -> land -> validate -> transform -> publish, one responsibility per task.
3. **Idempotency**: same input + same logical interval => same output. Use `MERGE` on a business key, or overwrite the target partition; never blind `INSERT` on retry. Record a run id.
4. Parameterise by the logical data interval, never `now()`. A backfill is then a loop over intervals. Orchestrator (Airflow, Dagster, Lakeflow Jobs, Cloud Composer): retries with exponential backoff, task timeouts, catch-up off unless intended, capped concurrency for backfills, data-arrival triggers.
5. **Streaming**: use event time; watermark = max event time seen - allowed lateness. State for a window is dropped once the watermark passes its end. Spark guarantees only that data within the threshold is counted; later data may or may not be, so route it to a dead-letter table and repair with a periodic batch correction. Append mode emits only finalised windows; update mode emits changed rows. Checkpoint offsets and state; exactly-once needs an idempotent or transactional sink (e.g. `foreachBatch` deduplicated on `batch_id`). Kafka and foreach sinks are at-least-once.
6. **CDC apply**: initial snapshot, then the change stream; order per key by log position (LSN/binlog offset); apply Debezium ops (c create, u update, d delete, r snapshot read) with `MERGE`; keep deletes as tombstones or `is_deleted`; dedupe at-least-once delivery on (key, position); handle schema-change events.
7. Gate each layer: row-count reconciliation, null/duplicate keys, freshness; fail or quarantine.
8. Observe: rows in/out; freshness lag = now - max(event time) per table; consumer lag = latest offset - committed offset; cost per run; alert on SLA breach.

## Backfill checklist
- Size it: intervals x runtime x cost; throttle; isolate compute from production runs.
- Write to staging then swap, or overwrite partitions; reconcile counts with the source.
- Re-run dependents in topological order; tell consumers; record what changed.

## Output
Design note: SLA and volume table; text diagram; per-stage table (input, output, key, write mode, idempotency method, checks); failure and recovery plan; backfill plan; cost estimate; risks and open questions.

## Pitfalls
- Watermarks on processing time; with several input streams the global watermark follows the slowest by default.
- Claiming exactly-once with an at-least-once sink.
- Timezone drift: store and partition in UTC.
- Streaming small files: schedule compaction.
- Silent partial loads: compare counts, not just job status.
