# AgentSecOps

This section describes how nexc-engine creates, configures, traces, defends and audits agents, and where each control lives.

<!--@include: ../../README.md#agentsecops-->

## Pages

| Page                                                          | What it is for                                                                                      |
| ------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| [Assessment and target architecture](/secops/assessment)      | Where the product stood, the answers to the design questions, the five rules the design follows.    |
| [Identity](/secops/identity)                                  | The agent ID card: principals, capabilities, per-run tokens.                                        |
| [Trace](/secops/trace)                                        | Every action as a span on a hash chain, and the views built on it.                                  |
| [Policy and defend](/secops/policy)                           | The policy engine, DLP, the risk score, quarantine, versioned policies and approvals.               |
| [Ask the audit](/secops/audit)                                | onto-intelli as the audit's query engine.                                                           |
| [Tools gateway](/secops/gateway)                              | How an agent reaches MCP servers, the web and counterparts, and in which order a call is decided.   |
| [Agent Firewall API](/secops/firewall)                        | Signed decisions and a trace for agents that run elsewhere.                                         |
| [A2A on the firewall](/secops/a2a)                            | Agent cards, inbound and outbound tasks, delegation bounded across platforms.                       |
| [The console](/secops/console)                                | The one screen: agents, live feed, approvals, alerts, risk, policies, the audit, the trace explorer. |
| [Five-minute demo](/secops/demo)                              | The script `make demo-secops` plays.                                                                |
| [Data model, protocol and plan](/secops/plan)                 | The tables, the protocol changes, the performance targets and the phases.                           |

## Screenshots

<img class="light-only" src="/media/secops-light.png" alt="AgentSecOps console: agents, live feed of spans, approvals, alerts, risk, policies and the audit, light theme">
<img class="dark-only" src="/media/secops-dark.png" alt="AgentSecOps console in the dark theme">

![Trace explorer: spans across every run, narrowed by agent, kind, decision, tool, host, run and time](/media/secops-explorer.png)
