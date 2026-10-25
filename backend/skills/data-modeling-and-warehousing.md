---
id: data-modeling-and-warehousing
name: Data modelling and warehousing
category: data
description: Models analytical data with Kimball star schemas, slowly changing dimensions, Data Vault 2.0 and medallion layers, and sets partitioning and clustering for the queries that matter.
keywords: data modeling, data warehouse, star schema, dimensional model, kimball, fact table, slowly changing dimension, scd, data vault, lakehouse, medallion, partitioning, clustering, surrogate key
source: Kimball Group - Dimensional modeling techniques | https://www.kimballgroup.com/data-warehouse-business-intelligence-resources/kimball-techniques/dimensional-modeling-techniques/
source: Wikipedia - Data vault modeling | https://en.wikipedia.org/wiki/Data_vault_modeling
source: Databricks - Medallion lakehouse architecture | https://docs.databricks.com/aws/en/lakehouse/medallion
source: Google Cloud - Introduction to partitioned tables | https://docs.cloud.google.com/bigquery/docs/partitioned-tables
source: Snowflake - Clustering keys and clustered tables | https://docs.snowflake.com/en/user-guide/tables-clustering-keys
---
## When to use
Designing or reviewing warehouse or lakehouse schemas, choosing Kimball or Data Vault, handling history, setting partitioning and clustering.

## Kimball dimensional method
1. Pick the **business process** (orders, shipments, claims), not a department.
2. Declare the **grain** in one sentence ("one row per order line per day"). Never mix grains in one fact table.
3. Choose **dimensions** (who, what, where, when, why, how) with integer surrogate keys; keep the natural key as an attribute.
4. Choose **facts** at that grain: additive (amount), semi-additive (balances: sum across accounts, not time), non-additive (ratios: store numerator and denominator).
- Fact types: transaction; periodic snapshot (state at regular intervals); accumulating snapshot (one row per pipeline instance, updated at milestones); factless (events or coverage).
- **Conformed dimensions** and the **bus matrix** (processes x dimensions) let facts be compared across marts.
- Junk dimension for low-cardinality flags; degenerate dimension (order number on the fact); role-playing (one date dimension as order date, ship date); bridge table for many-to-many.

## Slowly changing dimensions
| Type | Behaviour | Use |
|---|---|---|
| 0 | keep original | birth date |
| 1 | overwrite | corrections, no history needed |
| 2 | new row with `valid_from`, `valid_to`, `is_current` | report "as was" at event time |
| 3 | prior-value column | one planned reorganisation |
| 4 | mini-dimension for fast-changing attributes | banded demographics |
| 5 | type 4 plus a type 1 outrigger to the current mini-dimension row | current band from the base dimension |
| 6 | type 2 rows plus current-value type 1 columns | "as was" and "as is" together |
| 7 | fact carries both surrogate and durable key | either view via joins |
Load type 2 with `MERGE`: hash the tracked attributes, expire the current row when the hash changes, insert the new version. Facts join on the surrogate key valid at the event time.

## Data Vault 2.0
- **Hub** = unique business keys (+ hash key, load timestamp, record source). **Link** = relationship between hubs (hash of the combined keys). **Satellite** = descriptive attributes with history (parent hash key, load timestamp, hashdiff), one parent each.
- Insert-only; raw vault keeps source fidelity; business vault applies rules; PIT and bridge tables speed queries; marts on top are usually star schemas.
- Choose it for many changing sources, audit needs and parallel loading; choose Kimball directly when sources are few and speed to BI matters.
- Hash keys: standardise first (trim, case, delimiter, null token), then hash consistently everywhere.

## Physical design
- Partition on the column most queries filter by, usually a date; keep partitions large (BigQuery suggests clustering instead when partitions would hold under about 10 GB); avoid high-cardinality partition keys.
- Cluster on frequent filter or join columns: BigQuery clusters into sorted blocks; Snowflake advises at most 3-4 key columns, ordered low to high cardinality, and only on large (often multi-TB), read-heavy tables with selective filters, since reclustering costs credits.
- Lakehouse: Delta/Iceberg tables, compaction of small files, clustering on filter columns.

## Output
Bus matrix; per table: grain, keys, SCD type per attribute, partition and cluster keys, load pattern; ER or text diagram; sample queries proving the model answers the questions.

## Pitfalls
- Undeclared or mixed grain causing double counting.
- Summing semi-additive balances over time.
- Type 2 joins on natural key (fan-out).
- Late-arriving dimension rows: insert an inferred member, then update it.
- Snowflaking dimensions for no reason.
- Over-partitioning (tiny partitions, metadata overhead).
