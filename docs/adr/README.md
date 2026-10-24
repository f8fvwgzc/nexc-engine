# Architecture decision records

Decisions that shape more than one part of nexc-engine, with what was there before, what was
decided, what it costs and how it was checked. A record is never rewritten after the fact: a later
decision that changes one gets a record of its own that says which one it replaces.

| Record | Decision |
| --- | --- |
| [0001](0001-http3-edge-caching-and-indexes.md) | HTTP/3 between the browser and the edge (HTTP/2 between the services), caching in layers including the gRPC reads of onto-intelli, and an index for every foreign key |
