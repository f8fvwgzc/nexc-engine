---
id: eng-code-quality-and-refactoring
name: Code quality and refactoring
category: engineering
description: Improves the readability and design of code through clear naming, small cohesive functions, pragmatic SOLID and behaviour-preserving refactorings made under tests in small reviewable steps.
keywords: code quality, refactoring, clean code, readability, naming, code smells, solid, technical debt, maintainability, complexity, duplication, legacy code, extract function
source: Martin Fowler - Refactoring catalog (2nd edition) | https://refactoring.com/catalog/
source: Google Engineering Practices - What to look for in a code review | https://google.github.io/eng-practices/review/reviewer/looking-for.html
source: Google Engineering Practices - Small CLs | https://google.github.io/eng-practices/review/developer/small-cls.html
---
## When to use
Before adding a feature to tangled code (first make the change easy, then make the change), when reviewing hard-to-read code, when paying down technical debt, or when asked to clean up a module.

## Principles
- Optimise for a reader without your context: code is complex when a reader cannot understand it quickly.
- Names reveal intent and units (elapsed_ms, retry_limit); functions are verbs, booleans are predicates (is_active, has_quota); no private abbreviations; name length grows with scope.
- Functions: one level of abstraction and one reason to change; usually under 20-30 lines; 0-3 parameters (group more into a parameter object); no flag arguments that switch behaviour; separate queries from modifiers.
- Complexity: shallow nesting via guard clauses; cyclomatic complexity above about 10 per function is a review trigger; tool scores are signals, not goals.
- Pragmatic SOLID: single responsibility; open/closed only where variation is real and recurring; Liskov (a subtype keeps the contract: no stronger preconditions, no weaker postconditions); small client-specific interfaces; depend on abstractions at boundaries (I/O, clock, randomness, network). Without a second real case, prefer YAGNI and the rule of three before removing duplication.
- Comments explain why, constraints and non-obvious decisions; the code says what.

## Smells and refactorings
Names from Fowler's catalog, 2nd edition (2018).
| Smell | Refactoring |
|---|---|
| Long function | Extract Function, Decompose Conditional, Split Phase |
| Duplicated code | Extract Function, Slide Statements, Pull Up Method |
| Long parameter list | Introduce Parameter Object, Preserve Whole Object |
| Deep nesting | Replace Nested Conditional with Guard Clauses |
| Repeated type switch | Replace Conditional with Polymorphism |
| Primitive obsession | Replace Primitive with Object |
| Feature envy | Move Function |
| Magic numbers | Replace Magic Literal |
| Reused temp or shared mutable data | Split Variable, Encapsulate Variable |
| Flag argument | Remove Flag Argument |
| Unused code | Remove Dead Code |

## Safe refactoring method
1. Pin behaviour: run the tests; if coverage is thin, first add characterisation tests that record current outputs, odd ones included.
2. Move in tiny behaviour-preserving steps; run tests after each; commit on green.
3. Keep refactoring separate from behaviour changes (its own commit or pull request). Aim for about 100 changed lines per review; around 1,000 is usually too large.
4. Prefer automated IDE refactorings (rename, extract, move) to hand edits; use codemods for wide renames; run the formatter in its own commit.
5. Legacy code: introduce a seam (inject the dependency), replace modules incrementally (strangler pattern), switch paths with feature flags, and compare old and new outputs in a parallel run before cutover.
6. Choose targets with data: hotspots where change frequency and complexity are both high.

## Output
For a cleanup: the smells found with locations, the refactorings applied in order, proof that behaviour is unchanged (tests run, characterisation tests added), and follow-ups left as tickets.

## Pitfalls
- Refactoring without tests, or mixing a rewrite with a bug fix so reviewers cannot separate them.
- Speculative layers and abstractions for needs that never arrive.
- Formatting churn mixed with logic changes.
- Big-bang rewrites; keep the system working after every step.
