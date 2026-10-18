# Alerts

This page routes a workspace's alerts and approvals to a webhook or a Slack channel, and shows what a receiver gets.

## Destinations

A workspace admin adds destinations under **Settings, Alerts**. A destination has a name, a kind (`webhook` for a signed JSON POST, `slack` for an incoming webhook with Block Kit), the URL, the alert kinds it receives, the least severity it receives, and whether approvals requested and decided go too. The URL is kept encrypted and only its host is shown again. The signing secret of a webhook is generated once and shown once. In production a destination on a private or loopback host is refused unless `NEXC_ALERT_ALLOW_PRIVATE_HOSTS` is set.

| Behaviour              | Detail                                                                                                                                         |
| ---------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| Links in a delivery    | Every delivery links into the console under `NEXC_PUBLIC_URL`: the AgentSecOps page, the run and span, the agent.                              |
| Retries                | A delivery that fails is retried. Retries carry the same event id, so a receiver can drop what it already handled.                              |
| Switching off          | A destination that fails fifty deliveries in a row is switched off by the platform, with the reason shown. Enabling it again clears the count. |
| Test                   | `POST /workspaces/{wid}/alert-destinations/{id}/test` sends a `test` event at once and reports what the destination answered.                   |
| Delivery log           | `GET /workspaces/{wid}/alert-destinations/{id}/deliveries` lists the deliveries, test events included.                                         |

The endpoints are under `/workspaces/{wid}/alert-destinations` in the [endpoints](/reference/endpoints). The shapes follow.

## The types

```ts
<!--@include: ../../docs/CONTRACT.md#alerts-->
```

## Pulling instead of pushing

A SIEM that prefers to pull reads the same alerts, and the audit log, from the export endpoints: [SIEM export](/operate/siem).
