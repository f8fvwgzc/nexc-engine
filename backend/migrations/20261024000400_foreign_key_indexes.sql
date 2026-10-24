-- Every foreign key gets an index whose leading column is the key (docs/adr/0001-http3-edge-caching-and-indexes.md).
-- Without one, deleting or re-keying the row it points at reads the whole referencing table
-- (ON DELETE CASCADE / SET NULL / RESTRICT look the children up by the key), under the
-- delete's locks: deleting an agent read every issue, deleting a run every alert, LLM usage row
-- and run command, deleting a workspace every A2A task and alert delivery, and erasing an
-- account every audit and platform event. A nullable key is indexed where it is set
-- (`WHERE col IS NOT NULL`), as the earlier migrations do: the lookups ask for one value, so
-- the rows without one are never read and the index stays small.
--
-- Found by running the backend's test suite against a PostgreSQL with sequential scans
-- disabled and every plan logged (auto_explain): the predicates below were still read
-- sequentially. tests/schema.rs keeps it so: a foreign key without an index fails it.
--
-- On a large existing installation each statement takes a short write lock on its table. To
-- build them without one, run this file through psql with CONCURRENTLY first (psql sends one
-- statement at a time, outside a transaction); the migration then finds them and does nothing:
--   sed 's/CREATE INDEX IF NOT EXISTS/CREATE INDEX CONCURRENTLY IF NOT EXISTS/' \
--     backend/migrations/20261024000400_foreign_key_indexes.sql | grep -v '^SET LOCAL' | psql "$DATABASE_URL"
SET LOCAL lock_timeout = '5s';


-- Workspaces: deleting one takes everything of it along.
CREATE INDEX IF NOT EXISTS a2a_tasks_workspace_id_idx ON a2a_tasks (workspace_id);
CREATE INDEX IF NOT EXISTS alert_deliveries_workspace_id_idx ON alert_deliveries (workspace_id);
CREATE INDEX IF NOT EXISTS notifications_workspace_id_idx ON notifications (workspace_id);
CREATE INDEX IF NOT EXISTS onto_vocabulary_migrations_workspace_id_idx ON onto_vocabulary_migrations (workspace_id);

-- Graphs: deleting one takes its memories, realtime tickets and LLM usage along.
CREATE INDEX IF NOT EXISTS llm_usage_graph_id_idx ON llm_usage (graph_id) WHERE graph_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS memories_graph_id_idx ON memories (graph_id) WHERE graph_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS realtime_tickets_graph_id_idx ON realtime_tickets (graph_id);

-- Runs: deleting one (a graph deleted, a retention sweep) takes its alerts and commands along.
CREATE INDEX IF NOT EXISTS alerts_run_id_idx ON alerts (run_id) WHERE run_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS llm_usage_run_id_idx ON llm_usage (run_id) WHERE run_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS run_commands_run_id_idx ON run_commands (run_id);

-- Agents: deleting one detaches its reports and issues.
CREATE INDEX IF NOT EXISTS agents_reports_to_idx ON agents (reports_to) WHERE reports_to IS NOT NULL;
CREATE INDEX IF NOT EXISTS issues_agent_id_idx ON issues (agent_id) WHERE agent_id IS NOT NULL;

-- Teams: deleting one detaches its agents.
CREATE INDEX IF NOT EXISTS agents_team_id_idx ON agents (team_id) WHERE team_id IS NOT NULL;

-- Issues: deleting one takes its notifications along.
CREATE INDEX IF NOT EXISTS notifications_issue_id_idx ON notifications (issue_id) WHERE issue_id IS NOT NULL;

-- Workflow states: one in use cannot be deleted (RESTRICT), which the check reads by state.
CREATE INDEX IF NOT EXISTS issues_state_id_idx ON issues (state_id);

-- Approvals: deleting one takes its alert deliveries along.
CREATE INDEX IF NOT EXISTS alert_deliveries_approval_id_idx ON alert_deliveries (approval_id) WHERE approval_id IS NOT NULL;

-- People: what an account did or made, set to NULL or removed when the account is erased.
CREATE INDEX IF NOT EXISTS a2a_counterparts_created_by_idx ON a2a_counterparts (created_by) WHERE created_by IS NOT NULL;
CREATE INDEX IF NOT EXISTS agent_api_tokens_created_by_idx ON agent_api_tokens (created_by) WHERE created_by IS NOT NULL;
CREATE INDEX IF NOT EXISTS agents_owner_user_id_idx ON agents (owner_user_id) WHERE owner_user_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS alert_destinations_created_by_idx ON alert_destinations (created_by) WHERE created_by IS NOT NULL;
CREATE INDEX IF NOT EXISTS alerts_acknowledged_by_idx ON alerts (acknowledged_by) WHERE acknowledged_by IS NOT NULL;
CREATE INDEX IF NOT EXISTS approvals_decided_by_idx ON approvals (decided_by) WHERE decided_by IS NOT NULL;
CREATE INDEX IF NOT EXISTS artifact_file_versions_created_by_idx ON artifact_file_versions (created_by) WHERE created_by IS NOT NULL;
CREATE INDEX IF NOT EXISTS artifact_files_created_by_idx ON artifact_files (created_by) WHERE created_by IS NOT NULL;
CREATE INDEX IF NOT EXISTS artifact_files_updated_by_idx ON artifact_files (updated_by) WHERE updated_by IS NOT NULL;
CREATE INDEX IF NOT EXISTS artifact_folders_created_by_idx ON artifact_folders (created_by) WHERE created_by IS NOT NULL;
CREATE INDEX IF NOT EXISTS artifacts_released_by_idx ON artifacts (released_by) WHERE released_by IS NOT NULL;
CREATE INDEX IF NOT EXISTS assistant_conversations_user_id_idx ON assistant_conversations (user_id);
CREATE INDEX IF NOT EXISTS audit_log_actor_id_idx ON audit_log (actor_id) WHERE actor_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS day_summaries_created_by_idx ON day_summaries (created_by) WHERE created_by IS NOT NULL;
CREATE INDEX IF NOT EXISTS documents_uploaded_by_idx ON documents (uploaded_by) WHERE uploaded_by IS NOT NULL;
CREATE INDEX IF NOT EXISTS graphs_deleted_by_idx ON graphs (deleted_by) WHERE deleted_by IS NOT NULL;
CREATE INDEX IF NOT EXISTS issue_events_actor_id_idx ON issue_events (actor_id) WHERE actor_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS mcp_servers_created_by_idx ON mcp_servers (created_by) WHERE created_by IS NOT NULL;
CREATE INDEX IF NOT EXISTS notifications_actor_id_idx ON notifications (actor_id) WHERE actor_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS oidc_logins_link_user_id_idx ON oidc_logins (link_user_id) WHERE link_user_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS password_resets_created_by_idx ON password_resets (created_by) WHERE created_by IS NOT NULL;
CREATE INDEX IF NOT EXISTS plans_owner_id_idx ON plans (owner_id);
CREATE INDEX IF NOT EXISTS platform_events_actor_id_idx ON platform_events (actor_id) WHERE actor_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS policies_author_id_idx ON policies (author_id) WHERE author_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS projects_created_by_idx ON projects (created_by) WHERE created_by IS NOT NULL;
CREATE INDEX IF NOT EXISTS projects_lead_id_idx ON projects (lead_id) WHERE lead_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS realtime_tickets_user_id_idx ON realtime_tickets (user_id);
CREATE INDEX IF NOT EXISTS run_commands_requested_by_idx ON run_commands (requested_by) WHERE requested_by IS NOT NULL;
CREATE INDEX IF NOT EXISTS runs_owner_id_idx ON runs (owner_id);
CREATE INDEX IF NOT EXISTS workspace_guardrails_updated_by_idx ON workspace_guardrails (updated_by) WHERE updated_by IS NOT NULL;
CREATE INDEX IF NOT EXISTS workspace_invites_invited_by_idx ON workspace_invites (invited_by) WHERE invited_by IS NOT NULL;
CREATE INDEX IF NOT EXISTS workspace_knowledge_settings_updated_by_idx ON workspace_knowledge_settings (updated_by) WHERE updated_by IS NOT NULL;
CREATE INDEX IF NOT EXISTS workspace_llm_settings_updated_by_idx ON workspace_llm_settings (updated_by) WHERE updated_by IS NOT NULL;
CREATE INDEX IF NOT EXISTS workspace_transfers_started_by_idx ON workspace_transfers (started_by) WHERE started_by IS NOT NULL;
CREATE INDEX IF NOT EXISTS workspaces_created_by_idx ON workspaces (created_by) WHERE created_by IS NOT NULL;

-- Predicates the application filters on that no index served.
-- The export's run list asks, for every run, whether the outbox still holds it.
CREATE INDEX IF NOT EXISTS onto_export_queue_subject_idx ON onto_export_queue (subject_id, kind);
-- The sweep that clears the tenants of deleted workspaces in onto-intelli: the ones left to clear.
CREATE INDEX IF NOT EXISTS onto_retired_tenants_pending_idx
    ON onto_retired_tenants (retired_at, workspace_id) WHERE cleared_at IS NULL;

-- artifacts (run_id) is the leading column of the unique (run_id, node_id, path): the second
-- index only cost every write.
DROP INDEX IF EXISTS artifacts_run_idx;
