"""Agent-control tools: spawn_subagent and finish."""

from __future__ import annotations

from pydantic import Field

from .base import Tool, ToolArgs, ToolContext, ToolError


class SpawnArgs(ToolArgs):
    role: str = Field(description="Role of the sub-agent, e.g. 'researcher', 'editor', 'qa'.")
    task: str = Field(description="Self-contained task description with everything it needs.")


class FinishArgs(ToolArgs):
    answer: str = Field(
        description="Your final answer for this node in Markdown: what you produced and the key "
        "content downstream nodes need."
    )


async def spawn_subagent(ctx: ToolContext, args: SpawnArgs) -> str:
    if ctx.spawn is None:
        raise ToolError("spawning sub-agents is not available at this depth")
    if not args.role.strip() or not args.task.strip():
        raise ToolError("role and task must be non-empty")
    return await ctx.spawn(args.role.strip(), args.task.strip())


async def finish(ctx: ToolContext, args: FinishArgs) -> str:
    ctx.final_answer = args.answer
    return "final answer recorded"


SPAWN_SUBAGENT = Tool(
    name="spawn_subagent",
    description=(
        "Delegate a well-scoped sub-task to a new specialist sub-agent that shares your workspace "
        "and receives a share of your remaining token budget. Returns the sub-agent's final "
        "answer. Use sparingly, for genuinely separable work."
    ),
    args=SpawnArgs,
    handler=spawn_subagent,
)
FINISH = Tool(
    name="finish",
    description="Call exactly once when the deliverable is complete, with your final answer.",
    args=FinishArgs,
    handler=finish,
)
