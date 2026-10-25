-- Emoji people give a comment or a run's report on an issue's timeline: one row per person and
-- emoji, so giving the same emoji twice is one reaction. Which emoji are allowed is the server's
-- list (`domain::issue::REACTION_EMOJI`); the check only bounds the text. The primary key leads
-- with the entry, which serves reading an entry's reactions and the cascade from issue_events;
-- the index on the person serves the cascade from users.
SET LOCAL lock_timeout = '5s';

CREATE TABLE IF NOT EXISTS issue_event_reactions (
    event_id    uuid NOT NULL REFERENCES issue_events (id) ON DELETE CASCADE,
    user_id     uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    emoji       text NOT NULL CHECK (octet_length(emoji) BETWEEN 1 AND 32),
    created_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (event_id, user_id, emoji)
);
CREATE INDEX IF NOT EXISTS issue_event_reactions_user_id_idx ON issue_event_reactions (user_id);
