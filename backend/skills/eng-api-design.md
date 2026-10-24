---
id: eng-api-design
name: API design
category: engineering
description: Designs HTTP/REST and gRPC APIs with sound resource models, correct HTTP semantics and status codes, RFC 9457 errors, cursor pagination, idempotency keys, versioning and an OpenAPI contract.
keywords: api design, rest, http api, endpoint, status codes, openapi, swagger, problem details, rfc 9457, pagination, idempotency, versioning, deprecation, grpc, protobuf
source: RFC 9457 - Problem Details for HTTP APIs | https://www.rfc-editor.org/rfc/rfc9457.html
source: IETF draft - The Idempotency-Key HTTP Header Field | https://datatracker.ietf.org/doc/html/draft-ietf-httpapi-idempotency-key-header
source: Google AIP-158 - Pagination | https://google.aip.dev/158
source: OpenAPI Specification v3.2.0 | https://spec.openapis.org/oas/v3.2
source: RFC 9745 - The Deprecation HTTP Response Header Field | https://www.rfc-editor.org/rfc/rfc9745.html
---
## When to use
Designing or reviewing an API, adding endpoints, writing an OpenAPI document, or setting error, pagination and versioning conventions.

## Resource model
- Plural nouns: /orders, /orders/{order_id}; nest only for true containment (/orders/{id}/lines). Opaque stable IDs (UUID, ULID).
- Standard methods first (list, get, create, update, delete); a custom action only when none fits: POST /orders/{id}:cancel.
- One field-case convention; RFC 3339 UTC timestamps; money as integer minor units plus ISO 4217 code; enums as strings.

## HTTP semantics (RFC 9110)
| Method | Safe | Idempotent | Use |
|---|---|---|---|
| GET | yes | yes | read only |
| PUT | no | yes | full replace at a known URI |
| PATCH | no | no by default | partial update (RFC 7396 merge patch) |
| DELETE | no | yes | remove |
| POST | no | no | create in a collection, actions |
| QUERY | yes | yes | read with a request body, e.g. complex search (RFC 10008, June 2026) |
Status codes: 200; 201 with Location; 202 for async work plus a status resource; 204. 400 malformed; 401 unauthenticated; 403 forbidden; 404; 409 state conflict; 412 failed If-Match; 428 If-Match required but missing; 422 well-formed but invalid; 429 with Retry-After; 500, 502, 503 with Retry-After, 504. ETag with If-Match for optimistic concurrency.

## Errors: RFC 9457
July 2023, replaces RFC 7807. Media type application/problem+json; members type (URI, default about:blank), title (stable per type), status, detail (this occurrence, not for parsing), instance; extensions such as errors[] with field pointers. Never return stack traces or internals.

## Pagination (AIP-158)
Cursor by default: page_size in (documented default, values above the max lowered to the max, negative rejected), page_token in, next_page_token out; an empty token is the only end signal. Tokens are opaque and grant no access. Design it in from day one: adding it later breaks clients. Offset only for small, stable sets.

## Idempotency
Retryable POSTs take an Idempotency-Key header (IETF draft, revision -08 of October 2026, not yet an RFC). The client sends a unique key such as a UUID; the server stores key, request fingerprint and response for a published window. Same key and payload: replay the first response. Same key, different payload: 422. Original still running: 409. Key missing where required: 400.

## Versioning
- Non-breaking: new optional fields, new endpoints; clients ignore unknown fields. Breaking: removing or renaming fields, type or meaning changes, new required inputs, stricter validation.
- Major version in the path (/v1) or media type; support N-1 for a published window; announce with the Deprecation header (RFC 9745, March 2025; a structured date such as Deprecation: @1767225600), the Sunset header (RFC 8594, an HTTP date) and a Link to migration docs.

## Contract
OpenAPI 3.1 uses JSON Schema 2020-12; 3.2.0 (September 2025) adds the query operation, sequential and streaming media types and nested tags (Tag parent); 3.2.1 (September 2026) is the current patch. Write the spec first, lint it (Spectral), generate docs, mocks and SDKs, and diff for breaking changes in CI.

## gRPC
Never reuse or renumber protobuf fields (reserve removed ones); add fields only; use canonical codes (INVALID_ARGUMENT, NOT_FOUND, ALREADY_EXISTS, FAILED_PRECONDITION, UNAVAILABLE); set a deadline on every call.

## Output
A design note or review: resource list with methods and status codes, error catalogue (problem types), pagination and idempotency rules, versioning and deprecation policy, open questions, plus the OpenAPI or proto file when the step asks for one.

## Pitfalls
Verbs in paths (/getOrder); 200 with an error body; unbounded lists; unsafe retries of POST; breaking changes without a new version.
