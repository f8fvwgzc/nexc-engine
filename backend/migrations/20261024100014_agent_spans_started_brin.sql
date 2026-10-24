-- no-transaction
-- Spans in a time window. The AgentSecOps console counts the spans and the denials of the last
-- day and fortnight (`repo::secops::span_count`, `denials_by_rule`: `s.started_at >= $3`), which
-- read every span of the workspace's runs. Spans are never updated (a trigger refuses it,
-- 20261020000203) and arrive roughly in the order they start, so a BRIN index (a min and max per
-- 128 pages) finds the window's pages for a few kilobytes of index and almost no cost per insert.
-- A btree would cost every span write an entry for the same answer. Ranges are summarised by
-- autovacuum as they fill (`autosummarize`), and until then read as if they matched.
--
-- The order is not guaranteed, only likely: `started_at` is the reporter's clock (an external
-- agent, OTLP), and the retention purge deletes the spans of removed graphs, whose space new
-- spans then reuse. Either widens a range's summary, which never narrows again. The counts stay
-- right (a BRIN index is always rechecked); they only read more pages. `REINDEX INDEX
-- CONCURRENTLY agent_spans_started_brin` after a large purge tightens the summaries
-- (docs/DATABASE.md, "Indexes"). Built concurrently (docs/DATABASE.md, "Migrations"): spans arrive
-- all the time.
CREATE INDEX CONCURRENTLY IF NOT EXISTS agent_spans_started_brin ON agent_spans
    USING brin (started_at) WITH (autosummarize = on);
