---
id: eng-testing-strategy
name: Testing strategy
category: engineering
description: Designs a test strategy that balances static, unit, integration, contract and end-to-end tests, applies TDD, property-based and contract testing, and sets meaningful coverage and test data rules.
keywords: testing strategy, test pyramid, testing trophy, unit test, integration test, end-to-end test, e2e, tdd, test coverage, mutation testing, property-based testing, contract testing, pact, test data, flaky tests
source: Ham Vocke - The Practical Test Pyramid (martinfowler.com) | https://martinfowler.com/articles/practical-test-pyramid.html
source: Kent C. Dodds - The Testing Trophy and Testing Classifications | https://kentcdodds.com/blog/the-testing-trophy-and-testing-classifications
source: Pact documentation - Introduction | https://docs.pact.io/
source: Hypothesis documentation | https://hypothesis.readthedocs.io/en/latest/
---
## When to use
Planning tests for a new service or feature, reviewing a suite, fixing a slow or flaky suite, or deciding what must be tested before a release.

## Shapes and levels
- Pyramid: many fast unit tests, fewer integration and contract tests, very few end-to-end tests. Anti-pattern: the ice-cream cone of mostly UI and manual tests.
- Trophy: static analysis as the base, then unit, a thick integration layer and a thin end-to-end top; suits UI-heavy apps where integration tests give the most confidence per cost.
- Rule: push each check to the lowest level that can catch the defect; add a higher-level test only for what lower levels cannot see (wiring, configuration, real browser or network). If a high-level test fails and no lower one does, add the lower one.
| Level | Scope | Speed | Dependencies |
|---|---|---|---|
| Static | types, lint, schema checks | seconds | none |
| Unit | one unit via its public interface | milliseconds each | fakes or stubs for slow or side-effecting collaborators |
| Integration | one boundary: DB, queue, files, HTTP client | seconds | real dependency in a container, or a fake server |
| Contract | consumer expectations against provider | seconds | pact files, provider verification |
| End-to-end | critical user journeys on a deployed system | minutes | real system |

## Method
1. List risks: money and data paths, complex rules, boundaries, past defects, security. Map each to a level.
2. TDD where the design is unclear: red (failing test for the next behaviour), green (simplest code that passes), refactor under green. Test behaviour, not internals, so refactors do not break tests.
3. Each test: Arrange-Act-Assert (or Given-When-Then), one behaviour, a name stating it ("rejects_expired_token"), deterministic (control clock, randomness, ordering), independent of other tests.
4. Case design: equivalence partitions and boundary values (min-1, min, max, max+1), decision tables for rule combinations, state transitions for workflows, error and timeout paths.
5. Property-based tests (Hypothesis, fast-check) for invariants: round trip decode(encode(x)) = x, idempotence f(f(x)) = f(x), ordering, totals preserved; failures shrink to a minimal counter-example.
6. Contract tests for service APIs with known consumers: consumer tests generate the contract, provider CI verifies it, and the deploy step checks compatibility (Pact can-i-deploy). Public APIs with unknown consumers: OpenAPI schema validation and breaking-change checks instead.
7. Pipeline: static and unit tests on every commit (whole stage under about 10 minutes), integration and contract next, end-to-end smoke after deploy. Quarantine flaky tests and fix them within days.

## Coverage and test quality
- Line or branch coverage shows untested code; it does not show that tests assert anything. Use it as a floor on changed code (80% branch on new code is a common team target, not a standard) and inspect uncovered critical paths.
- Mutation testing (PIT, Stryker, mutmut) checks that tests fail when code is altered; use it on core logic.
- Confirm a new test fails when the code is broken.

## Test data
Builders or factories with defaults and explicit overrides; minimal data per test; synthetic or masked data, never raw production personal data; fixed random seeds; transaction rollback or cleanup per test.

## Pitfalls
Mocking types you do not own; asserting on private internals; sleeps instead of waiting on a condition; end-to-end tests duplicating unit checks; coverage gamed with assertion-free tests.
