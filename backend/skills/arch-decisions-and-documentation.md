---
id: arch-decisions-and-documentation
name: Architecture decisions and documentation
category: architecture
description: Records architecture decisions as ADRs, documents systems with C4 and arc42, evaluates trade-offs with ATAM and turns architectural constraints into automated fitness functions.
keywords: adr, architecture decision record, c4 model, arc42, atam, trade-off analysis, fitness function, architecture documentation, iso 42010, madr, utility tree, quality scenario
source: iSAQB - CPSA Foundation Level curriculum 2025.1 | https://public.isaqb.org/curriculum-foundation/curriculum-foundation-en.html
source: C4 model - Simon Brown | https://c4model.com/
source: arc42 - Template overview | https://arc42.org/overview
source: ADR GitHub organization - Architectural Decision Records | https://adr.github.io/
---
## When to use
Recording a decision, writing or reviewing architecture documentation, evaluating a design against quality goals, or making architecture rules enforceable.

## ADRs
- Write one when a decision is architecturally significant: hard to reverse, shapes a quality attribute, crosses team boundaries, costs real money or sets a precedent.
- One decision per record, numbered, in the repo (docs/adr/0007-use-postgres.md), reviewed by pull request. Once accepted, never rewrite: a new ADR supersedes it and both link to each other.
- Nygard form: Title; Status (proposed, accepted, rejected, deprecated, superseded by NNNN); Context (forces, constraints); Decision ("We will ..."); Consequences (good, bad, follow-ups). MADR 4.0 adds decision drivers, considered options with pros and cons, and a Confirmation section (how compliance is checked).
- One-line Y-statement: in the context of X, facing Y, we chose Z over A and B to achieve Q, accepting downside D.

## C4 model
- System Context: the system, its users and external systems; for every audience.
- Container: deployable or runnable units and data stores with technology; for technical staff.
- Component: parts inside one container. Code: only when it adds value, ideally generated.
- Supplementary: system landscape, dynamic (runtime sequence), deployment.
- Rules: each element shows name, type, technology and a one-line responsibility; each arrow is one-directional with a specific label (not "uses") and, between containers, the protocol; every diagram has a title stating type and scope plus a legend explaining shapes, colours and acronyms; never mix levels.

## arc42 sections
1 Introduction and goals (top 3-5 quality goals, stakeholders); 2 Constraints; 3 Context and scope; 4 Solution strategy; 5 Building block view; 6 Runtime view; 7 Deployment view; 8 Crosscutting concepts; 9 Architecture decisions; 10 Quality requirements (quality tree and scenarios); 11 Risks and technical debt; 12 Glossary. Fill only what serves the readers. ISO/IEC/IEEE 42010:2022 frames this as stakeholders, concerns, viewpoints and views.

## ATAM
Nine steps: present the method; business drivers; the architecture; identify architectural approaches; build the utility tree (quality attribute, refinement, scenario, each rated importance and difficulty H/M/L); analyse approaches; stakeholders brainstorm and vote on scenarios; analyse again; present results.
Outputs: risks (problematic decisions), non-risks (sound decisions), sensitivity points (a property of components or relationships that is critical to one quality response), trade-off points (one property that helps some qualities and hurts others), and risk themes tied to the business drivers they threaten. A half-day mini-ATAM on the top five scenarios suits small teams.

## Fitness functions
Executable checks that a characteristic still holds, run in CI (triggered) or production (continual): dependency and layering rules (ArchUnit, import linters, no cycles), API compatibility checks, p99 latency budget in a load test, bundle size limit, security scan gate, cost per transaction alert. Any ADR that sets a constraint names the fitness function that guards it.

## Checklist
Decision log current; quality goals are measurable scenarios; diagrams have legend and scope; risks and debt have owners; documentation lives with the code and changes in the same pull request.

## Pitfalls
ADRs written after the fact with no options; boxes without technology; diagrams that mix levels; wiki rules no tool enforces; ATAM without business drivers; documentation nobody owns.
