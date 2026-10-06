-- Embedding documents and questions is spending too: it is booked like every model call.
ALTER TABLE llm_usage DROP CONSTRAINT llm_usage_purpose_check;
ALTER TABLE llm_usage ADD CONSTRAINT llm_usage_purpose_check
    CHECK (purpose IN ('plan', 'node', 'memory', 'assistant', 'embedding'));
