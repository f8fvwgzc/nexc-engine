-- Allow the `claude_code` provider (local Claude Code CLI, using the operator's own login).
ALTER TABLE llm_settings DROP CONSTRAINT llm_settings_provider_check;
ALTER TABLE llm_settings ADD CONSTRAINT llm_settings_provider_check
    CHECK (provider IN ('anthropic', 'openai_compatible', 'demo', 'claude_code'));
