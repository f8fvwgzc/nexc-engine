---
id: plan-specialist-routing
name: Routing work to specialists
category: planning
description: Assigns each plan step to the agent whose role, purpose, skills, tools and data clearance fit it, writes a handoff contract for the step, adds reviewers where the stakes call for one, and escalates blockers.
keywords: routing, assign, delegate, delegation, handoff, specialist, agent selection, subagent, reviewer, escalation, raci, owner, orchestration, planner, task brief
source: Anthropic - Building effective agents (routing, orchestrator-workers, evaluator-optimizer) | https://www.anthropic.com/engineering/building-effective-agents
source: Anthropic - How we built our multi-agent research system (2025) | https://www.anthropic.com/engineering/multi-agent-research-system
---
## When to use
Once steps exist, to choose who does each one and to brief them so the work comes back usable the first time.

## Matching rules
Score each candidate agent against the step, in this order:
1. Hard gates: it has the tools the step needs (run_python for computation, make_docx for a formal document, spawn_subagent to fan out), is cleared for the data labels involved, and fits the remaining budget. Fail any gate and it is out.
2. Fit: title and department match the domain; its purpose covers the step's outcome; its skill ids cover the step's keywords (count the overlap).
3. Tie-break: prefer the narrower specialist; then the agent already holding the relevant context from an upstream step.
4. No fit: give it to the closest generalist, state the gap in the plan, and add a reviewer from the nearest domain.
Rules: one accountable owner per step; the reviewer is never the author; agents give general professional information only, so route anything needing a licensed sign-off to a human approval step.

## Handoff contract (put all of this in the step brief)
- Objective: one sentence, plus the decision it serves.
- Inputs: file paths and upstream step ids, with what each contains and how far to trust it.
- Output: path, format, required sections, length limit, citation style.
- Acceptance criteria: the checks a reviewer will apply.
- Constraints: data labels, allowed tools, no outside calls, token budget.
- Boundaries: what is out of scope; out-of-scope items go back to the Planner, not done ad hoc.
- Effort: scale it to the question. Anthropic's research system uses one agent with about 3-10 tool calls for a simple fact, 2-4 subagents with about 10-15 calls each for a direct comparison, and wide fan-out only for complex research with clearly split duties. In nexc, stay inside the agent's delegation limits (max_depth, max_fan_out).
Vague one-line briefs produce duplicated effort, gaps and misread scope; spend the words here.

## When to add a reviewer
Add a separate review step when any is true: the deliverable leaves the team; the field is regulated; numbers drive a money, safety or legal decision; the action is hard to reverse; several sources were synthesised; code will run; the author reports low confidence. Give the reviewer the acceptance criteria and a rubric (factual accuracy, citation accuracy, completeness, source quality, tool efficiency), each scored with a pass/fail verdict. Cap author-review loops at 2 rounds, then escalate.

## Escalation
An agent that cannot finish returns "blocked" with the reason and what it needs. The Planner then, in order: supplies the missing input; re-routes to a better-fitting agent; splits or narrows the step; relaxes a non-critical criterion and records it; escalates to a human. Escalate straight to a human for approvals, licensed sign-off, policy or permission denials, conflicting requirements, or budget overrun.

## Output
Routing table: Step | Agent (role) | Why this agent | Reviewer | Brief summary | Escalation contact.

## Pitfalls
- Routing by agent name instead of skills, tools and clearance.
- Overlapping briefs, so two agents research the same thing.
- Missing output format, forcing rework at the merge.
- Spawning many subagents for a simple question; chatty status traffic between agents.
