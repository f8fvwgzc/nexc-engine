# Agent Firewall API

This page describes the gRPC service that gives agents running on other platforms the same signed decisions and the same trace as nexc's own agents.

<!--@include: ../../docs/CONTRACT.md#firewall-wire-->

## Turning it on

| Step                                       | How                                                                                                                                                                                                       |
| ------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Start the service                          | `NEXC_GRPC_ADDR=0.0.0.0:50052`. It is off by default, is for services only and is never routed to browsers.                                                                                               |
| Sign the decisions                         | `NEXC_FIREWALL_SIGNING_KEY`, the base64 of 32 bytes. It is derived from `NEXC_MASTER_KEY` when unset.                                                                                                           |
| Give an external agent a token             | Create an agent whose runtime is `external`, then issue a token on its card (`POST /agents/{id}/tokens`): the secret is shown once and kept as a hash. Revoking it ends access at the next call.         |
| Open it in Kubernetes                      | In the Helm chart `backend.grpcAddr` turns it on, `backend.firewall.port` is its Service port and `networkPolicies.firewallFrom` names the namespaces admitted. The kustomize NetworkPolicy documents the same rule. See [Helm chart](/operate/helm). |

Every decision and every reported span lands on the same trace as a runtime's: [Trace](/secops/trace). The A2A adapter uses the same tokens for its counterparts: [A2A on the firewall](/secops/a2a).
