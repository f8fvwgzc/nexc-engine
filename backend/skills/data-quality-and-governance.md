---
id: data-quality-and-governance
name: Data quality and governance
category: data
description: Sets up data quality tests, data contracts, lineage, catalogue entries, classification and privacy controls along the DAMA-DMBOK knowledge areas, and reports quality with measurable dimensions.
keywords: data quality, data governance, dama, dmbok, cdmp, data contract, lineage, data catalog, metadata, data steward, classification, pii, privacy, master data, openlineage, dbt tests
source: DAMA International - CDMP exams | https://cdmp.info/exams/
source: DAMA International - DMBOK 3.0 project | https://dama.org/dama-dmbok-3-0-project/
source: dbt - Model contracts | https://docs.getdbt.com/docs/mesh/govern/model-contracts
source: OpenLineage - Object model | https://openlineage.io/docs/spec/object-model
source: Bitol - Open Data Contract Standard v3.2.0 release | https://github.com/bitol-io/open-data-contract-standard/releases/tag/v3.2.0
---
## When to use
Defining quality rules or SLAs, writing a data contract, setting ownership and classification, tracing lineage, assessing governance.

## Reference frame
- **DAMA-DMBOK2 (revised edition)** is the current body of knowledge behind CDMP; DMBOK 3.0 is in development (kick-off June 2025), unpublished. Its 11 knowledge areas: Data Governance (centre), Data Architecture, Data Modeling and Design, Data Storage and Operations, Data Security, Data Integration and Interoperability, Document and Content Management, Reference and Master Data, Data Warehousing and BI, Metadata Management, Data Quality.
- Roles: data owner (accountable, business), data steward (defines rules and meaning), data custodian (operates the platform). Keep a RACI per domain.

## Quality dimensions and metrics
| Dimension | Metric |
|---|---|
| Completeness | non-null required values / expected values |
| Uniqueness | distinct business keys / rows (target 1.0) |
| Validity | rows passing format/domain/range rule / rows tested |
| Accuracy | matches to a trusted reference / rows checked |
| Consistency | rows agreeing across systems or rules / rows compared |
| Timeliness | now - max(event time) versus the freshness SLA |
State the denominator: validity can be measured on all rows or on non-null rows only.

## Tests to implement
- Schema: names, types, required columns (contract enforced at build).
- Row level: not null, unique, accepted values, regex format, range, referential integrity.
- Dataset level: row-count reconciliation to source (tolerance), volume anomaly (z = (x - mean)/sd over a trailing window, alert at |z| > 3), freshness, distribution shift.
- Severity: error blocks publication; warn records and notifies. Quarantine failing rows with the rule id.

## Data contracts
- Producer-owned, versioned spec of a dataset: owner, schema, semantics, quality rules, SLA (freshness, availability), classification, support channel. Write it in the Open Data Contract Standard (Bitol, LF AI & Data) v3.2.0 (Sep 2026: per-environment variables, field deprecation, vector/map/enum types, AI-usage context); check locally for newer.
- Breaking for consumers: removing or renaming a column, changing a type or precision, removing or loosening a guarantee (NOT NULL, accepted values, SLA), changing meaning or grain. Adding a nullable column is additive (minor). A new constraint breaks producers whose rows violate it. Breaking => new major version plus a deprecation window.
- dbt `contract: {enforced: true}`: checks column names and types before build, writes constraints into the DDL (not_null usually enforced, keys rarely), and errors when state comparison finds a removed column or a changed type or constraint.

## Lineage and catalogue
- Capture lineage as run, job and dataset events (OpenLineage), with schema and column-lineage facets, so impact analysis can answer "what breaks if this column changes".
- Catalogue entry: description, owner, steward, glossary term, classification, PII flags, SLA, quality score, sample queries.

## Classification and privacy
- Levels such as public, internal, confidential, restricted; tag PII and special-category data (GDPR Art. 9).
- Controls: minimisation, column masking, tokenisation, row-level security, purpose-bound access, retention schedules, erasure propagated to derived tables and logs.
- Privacy law is jurisdiction-specific: give general guidance and refer binding questions to the DPO or counsel.

## Output
Quality scorecard per dataset (dimension, rule, threshold, result, trend); contract YAML; RACI; issue log with root cause and owner.

## Pitfalls
- Tests nobody owns or acts on; alert fatigue from warn-level noise.
- Measuring only nulls and missing accuracy.
- Governance as paperwork without decision rights.
