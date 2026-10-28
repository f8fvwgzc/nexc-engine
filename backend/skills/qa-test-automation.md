---
id: qa-test-automation
name: Test automation
category: qa
description: Designs maintainable test automation: pyramid balance, framework layers with page objects and fixtures, test data, contract testing, flaky-test control and CI quality gates.
keywords: test automation, test pyramid, automation framework, page object, fixtures, flaky tests, ci pipeline, contract testing, pact, playwright, selenium, api testing, regression automation, test data management, quality gate
source: Martin Fowler - The Practical Test Pyramid | https://martinfowler.com/articles/practical-test-pyramid.html
source: Pact documentation - Contract testing | https://docs.pact.io/
source: Google Testing Blog - Flaky tests at Google and how we mitigate them (May 2016) | https://testing.googleblog.com/2016/05/flaky-tests-at-google-and-how-we.html
---
## When to use
Starting or rescuing automation, designing a framework, fixing flaky suites, wiring tests into CI/CD, testing service integrations.

## Strategy
- Pyramid: many fast unit tests; fewer integration, API and contract tests; a handful of end-to-end UI journeys for critical paths. Push each check to the lowest layer that can catch the defect; when an E2E test finds a bug no lower test caught, add the lower test. Avoid the inverted "ice-cream cone".
- Automate checks that are repeated, stable, deterministic and valuable: regression, smoke, data-heavy rules, API contracts, performance baselines. Keep exploratory and usability testing human.
- Business case: automation pays when (manual cost per run x expected runs) exceeds build plus maintenance cost; track maintenance hours honestly.

## Framework design
- Layers: test (intent in Given/When/Then or Arrange-Act-Assert) -> business actions -> page objects or API clients -> drivers (Playwright, Selenium WebDriver, Appium, HTTP client). Keyword-driven layers (ISO/IEC/IEEE 29119-5:2024) when business staff read the suite.
- Page objects hold locators and interactions; assertions stay in tests. Prefer user-facing locators (role, label, test id) over brittle CSS or XPath chains.
- Fixtures handle setup and teardown (pytest fixtures, Playwright fixtures, JUnit 5 extensions). Each test creates its own data through APIs, is independent of order, and cleans up or runs in an ephemeral environment (Testcontainers, per-test schema, transaction rollback).
- Test data: builders and factories, synthetic data; masked production extracts only with approval.
- Test doubles (stubs, mocks, fakes, service virtualisation) for slow, costly or unavailable dependencies; a mocked external API needs a contract test so the mock cannot drift.

## Contract testing
Consumer-driven (Pact): consumer tests record the interactions the consumer really uses into a contract; the provider replays it in its own CI with provider states; a broker stores contracts and results; a can-i-deploy check gates releases. Use for HTTP and message integrations between teams; add schema checks (OpenAPI, AsyncAPI). Keep a few E2E smoke tests as well.

## Flaky tests
A flaky test passes and fails on unchanged code. Google reported almost 16% of its tests with some flakiness and 84% of observed pass-to-fail transitions involving a flaky test. Causes: fixed sleeps, async races, shared state and order dependence, colliding test data, clocks, time zones, randomness, unstable environments, real network calls. Fixes: wait on conditions (auto-waiting tools), hermetic data, frozen clocks and fixed seeds, stub the network. Retries are for detection only: record the flake rate. Quarantine with an owner and deadline; release after N consecutive clean runs.

## CI integration
On every pull request: lint, unit, component and contract tests in under about 10 minutes, plus a merge-gating smoke E2E set. Nightly or pre-release: full regression, cross-browser and device, performance and security scans. Shard and parallelise, fail fast, publish JUnit XML, traces, screenshots and videos, block merge on red. Test code gets the same review and refactoring as product code.

## Metrics
Pass rate, flake rate per test, suite duration, defects caught by automation vs escaped, coverage of risk-ranked requirements, mutation score for unit-test strength.

## Pitfalls
Automating unstable UI first; sleep-based waits; one giant E2E suite; assertions inside page objects; shared accounts; "just rerun it"; counting scripts as success.
