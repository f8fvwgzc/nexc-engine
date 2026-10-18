# Operate

This section is for whoever runs nexc-engine: how to deploy it, keep its data safe, route its alerts and connect it to the tools an operations team already has.

| Page                                                   | What it is for                                                                        |
| ------------------------------------------------------ | ------------------------------------------------------------------------------------- |
| [Docker Compose](/operate/compose)                     | The full stack on one host, and how to put it behind TLS.                             |
| [Kubernetes](/operate/kubernetes)                      | The kustomize manifests, from minikube to a production checklist.                     |
| [Helm chart](/operate/helm)                            | The same deployment as a chart, with every value.                                     |
| [High availability](/operate/ha)                       | How a run survives a replica going away, the RTO and RPO of each failure, draining.   |
| [Backups](/operate/backups)                            | Back up, check and restore the database and the files on disk.                        |
| [Infrastructure](/operate/infrastructure)              | The one PostgreSQL database, secrets, queues and caches.                               |
| [Workspace transfers](/operate/transfers)              | Move one workspace to a database its owner controls.                                  |
| [Alerts](/operate/alerts)                              | Route alerts and approvals to a webhook or Slack.                                     |
| [SIEM export](/operate/siem)                           | Pull alerts and the audit log without webhooks.                                       |
| [OpenTelemetry export](/operate/otlp)                  | Send the trace to a collector.                                                        |
| [texc-symphony](/operate/symphony)                     | Hand coding nodes to texc-symphony.                                                   |
| [Security](/operate/security)                          | How the product is secured, how to report a vulnerability, what an operator hardens.  |
| [Troubleshooting](/operate/troubleshooting)            | Symptoms and fixes.                                                                   |

## Where the settings live

All configuration is environment variables in one `.env` at the repository root; Docker Compose and Kubernetes read the same names. The complete table is [Environment variables](/reference/environment), and `.env.example` documents each variable inline. `make doctor` (or `nexc doctor`) checks a configuration before the first start.
