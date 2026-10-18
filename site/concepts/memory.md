# Memory

This page explains what nexc-engine remembers from a run and how memories come back as context for later nodes and plans.

## What a memory is

A memory is a short statement extracted from a node's output when the node finishes. It has a kind (`fact`, `experience`, `observation` or `preference`), a scope (`user`, `graph` or `node`), an importance, a count of how often it was recalled, and a data label that decides which agents may recall it. The shape is the `Memory` type in the [types](/reference/types).

## Consolidation

New statements are consolidated against what is already stored, in the way of Mem0 and Hindsight: a new fact is added, a changed one updates the memory it supersedes, and a near-duplicate is skipped.

## Retrieval

When a node or a plan is prepared, memories are retrieved by a hybrid score: the cosine similarity of embeddings, BM25 over the text, and recency weighted by importance. The ones that score best go into the prompt as context, wrapped as data rather than instructions. With the `pgvector` extension the embedding search uses an HNSW index. See [Infrastructure](/operate/infrastructure).

## Scopes and labels

An agent recalls memories only from the namespaces its capabilities allow (`data.memory_namespaces`) and only those whose label its clearance covers. See [Identity](/secops/identity). A workspace admin classifies a memory with `PATCH /memories/{id}`, and the change is audited as `classification_changed`.

## Where to look

**Settings, Memory** lists a workspace's memories, searches them, shows the topics found among them and lets an admin classify or remove one. The API is `GET /memories`, `GET /memories/{id}`, `GET /memories/topics` and `POST /memories/topics/rebuild` in the [endpoints](/reference/endpoints).
