---
id: qa-test-design-techniques
name: Test design techniques
category: qa
description: Derives test cases with equivalence partitioning, boundary values, domain testing, decision tables, state transitions, pairwise, CRUD, scenario and exploratory charters, and states the coverage achieved.
keywords: test design, test case design, equivalence partitioning, boundary value analysis, decision table, state transition testing, pairwise testing, combinatorial testing, exploratory testing, test charter, istqb, ctal-ta, coverage, black-box testing
source: ISTQB - CTAL Test Analyst v4.0 syllabus | https://www.istqb.org/wp-content/uploads/sdm-uploads/ISTQB-CTAL-TA-Syllabus-v4.0-EN-4.pdf
source: ISTQB - CTFL v4.0.1 syllabus | https://www.istqb.org/wp-content/uploads/2024/11/ISTQB_CTFL_Syllabus_v4.0.1.pdf
source: ISTQB - CTAL-TA v4.0 release announcement | https://www.istqb.org/istqb-certified-tester-advanced-level-test-analyst-ctal-ta-v4-0-press-release
---
## When to use
Turning requirements, stories, rules, state models or interfaces into test cases; judging whether a suite is sufficient; reviewing test cases.

## Techniques and coverage
- Equivalence partitioning: split each input and output into partitions handled alike, valid and invalid; one value per partition; test invalid partitions one at a time. Coverage = partitions exercised / partitions identified.
- Boundary value analysis (ordered partitions): 2-value = each boundary and its nearest neighbour across it; 3-value = each boundary and both neighbours. Integers 18..65: 2-value {17, 18, 65, 66}; 3-value {17, 18, 19, 64, 65, 66}.
- Domain testing (CTAL-TA v4.0): partitions defined by Boolean expressions over several variables, tested with ON, OFF, IN and OUT points per border. Closed border (<=, >=, =): ON lies on it, OFF just outside; open border (<, >, !=): ON just inside, OFF on it. Simplified domain coverage: one ON and one OFF point per border using <, <=, >, >=; for = one ON and two OFF (both sides); for != one OFF and two ON. Reliable domain coverage adds an IN and an OUT point per inequality border.
- Decision tables: rules in a full table = product of condition value counts (n binary conditions = 2^n). Minimise by merging action-equivalent rules with "-" (don't care) and dropping infeasible ones; review for consistency, feasibility, completeness and correctness. Coverage = feasible rules exercised / feasible rules.
- State transition: states, events, guards, actions; draw a state table to expose undefined transitions. Criteria: all states; valid transitions (0-switch); all transitions (also attempts of invalid ones); N-switch (sequences of N+1 transitions; 1-switch common, 2+ only for high risk); round-trip (every loop back to its start state).
- Combinatorial: pairwise covers every value pair of any two parameters (tools: PICT, ACTS); base choice varies one parameter from a base case. A limited study (Kuhn et al., 2004) found about 97% of failures needed only one or two interacting factors. Four parameters with three values: 81 combinations, 9 pairwise tests. Encode constraints to remove infeasible pairs.
- CRUD: matrix of functions x entities; completeness check (each entity is created, read, updated and deleted somewhere) and lifecycle consistency tests including negatives (read before create, update after delete).
- Scenario / use case: main, alternative and exception flows; simple loop coverage (0, 1, typical, maximum iterations).
- Metamorphic: when no oracle exists, check relations across related inputs (a stricter search filter returns a subset; sorting a permutation gives the same result).
- Random and fuzz testing: inputs drawn from an operational profile; needs an automated oracle.
- Experience-based: error guessing, checklists, session-based exploratory testing. Charter: explore <target> with <resources> to discover <information>; 60-120 minute time box; notes, bugs, issues, debrief.

## Choosing techniques
Ranges and formats: EP, BVA, domain. Business rules: decision tables. Workflows and lifecycles: state transition, scenario, CRUD. Configurations: pairwise. Vague or new areas: exploratory. No oracle: metamorphic. Higher risk takes a stronger criterion (3-value BVA, 1-switch, reliable domain coverage).

## Output
Test conditions traced to requirements; the model per technique (partition, decision or state table); coverage items; test cases (ID, preconditions, inputs, steps, expected result, trace); achieved coverage in percent.

## Pitfalls
Forgetting invalid partitions; boundaries read from code instead of the specification; vague expected results; combinatorial explosion without constraints; exploratory sessions without charter or notes.
