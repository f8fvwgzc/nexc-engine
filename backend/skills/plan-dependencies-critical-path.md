---
id: plan-dependencies-critical-path
name: Dependencies and critical path
category: planning
description: Orders plan steps as a directed acyclic graph, separates blocking from informational links, finds parallel branches, and computes the critical path and float with forward and backward passes.
keywords: dependencies, dag, task graph, critical path, cpm, float, slack, parallel, sequencing, predecessor, successor, topological sort, cycle, schedule, bottleneck
source: GAO-16-89G Schedule Assessment Guide (December 2015) | https://www.gao.gov/assets/d1689G.pdf
source: Anthropic - Building effective agents, parallelization patterns (2024) | https://www.anthropic.com/engineering/building-effective-agents
---
## When to use
After decomposition, to decide what runs first, what runs in parallel, and which steps set the finish time.

## Method
1. Draw links only where needed. Blocking link: B cannot start until A's output exists (finish-to-start). Informational link: B benefits from A but can start without it; pass A's output as optional context and do not make it an edge. Only blocking links constrain the schedule.
2. Prefer finish-to-start. Use start-to-start or finish-to-finish only for real overlap; avoid start-to-finish. Keep lags and date constraints to a minimum and justify each one, so that logic, not typed-in dates, drives the schedule (GAO best practice 2).
3. No dangling logic: every step except the start has a predecessor and every step except the end has a successor, so slips propagate.
4. Check for cycles with a topological sort (Kahn): repeatedly remove steps with no remaining predecessors; if any remain, they form a cycle. Break it by splitting the step (draft > review > revise as separate nodes), versioning outputs (v1, v2), or a bounded loop (at most 2 review rounds).
5. Forward pass: ES = max EF of predecessors (0 at start); EF = ES + d.
6. Backward pass: LF = min LS of successors (project end at the last EF); LS = LF - d.
7. Total float TF = LS - ES. Free float FF = min ES of successors - EF. The critical path is the continuous chain with the least float (zero when the end is not constrained); it is the longest path.
8. Parallel groups: steps with no path between them can run together, limited by fan-out and budget. At a join, all branches must finish; the more branches, the lower the chance the join starts on time (merge bias).
9. To finish sooner, act only on critical steps: split them, fast-track (overlap) or add capacity. Shortening a step with float changes nothing.

## Worked example (durations in hours)
| Step | d | Needs | ES | EF | LS | LF | TF |
|---|---:|---|---:|---:|---:|---:|---:|
| A research | 2 | - | 0 | 2 | 1 | 3 | 1 |
| B data pull | 3 | - | 0 | 3 | 0 | 3 | 0 |
| C analysis | 4 | A, B | 3 | 7 | 3 | 7 | 0 |
| D draft | 2 | C | 7 | 9 | 7 | 9 | 0 |
| E chart | 1 | C | 7 | 8 | 8 | 9 | 1 |
| F review | 1 | D, E | 9 | 10 | 9 | 10 | 0 |

Critical path B > C > D > F, 10 h. A and E each have 1 h of total float and 1 h of free float (A: ES of C 3 - EF 2; E: ES of F 9 - EF 8). Cutting A to 1 h saves nothing; cutting C to 3 h ends the plan at 9 h.

## Checks (GAO best practices 6 and 7)
- The critical path runs unbroken from start to finish.
- Unusually large float usually means a missing successor link.
- Each parallel branch has a merge point with a named consumer.

## Output
Dependency table as above, the critical path, parallel groups by wave (wave 1: A, B; wave 2: C; wave 3: D, E; wave 4: F), and the step most likely to delay the finish.

## Pitfalls
- Making every step depend on the previous one by habit, which kills parallelism.
- Treating "nice to have" context as blocking.
- Hidden cycles through shared files.
- Ignoring the review step when computing the end date.
