# Backups

This page backs up the database and the files on disk, checks that a backup restores, and restores one into an empty installation.

<!--@include: ../../README.md#backups-->

## What belongs together

A backup is the database plus the files under `NEXC_DATA_DIR` (uploaded documents and run artifacts) plus the secrets in `.env`. The master key matters most: stored AI keys and two-factor secrets are sealed with it, and a server started with a different key cannot read them. Moving an installation therefore means moving the database and the same secrets; see [Infrastructure](/operate/infrastructure).

In Kubernetes, back up the PostgreSQL volume (or use a managed database with its own backups) and the `backend-data` volume together with the `nexc-secrets` Secret; see [Kubernetes](/operate/kubernetes).

An organisation that wants its own copy of one workspace does not need a backup of the whole installation: see [Workspace transfers](/operate/transfers).
