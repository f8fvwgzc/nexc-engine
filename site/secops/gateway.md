# Tools gateway

This page describes the gateway every external call of an agent goes through, and the order in which a call is decided.

<!--@include: ../../docs/ARCHITECTURE.md#gateway-->

## On the wire

<!--@include: ../../docs/CONTRACT.md#gateway-wire-->

## Settings

| Setting                                                       | Where                                                                                      |
| ------------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| MCP servers a workspace offers its agents                     | **Settings, MCP servers**; `/workspaces/{wid}/mcp-servers` in the [endpoints](/reference/endpoints) |
| A2A counterparts                                              | **Settings, A2A counterparts**; see [A2A on the firewall](/secops/a2a)                   |
| Calls per minute, timeout, largest response                   | `NEXC_GATEWAY_*` in [Environment variables](/reference/environment); `backend.gateway` in the [Helm chart](/operate/helm) |
| Which tools and hosts an agent may use                        | Its capabilities and policy, see [Identity](/secops/identity) and [Policy and defend](/secops/policy) |
