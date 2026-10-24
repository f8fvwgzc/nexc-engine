---
id: data-sql-and-performance
name: SQL and query performance
category: data
description: Writes correct analytical SQL and tunes slow or costly queries using query plans, indexes, window functions, partition pruning, scalable joins and warehouse cost controls.
keywords: sql, query performance, explain, query plan, index, window function, partition pruning, join, skew, bigquery, snowflake, postgresql, spark sql, query cost, slow query
source: PostgreSQL 18 - Using EXPLAIN | https://www.postgresql.org/docs/current/using-explain.html
source: Google Cloud - Optimize query computation in BigQuery | https://docs.cloud.google.com/bigquery/docs/best-practices-performance-compute
source: Snowflake - Clustering keys and clustered tables | https://docs.snowflake.com/en/user-guide/tables-clustering-keys
---
## When to use
A query is slow, scans or costs too much, duplicates or drops rows, or new analytical SQL must scale.

## Method
1. **Correctness first**: state the grain of every input and of the result; check row counts before and after each join (fan-out shows as more rows than the driving table).
2. **Read the plan**. PostgreSQL: `EXPLAIN ANALYZE` (includes BUFFERS from v18; add it on older versions; it executes the statement, so wrap DML in `BEGIN ... ROLLBACK`). Read `cost=startup..total`, estimated `rows` versus `actual rows`, and multiply per-loop figures by `loops`. A large estimate/actual gap means stale statistics: run `ANALYZE`. Warehouses: check bytes processed, slot or credit time, partitions scanned and spill in the query profile; BigQuery dry runs give bytes before running.
3. Fix the biggest node first, change one thing, re-measure.

## Indexes (OLTP / PostgreSQL)
- B-tree for equality and range; composite order = equality columns first, then the range column; leftmost prefix rule.
- Covering index (`INCLUDE`) enables index-only scans; partial index for hot subsets (`WHERE status = 'open'`); GIN for jsonb, arrays, full text; BRIN for huge, naturally ordered tables.
- A seq scan is right when most rows qualify.
- Sargable predicates: no function on the indexed column (`ts >= '2026-10-01' AND ts < '2026-10-02'`, not `date(ts) = ...`), no leading `%` in LIKE, no implicit casts.

## Window functions
- Dedupe: `ROW_NUMBER() OVER (PARTITION BY key ORDER BY updated_at DESC) = 1` (`QUALIFY` in BigQuery, Snowflake, Databricks).
- Change: `LAG/LEAD`; running total: `SUM(x) OVER (PARTITION BY k ORDER BY t ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW)`.
- Default frame with `ORDER BY` is RANGE, which groups ties; say ROWS when you mean rows.
- Use windows instead of self-joins; filter rows before windowing.

## Partition pruning and layout
- Filter directly on the partition column with literal or parameter values; casts or functions on it can disable pruning. BigQuery can require a partition filter per table.
- Clustering/sort keys prune blocks inside partitions; Snowflake keys work best with 3-4 columns ordered low to high cardinality.
- `LIMIT` does not cut bytes scanned on columnar warehouses; selecting fewer columns does.

## Joins at scale
- Pre-aggregate before joining; join on integer keys; BigQuery: largest table first.
- Distributed engines: broadcast the small side (Spark auto-broadcasts below `spark.sql.autoBroadcastJoinThreshold`), otherwise shuffle hash or sort-merge.
- Skew (one key holding a large share of rows): adaptive skew-join handling, salting the hot key, or handling it separately.
- Semi-joins (`EXISTS`) instead of `IN` with large or nullable lists; beware `NOT IN` with NULLs (returns nothing).

## Cost control
- Avoid `SELECT *`; materialise repeated expensive transforms; incremental models over full rebuilds; materialised views for hot aggregates; approximate distinct counts (HyperLogLog functions) when exact is not required.
- On-demand BigQuery bills bytes scanned; capacity pricing bills slots. Snowflake bills warehouse credits per second of runtime: right-size, auto-suspend, separate workloads, set resource monitors. Check current price lists locally; do not quote prices from memory.
- Budget a query: bytes = rows x average row width of selected columns, after pruning.

## Output
Before/after table (runtime, bytes or buffers, cost, rows), the plan nodes that changed, the rewritten SQL with dialect named, and any index or layout DDL with its write-cost trade-off.

## Pitfalls
- CTEs assumed to be cached (BigQuery may evaluate them more than once).
- Tuning on a tiny sample.
- Adding indexes that slow writes without measuring.
- `COUNT(DISTINCT)` over joined fan-out hiding duplicates.
