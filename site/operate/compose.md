# Docker Compose

This page runs the full stack on one host, puts it behind TLS and says what is published.

<!--@include: ../../README.md#compose-->

## Putting it on the internet

<!--@include: ../../README.md#internet-->

## The other overlays

| File                        | What it adds                                                                                                              |
| --------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `docker-compose.demo.yml`   | Pins the offline demo provider (`make demo`). See [Demo mode](/start/demo).                                               |
| `docker-compose.onto.yml`   | Profile `onto`: onto-intelli as the audit's query engine (`make demo-onto`). See [Ask the audit](/secops/audit).          |
| Profile `symphony`          | texc-symphony as the executor of coding nodes. See [texc-symphony](/operate/symphony).                                    |

`make docker-down` stops every profile and keeps the volumes. Backups are on [Backups](/operate/backups).
