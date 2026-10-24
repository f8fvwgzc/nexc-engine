# ADR 0003: Secrets never kept, more public documents, and narrower refreshes

- **Status:** accepted, 2026-10-24. **Extends ADR 0001** (Decision 2) and replaces nothing in it.
- **Not built or run yet:** implemented without building or running anything (no compiler,
  test, browser or `nginx -t` has seen it); "How to verify" lists what has to pass before relying
  on it.
- **Scope:** `backend/src/http/middleware/etag.rs`, the handlers of the two key sets
  (`handlers/evidence.rs`, `handlers/agent_token_keys.rs`) and of the API description
  (`app/router.rs`), `backend/src/realtime/fanout.rs`, `frontend/nginx.conf`,
  `frontend/nginx/site.conf`, `frontend/nginx/public-cache.conf`,
  `frontend/src/lib/query-client.ts`, `frontend/src/lib/freshness.ts`,
  `frontend/src/features/issues/issue-run.tsx`, `backend/tests/worst_case.rs`

<!-- #region body -->
## Context

[ADR 0001](0001-http3-edge-caching-and-indexes.md) layered the caches: every JSON read tagged and
`private, no-cache`, a handler's `public` policy kept beside the tag (the signing key set
`public, max-age=300`, the agent cards `public, max-age=30`), the edge keeping those documents,
the console keeping bodies per URL and its lists between visits (`query-persist.ts`), and the
onto-intelli reads kept per caller. What it left:

- **Secrets were tagged and kept.** Only a `public` policy was the handler's to choose, so every
  `200` JSON read went out `private, no-cache` with an `ETag`: sign-in and the account, the LLM
  key hints, the API tokens of agents, recorded span payloads, the SIEM pulls and the JSON
  downloads (evidence bundles). A browser kept each in its HTTP cache and the console's ETag cache.
- **Private reads did not vary by credential.** Nothing told a cache that ignores `private` that
  the answer depends on `Authorization`.
- **Two public documents were private.** The agent-token key set (`/api/v1/agent-tokens/jwks.json`)
  and the API description (`/api/openapi.json`) are the same for everyone but went out
  `private, no-cache`; on `NEXC_KEYS_ADDR` the key set had no tag at all.
- **The edge's static headers.** Hashed assets were `immutable` for a year `always`, so a `404`
  for an asset a rolling deploy had not reached yet was kept as immutable too; `index.html` was
  `no-cache, no-store, must-revalidate`; `favicon.svg` and `theme-init.js` had no `Cache-Control`;
  a kept public document carried the `X-Request-Id` of the request that filled it.
- **The console refreshed more than changed.** A query naming no kind of resource was fresh for
  30 s; every stale query refetched on a window focus, a workspace's setup and the catalogues
  included; a run reporting on an issue invalidated every `issues` query of every workspace.
- **One unknown key dropped a whole invalidation.** Replicas send up to 64 cache keys per
  `NOTIFY`. A replica that cannot read one key's kind, such as `Key::Ontology` (ADR 0001's read
  cache) on a replica of an earlier release during a rolling update, failed to parse the
  notification and dropped the policies, guardrails and vocabularies sent with it.

## Decision

1. **What must never be kept is `no-store` and untagged, whatever its handler says.**
   `conditional_reads` removes the `ETag` and sets `Cache-Control: no-store` on every `GET` of
   `/api/v1/auth/*` but `/auth/providers`, `/settings/llm`, `/workspaces/{wid}/llm`,
   `/workspaces/{wid}/knowledge/settings`, `/agents/{id}/tokens`, `/realtime/*`,
   `/spans/{sid}/payload`, `/workspaces/{wid}/audit/export` and `/alerts/export` (`never_kept`),
   and on every answer with `Content-Disposition: attachment`.
2. **The caller's own reads vary by credential:** `private, no-cache` and `Vary: Authorization`.
3. **The public documents still choose their policy in their handler** (ADR 0001's mechanism; the
   middleware keeps a `public` one). The two key sets and the API description send
   `PUBLIC_CACHE_CONTROL`, `public, max-age=300, stale-while-revalidate=60`; the agent cards keep
   `public, max-age=30`. The router of `NEXC_KEYS_ADDR` runs `conditional_reads` too, so its key
   set is tagged.
4. **The edge:** `/api/v1/agent-tokens/jwks.json` has a location of its own that includes
   `public-cache.conf`, as the other public documents do; `public-cache.conf` hides the
   upstream's `X-Request-Id`; hashed assets are `immutable` for a year on `200`, `206` and `304`
   and `no-store` otherwise; `index.html` is `no-cache`; the files without a hash at the root are
   `no-cache` (a server-level header that locations with headers of their own replace).
5. **The console refreshes what changed.** A query that names no kind of resource is a list
   (`staleTime: FRESH.list`, 60 s). A window focus refetches a stale query only when one of its
   observers is fresh for less than `FRESH.setup` or computes its own `staleTime`
   (`changesElsewhere`): the lists and live state, not setup or catalogues. A run reporting on an
   issue invalidates that issue and its timeline, its parent, the workspace's issue lists
   (`qk.issues.lists`) and projects, the team's cycles and the inbox (`issueKeys`). What the
   console keeps between visits does not change (ADR 0001, `query-persist.ts`); its ETag cache
   never holds a `no-store` answer, since such an answer has no tag.
6. **An invalidation survives a key kind a replica does not know.** The notifier sends
   `Key::Ontology` keys in a `NOTIFY` of their own (`batches`), so a replica of a release before
   them loses only those; and a replica skips a key it cannot read instead of the whole
   notification (`known_keys`), so the next new kind needs no such split.

## Consequences

- Every read on the never-kept list is a full `200` each time, never a `304`: the account and
  settings pages, the token list and the SIEM pulls cost their whole body on every load.
- A cache that honours the key sets' `public, max-age=300, stale-while-revalidate=60` may serve
  the previous set for six minutes after a rotation, and nginx sends no `Age`, so a cache behind
  it may add as much again. Publish the next agent-token key in `NEXC_AGENT_TOKEN_VERIFY_KEYS`
  well before the backend signs with it (CONTRACT §3).
- A path added later that holds a secret is kept privately until it joins `never_kept`;
  `backend/tests/worst_case.rs` lists the same paths itself, so the two lists are compared on
  every documented `GET`.
- Setup and catalogues changed in another tab or by another member show at their next mount, or
  once they are stale (5 and 30 minutes), no longer at the next focus.

## How to verify

Nothing in this record has been built or run. Before relying on it:

1. `make test-backend`: the unit tests of `etag.rs` (the never-kept paths, `Vary`, downloads, a
   handler's public policy), `tests/worst_case.rs`
   (`reads_are_tagged_and_answer_304_to_a_matching_etag`, which checks every documented `GET`
   against its own map of public documents and list of never-kept paths), and the fan-out tests
   in `realtime/fanout.rs`.
2. `make test-frontend`: `query-client.test.ts` (retries, focus), `query-keys.test.ts`
   (`qk.issues.lists`), `query-persist.test.ts` and `etag-cache.test.ts`.
3. By hand, against a running stack. `curl -I` sends `HEAD`, which the middleware leaves alone,
   so these send `GET`:

   ```sh
   curl -s -D - -o /dev/null http://localhost:8080/.well-known/jwks.json | grep -i 'cache-control\|etag'
   # public, max-age=300, stale-while-revalidate=60, and an ETag
   curl -s -D - -o /dev/null -H "Authorization: Bearer $TOKEN" http://localhost:8080/api/v1/auth/me \
     | grep -i 'cache-control\|etag\|vary'
   # no-store, and no ETag
   curl -s -D - -o /dev/null http://localhost:8080/api/v1/agent-tokens/jwks.json | grep -i x-cache-status
   # MISS, then HIT on the second call
   curl -s -D - -o /dev/null http://localhost:8080/assets/missing.js | grep -i cache-control   # no-store
   ```

4. During a rolling update from a release without `Key::Ontology`, a policy saved on a new
   replica is dropped on the old ones (`nexc_cache_lookups_total{cache="policies"}` shows a miss
   there).
<!-- #endregion body -->
