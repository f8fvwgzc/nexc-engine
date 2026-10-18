# A2A on the firewall

This page explains how the Agent-to-Agent protocol sits on nexc's principals, gateway and trace.

The wire itself is on [A2A adapter](/reference/a2a).

<!--@include: ../../docs/AGENTSECOPS.md#a2a-->

## Settings

`NEXC_A2A_TIMEOUT_SECS` bounds one outbound task and `NEXC_A2A_ALLOW_PRIVATE_HOSTS` lets counterparts be registered on private hosts during development. Both are in [Environment variables](/reference/environment). The console's A2A tab and **Settings, A2A counterparts** are described on [The console](/secops/console).
