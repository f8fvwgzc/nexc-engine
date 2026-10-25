---
id: plan-goal-decomposition
name: Goal decomposition
category: planning
description: Breaks a goal into outcomes, deliverables and assignable steps with a shared definition of done and verifiable acceptance criteria for each step; use when turning a request into a task graph.
keywords: plan, planning, decomposition, work breakdown structure, wbs, deliverables, tasks, steps, scope, definition of done, acceptance criteria, invest, smart, given when then, milestones
source: The 2020 Scrum Guide - Definition of Done and refinement | https://scrumguides.org/scrum-guide.html
source: Bill Wake - INVEST in good stories and SMART tasks (2003) | https://xp123.com/invest-in-good-stories-and-smart-tasks/
source: Cucumber - Gherkin reference (Given, When, Then) | https://cucumber.io/docs/gherkin/reference/
source: Work breakdown structure - 100 % rule overview | https://en.wikipedia.org/wiki/Work_breakdown_structure
---
## When to use
The Planner receives a goal and must produce steps that specialist agents can execute and a reviewer can check.

## Method
1. Restate the goal as an outcome: who needs it, what decision or change it enables, how success is measured, the deadline, and constraints (budget, data labels, tools available, no outside calls). Note open questions; ask only if a wrong guess would waste most of the work.
2. Outcome to deliverables. List the inspectable artefacts (noun phrases: "market sizing table", "decision memo", "API spec"). Build a deliverable-oriented work breakdown and apply the 100 % rule: children add up to exactly the parent's scope, they do not overlap, and nothing outside scope appears. Include the integration, review and packaging work explicitly; it is the most often forgotten.
3. Deliverables to steps. Each step is verb + object ("Estimate 2027 demand by region"), has one owner role, one primary output, and fits one agent's context and budget. Split a step that needs unrelated skills or yields two deliverables; merge steps that only shuttle text.
4. Definition of done for the whole plan, applied to every step: e.g. sources cited, figures with units, assumptions listed, file written at the named path, summary returned with finish. A step that misses it is not done; it goes back, it is not passed downstream.
5. Acceptance criteria per step: observable, binary, testable by someone other than the author. Use Given/When/Then for behaviour (3-5 lines), checklists for documents, thresholds for numbers.
6. Check each step against INVEST (independent, negotiable, valuable, estimable, small, testable) and its tasks against SMART.
7. Stop decomposing when a step is assignable, estimable and testable. Finer steps add handoff overhead.

## Acceptance criteria: weak vs strong
| Weak | Strong |
|---|---|
| Good market analysis | Table of TAM, SAM, SOM for 2026 in USD m with method and source per row |
| Review the code | All findings rated by severity; zero open critical items; tests pass |
| Write a summary | 5-line summary first; under 600 words; every figure tagged to a source |
| Check compliance | Each requirement of the named clause mapped to evidence or marked gap |

## Output
Plan table: ID | Step (verb + object) | Deliverable and path | Inputs | Acceptance criteria | Owner role | Depends on. Then the shared definition of done, assumptions and open questions.

## Checklist
- Every deliverable traces to the outcome; every step to a deliverable.
- No orphan work, no overlap between steps.
- Each step names its inputs and where its output lands.
- A review or verification step exists for anything decision-critical.

## Pitfalls
- Activity lists with no deliverable ("research", "analyse").
- Criteria like "high quality" that no one can test.
- Gold-plating beyond what the decision needs.
- Steps that need data, tools or access the agents do not have.
