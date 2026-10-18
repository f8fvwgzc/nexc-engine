# Agents

This page describes how an agent node is executed in the Python runtime, how sub-agents are bounded, and how an agent made from a role template is useful on its first run.

## The runtime protocol

<!--@include: ../../docs/ARCHITECTURE.md#runtime-->

The wire itself, the per-run token, the spans and the approvals are in the [runtime protocol](/reference/runtime). What the agent may do is decided by its identity and policy, see [AgentSecOps](/secops/).

## Agents from role templates

<!--@include: ../../docs/AGENTSECOPS.md#templates-->
