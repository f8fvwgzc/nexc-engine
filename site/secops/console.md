# The console

This page describes the AgentSecOps console at `/app/secops`, the pages around it, and what each part shows.

## The AgentSecOps page

The page is the one screen a CISO wants to see. Its parts:

| Part               | What it shows                                                                                                                                                                                                                                    |
| ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Agents             | Every agent of the workspace with its status and its risk score. An agent opens its card.                                                                                                                                                        |
| Live feed          | Spans and alerts as they happen, over SSE.                                                                                                                                                                                                       |
| Approvals          | Decided in place; an approval also lands in the inbox of the approvers.                                                                                                                                                                          |
| Alerts             | Acknowledged in place; where they go beyond the console is on [Alerts](/operate/alerts).                                                                                                                                                        |
| Risk               | The risk score over the last fourteen days.                                                                                                                                                                                                      |
| Policies           | The resolved policy of any agent, the level that set each rule, and a diff between versions.                                                                                                                                                     |
| Ask the audit      | The audit's questions, answered by onto-intelli with the facts cited; see [Ask the audit](/secops/audit).                                                                                                                                       |
| Trace explorer     | Spans across every run, narrowed by agent, kind, decision, tool, host, run and time. The kind `inbound` marks A2A tasks received.                                                                                                                |

## The agent card

The card is the ID card of the agent: purpose, owner, team, scopes, model, budget left, last heartbeat, policy version, the last twenty actions, and the evaluation score of an agent made from a role template. Its **A2A** tab holds the switch that publishes the agent, the addresses of its card and its JSON-RPC endpoint, and the tasks it received with who called, the hop and the rule that refused one. For an agent whose runtime is `external`, the card issues and revokes the API tokens of the [Agent Firewall API](/secops/firewall).

## Around it

| Page                              | What it is for                                                                                       |
| --------------------------------- | ---------------------------------------------------------------------------------------------------- |
| **Settings, Policies**            | The policy editor for the workspace, its teams and its agents, with every version kept.             |
| **Settings, Alerts**              | The alerts of the workspace and the destinations they are routed to.                                 |
| **Settings, MCP servers**         | The servers the [tools gateway](/secops/gateway) offers to agents.                                   |
| **Settings, A2A counterparts**    | The remote agents this workspace may call and that may call it; see [A2A on the firewall](/secops/a2a). |
| **Settings, Audit log**           | Every change to membership, policies, classifications and agents.                                    |
| **Inbox**                         | Where approvals wait for a decision.                                                                 |
| A run's page                      | The trace of that run, with the chain verified.                                                      |

Platform administrators see the installation's side in the platform console: whether the firewall, the collector and onto-intelli answer, among the rest of `GET /admin/infrastructure`.
