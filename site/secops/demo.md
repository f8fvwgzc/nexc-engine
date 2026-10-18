# Five-minute demo

This page is the script `make demo-secops` plays on the demo provider, step by step, with what to look at after each one.

<!--@include: ../../docs/AGENTSECOPS.md#demo-->

## Running it

`make demo-secops` starts the stack in demo mode, runs the script and prints the URLs to open. `DEMO_ARGS="--keep --skip-prove"` keeps what the script makes or skips the prove targets. The latency table on its own is `make prove-agentsecops`; it needs PostgreSQL (`make db-up`). See [Demo mode](/start/demo) for the other scripted demos.
