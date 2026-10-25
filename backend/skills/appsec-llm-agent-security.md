---
id: appsec-llm-agent-security
name: LLM and agent security
category: appsec
description: Threat-models and hardens LLM features, RAG pipelines and tool-using agents against the OWASP Top 10 for LLM Applications 2025, the agentic Top 10 and MITRE ATLAS techniques.
keywords: llm security, prompt injection, indirect prompt injection, excessive agency, system prompt leakage, owasp llm top 10, agentic security, mitre atlas, rag security, tool permissions, ai red teaming, output handling, least privilege
source: OWASP GenAI Security Project - Top 10 for LLM Applications 2025 | https://genai.owasp.org/llm-top-10/
source: OWASP GenAI Security Project - Top 10 for Agentic Applications for 2026 | https://genai.owasp.org/resource/owasp-top-10-for-agentic-applications-for-2026/
source: Promptfoo - OWASP Top 10 for Agentic Applications | https://www.promptfoo.dev/docs/red-team/owasp-agentic-ai/
source: MITRE ATLAS - atlas-data repository | https://github.com/mitre-atlas/atlas-data
---
## When to use
Designing, reviewing or red-teaming LLM features, RAG, tool-using and multi-agent systems.

## Key references (October 2026)
- OWASP Top 10 for LLM Applications 2025: LLM01 Prompt Injection; LLM02 Sensitive Information Disclosure; LLM03 Supply Chain; LLM04 Data and Model Poisoning; LLM05 Improper Output Handling; LLM06 Excessive Agency; LLM07 System Prompt Leakage; LLM08 Vector and Embedding Weaknesses; LLM09 Misinformation; LLM10 Unbounded Consumption.
- OWASP Top 10 for Agentic Applications for 2026 (Dec 2025): ASI01 Agent Goal Hijack; ASI02 Tool Misuse and Exploitation; ASI03 Identity and Privilege Abuse; ASI04 Agentic Supply Chain Vulnerabilities; ASI05 Unexpected Code Execution; ASI06 Memory and Context Poisoning; ASI07 Insecure Inter-Agent Communication; ASI08 Cascading Failures; ASI09 Human-Agent Trust Exploitation; ASI10 Rogue Agents.
- MITRE ATLAS (data release v2026.09, 16 tactics incl. AI Model Access and AI Attack Adaptation). Tag threats with IDs such as AML.T0051 LLM Prompt Injection, AML.T0054 LLM Jailbreak, AML.T0056 Extract LLM System Prompt, AML.T0057 LLM Data Leakage, AML.T0070 RAG Poisoning, AML.T0053 AI Agent Tool Invocation, AML.T0080 AI Agent Context Poisoning, AML.T0086 Exfiltration via AI Agent Tool Invocation.

## Design controls
- Trust boundary: everything not from the operator (user text, retrieved documents, web pages, emails, tool results, other agents' messages) is untrusted data. Indirect injection arrives through these channels; no prompt wording is a security control.
- Least privilege per agent: minimal tools, short-lived scoped credentials, allow-listed hosts, paths and tables, read-only by default, no egress unless required. The agent never holds more rights than the user it acts for.
- Human approval or a policy engine for irreversible or high-impact actions (payments, deletion, sending messages, permission changes, deploys); show the exact action and parameters.
- Output handling: model output going to shells, SQL, eval, HTML, file paths or URLs gets the same validation as user input; use schema-validated structured output; encode on render; block markdown images and links to untrusted domains (a common exfiltration path).
- Secrets and data: keep secrets out of prompts and assume the system prompt will leak; enforce document-level authorisation at retrieval time with the end user's permissions; isolate memory per user and tenant; DLP on outputs and logs.
- Supply chain and poisoning: pin and verify models, datasets, plugins and MCP servers (hash, signature, provenance, ML-BOM); review tool descriptions as code; monitor ingestion sources.
- Consumption: limits on tokens, steps, wall time, cost, recursion and delegation depth and fan-out; per-user rate limits; loop breakers and a kill switch.
- Multi-agent: authenticate and sign inter-agent messages; one compromised agent must not raise another's privileges.
- Telemetry: log prompts, retrieved ids, tool calls and denials with trace ids; alert on denials, odd tool sequences, egress attempts.

## Testing
Red-team per risk: direct and indirect injection (hidden text in documents and tool output), system prompt extraction, exfiltration through tools or rendered links, privilege escalation through tools, persistent memory poisoning, runaway loops. Measure attack success rate before and after each control; keep cases as regression tests.

## Output
Threat model (assets, trust boundaries, flows including retrieval and tools); risk table mapped to LLMxx, ASIxx and ATLAS IDs; control matrix; residual risk; test evidence.

## Pitfalls
Keyword filters or a second LLM as the only defence; agent credentials broader than the user's; trusting tool output; secrets in logs; no human gate on destructive tools.
