-- Characters of upstream context that were left out of a call's prompt
-- because they were padding or not relevant to the task.
ALTER TABLE llm_usage ADD COLUMN context_chars_saved bigint NOT NULL DEFAULT 0
    CHECK (context_chars_saved >= 0);
