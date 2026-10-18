# SIEM export

This page pulls a workspace's alerts and its audit log into a SIEM without webhooks.

## The two endpoints

| Endpoint                                                   | Returns                                      |
| ---------------------------------------------------------- | -------------------------------------------- |
| `GET /workspaces/{wid}/alerts/export?since=&until=&format=` | `Alert` records, oldest first                |
| `GET /workspaces/{wid}/audit/export?since=&until=&format=`  | `AuditEntry` records, oldest first           |

Both stream `application/x-ndjson`, one object per line; `format=json` answers one array instead. `since` is exclusive and defaults to 24 hours before `until`; `until` is inclusive and defaults to now; a window the wrong way round is a 422. A pull answers at most 10,000 records: a pull that answered that many has more, and the next one starts from `since` set to the last `created_at`. Each account may pull thirty times a minute with a burst of ten; beyond that the answer is 429 with `Retry-After`. Workspace admins only: members get 403, outsiders 404.

## A pull

```sh
curl -sS -H "Authorization: Bearer $TOKEN" \
  "https://nexc.example.com/api/v1/workspaces/$WID/alerts/export?since=2026-10-17T00:00:00Z" \
  | while read -r line; do printf '%s\n' "$line" | jq -c .; done
```

The bearer token is an access token of a workspace admin; see the conventions of [the contract](/reference/). The shapes of `Alert` and `AuditEntry` are in the [types](/reference/types).

## Other ways out

| Need                                           | Page                                        |
| ---------------------------------------------- | ------------------------------------------- |
| Alerts and approvals pushed as they happen     | [Alerts](/operate/alerts)                   |
| The trace, span by span, in a tracing backend  | [OpenTelemetry export](/operate/otlp)       |
| Audit questions answered from the trace        | [Ask the audit](/secops/audit)              |
