---
id: prod-requirements-and-specs
name: Requirements and specifications
category: product
description: Elicits, writes and manages requirements: BABOK v3 techniques, INVEST user stories, Gherkin acceptance criteria, measurable non-functional requirements and a complete PRD; use when a feature must be specified for build.
keywords: requirements, specification, prd, product requirements document, user story, invest, acceptance criteria, gherkin, given when then, babok, business analysis, non-functional requirements, nfr, iso 25010, traceability, definition of ready
source: IIBA - BABOK Guide | https://www.iiba.org/career-resources/a-business-analysis-professionals-foundation-for-success/babok/
source: IIBA - CBAP certification | https://www.iiba.org/business-analysis-certifications/cbap/
source: Bill Wake - INVEST in good stories and SMART tasks | https://xp123.com/articles/invest-in-good-stories-and-smart-tasks/
source: Cucumber - Gherkin reference | https://cucumber.io/docs/gherkin/reference/
source: ISO/IEC 25010 product quality model | https://iso25000.com/index.php/en/iso-25000-standards/iso-25010
---
## When to use
Turning a validated problem into something a team can build, test and accept, or reviewing a spec for gaps.

## BABOK v3 frame
- Six knowledge areas: business analysis planning and monitoring; elicitation and collaboration; requirements life cycle management; strategy analysis; requirements analysis and design definition; solution evaluation. Core concepts: change, need, solution, stakeholder, value, context.
- Classify: business requirements (why), stakeholder requirements (who needs what), solution requirements (functional and non-functional), transition requirements (data migration, training, cutover).
- Common techniques: interviews, workshops, observation, document analysis, process models (BPMN), use cases, data models, decision tables (DMN), business rules analysis, prototypes, root cause analysis, backlog management.
- A good requirement is atomic, complete, consistent, unambiguous, feasible, testable, prioritised and traceable to an objective.

## User stories
- "As a <role>, I want <capability>, so that <benefit>." Check INVEST: Independent, Negotiable, Valuable, Estimable, Small, Testable.
- Split by workflow step, business-rule variant, happy versus unhappy path, data type, interface, operation (create, read, update, delete), or by deferring performance work; use a timeboxed spike for unknowns.
- Definition of Ready (team-agreed) before a Sprint; Definition of Done for the Increment.

## Acceptance criteria (Gherkin)
```
Rule: Orders over the credit limit need approval
  Scenario: Order above limit is held
    Given a customer with a credit limit of 5,000
    When they submit an order worth 6,000
    Then the order status is "Awaiting approval"
```
- One behaviour per scenario, about 3-5 steps, written in domain language not clicks.
- Scenario Outline with Examples for data variations; short Background for shared setup.
- Cover negative paths, boundaries (limit exactly 5,000) and permissions.

## Non-functional requirements
ISO/IEC 25010:2023 characteristics: functional suitability, performance efficiency (time behaviour, resource utilisation, capacity), compatibility, interaction capability (formerly usability: learnability, operability, user error protection, inclusivity and more), reliability, security, maintainability, flexibility (formerly portability), safety. Make each measurable: "p95 response under 300 ms at 500 requests per second", "99.9% monthly availability", "WCAG 2.2 level AA", "restore within 4 hours, lose at most 15 minutes of data".

## PRD structure
1. Problem, context and evidence
2. Goals and non-goals
3. Success metrics with targets and guardrails
4. Users, personas, jobs
5. Scope: prioritised stories with acceptance criteria
6. Flows and designs
7. Non-functional requirements
8. Data, analytics events, privacy and retention
9. Dependencies, risks, assumptions, open questions
10. Rollout: flags, beta, migration, support readiness
11. Approvals and change log
Keep a traceability matrix: objective, requirement ID, story, test case.

## Pitfalls
Solutions written as requirements; "fast", "easy", "secure" with no number; stories with no "so that"; acceptance criteria that restate the story; missing error and edge cases; specs frozen after build starts with no change log.
