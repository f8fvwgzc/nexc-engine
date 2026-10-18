# High availability

This page says how a run survives the loss of the replica executing it, what each failure costs, and how to deploy and watch for it.

<!--@include: ../../docs/HA.md#model-->

<!--@include: ../../docs/HA.md#failure-modes-->

<!--@include: ../../docs/HA.md#targets-->

<!--@include: ../../docs/HA.md#deploy-->

The exact wire behaviour (the lease columns, the spans a takeover writes, the `Idempotency-Key` rules) is in [the contract](/reference/endpoints). The settings are on [Environment variables](/reference/environment), and the chart's values on [Helm chart](/operate/helm).
