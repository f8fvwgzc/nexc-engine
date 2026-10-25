---
id: qa-test-strategy
name: Test strategy
category: qa
description: Writes risk-based test strategies and test plans that set test levels and types, entry and exit criteria, estimates and shift-left practices, following ISTQB and ISO/IEC/IEEE 29119.
keywords: test strategy, test plan, risk-based testing, product risk, test levels, test types, entry criteria, exit criteria, definition of done, iso 29119, shift-left, testing quadrants, test estimation, test approach
source: ISTQB - CTFL v4.0.1 syllabus | https://www.istqb.org/wp-content/uploads/2024/11/ISTQB_CTFL_Syllabus_v4.0.1.pdf
source: ISTQB - CTAL Test Analyst v4.0 syllabus | https://www.istqb.org/wp-content/uploads/sdm-uploads/ISTQB-CTAL-TA-Syllabus-v4.0-EN-4.pdf
source: Wikipedia - ISO/IEC 29119 | https://en.wikipedia.org/wiki/ISO/IEC_29119
---
## When to use
Planning testing for a product, release or programme; writing a strategy or plan; deciding what to test and how deeply.

## Method
1. Context: business goals, lifecycle (agile, V-model, continuous delivery), architecture, regulation, users, constraints.
2. Product risk analysis with stakeholders: risks per ISO/IEC 25010:2023 characteristic, rate likelihood and impact (e.g. 1-5 each); risk level = likelihood x impact; e.g. 15-25 high, 8-14 medium, below 8 low (agree the bands).
3. Risk control: per risk set test levels, types, techniques, coverage criterion, independence, environment and timing. High risk: earlier, deeper, more independent. Low risk: lighter testing or documented acceptance.
4. Entry and exit criteria per level, metrics, reporting cadence.
5. Shift left: testers in refinement (testable acceptance criteria, examples, BDD), reviews and static analysis, TDD and unit tests, contract tests, CI on every commit. Shift right: canary releases, feature flags, synthetic monitoring, production telemetry.
6. Re-assess risks every iteration and adjust the plan.

## Levels and types
Levels: component, component integration, system, system integration, acceptance (user, operational, contractual, regulatory, alpha and beta). Types: functional, non-functional (performance, security, usability and accessibility, reliability, compatibility, flexibility), black-box and white-box, plus confirmation testing and regression testing.
Agile testing quadrants: Q1 technology-facing, supports the team (unit, component integration, automated); Q2 business-facing, supports the team (story and functional tests, examples, API tests); Q3 business-facing, critiques the product (exploratory, usability, UAT); Q4 technology-facing, critiques the product (performance, security, smoke, other non-functional).

## Entry and exit criteria
Entry: stories meet the Definition of Ready, environment and test data available, build deployed, smoke tests pass.
Exit (Definition of Done per level): planned tests run; agreed coverage reached (e.g. all high-risk requirements, branch coverage target); no open critical or high defects without a signed waiver; regression green; residual risk accepted by the product owner. Running out of time is a valid stop only when stakeholders accept the remaining risk.

## ISO/IEC/IEEE 29119
Part 1:2022 concepts; Part 2:2021 processes (organisational, test management, dynamic test); Part 3:2021 documentation (test policy, organisational test strategy, test plan, status and completion reports, test design and case specifications, incident reports); Part 4:2021 techniques; Part 5:2024 keyword-driven testing.
Test plan content: context and scope, assumptions and constraints, stakeholders, communication, risk register, test approach (levels, types, techniques, deliverables, entry and exit criteria, independence, metrics, data and environment needs), budget and schedule.

## Estimation
Three-point: E = (a + 4m + b) / 6 and SD = (b - a) / 6, with a optimistic, m most likely, b pessimistic (a=4, m=8, b=16 gives E=8.67, SD=2). Ratio-based from history (dev:test effort 3:2 means 900 dev days -> 600 test days). Wideband Delphi or Planning Poker for team estimates.

## Output
Objectives; scope in and out; risk table mapped to test approach; levels, types and owners; environments and data; automation approach; criteria; metrics; tools; schedule; open risks and assumptions.

## Pitfalls
Equal effort everywhere; template plans without risk analysis; exit criteria nobody enforces; late tester involvement; non-functional testing left to UAT.
