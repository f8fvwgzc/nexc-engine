# Identity

This page describes the agent ID card: the principal every agent is, the capabilities it carries, and the signed per-run token that bounds what it and its sub-agents may do.

<!--@include: ../../docs/AGENTSECOPS.md#identity-->

## In the contract

The `Capabilities`, `AgentCard` and `ResolvedPolicy` shapes are in the [types](/reference/types); the per-run token and how the runtime checks every tool call against it are in the [runtime protocol](/reference/runtime). The API tokens an external agent presents to the [Agent Firewall API](/secops/firewall) are issued on the agent card (`POST /agents/{id}/tokens`).
