---
id: ml-llm-applications
name: LLM applications
category: ml
description: Builds and evaluates LLM features: RAG (chunking, hybrid retrieval, reranking, retrieval evaluation), prompt design, structured outputs, tool use, evals with LLM judges, and cost and latency control.
keywords: llm, rag, retrieval augmented generation, embeddings, vector search, chunking, reranking, prompt engineering, structured output, json schema, tool use, agents, evals, llm-as-judge, hallucination, token cost
source: Anthropic - Introducing Contextual Retrieval | https://www.anthropic.com/news/contextual-retrieval
source: Claude Docs - Create strong empirical evaluations | https://platform.claude.com/docs/en/test-and-evaluate/develop-tests
source: Zheng et al. - Judging LLM-as-a-Judge with MT-Bench and Chatbot Arena | https://arxiv.org/abs/2306.05685
source: OWASP - Top 10 for LLM Applications 2025 | https://genai.owasp.org/llm-top-10/
source: Ragas - Available metrics | https://docs.ragas.io/en/stable/concepts/metrics/available_metrics/
---
## When to use
Designing, debugging or evaluating an LLM feature: document Q&A, extraction, classification, agents with tools.

## Approach choice
- Prompt only when the model already knows enough and the context fits. A corpus under about 200k tokens can go whole into the prompt with prompt caching.
- RAG when knowledge is large, private or changing, or answers need citations.
- Fine-tuning for format, style or narrow skills at volume, not for adding fresh facts.

## RAG design
1. Ingest: parse, clean, dedupe; keep metadata (source, section, date, access list).
2. Chunk by structure (headings, paragraphs), a few hundred tokens with modest overlap; optionally prepend a 50-100 token chunk-specific context (generated from the whole document) before embedding and BM25 indexing.
3. Retrieve hybrid: BM25 for exact terms (codes, names) plus dense embeddings; fuse with reciprocal rank fusion, score = sum 1/(k + rank_i), k about 60. Enforce access control at retrieval time.
4. Rerank a wide candidate set (about 100-150) with a cross-encoder down to the top ~20.
5. Generate with citations; instruct the model to say when the context does not contain the answer.
- Anthropic reported top-20 retrieval failure (1 - recall@20) falling from 5.7% by 35% with contextual embeddings, 49% adding contextual BM25, 67% (to 1.9%) adding reranking.

## Retrieval and answer evaluation
- Labelled set of queries with relevant chunks: recall@k, MRR = mean(1/rank of first relevant), nDCG@k.
- Answers, with Ragas names: Faithfulness (share of answer claims supported by retrieved context), Response Relevancy, Context Precision, Context Recall. Ragas has no citation metric: check that each cited chunk supports its claim with your own code or judge.
- Debug in order: was the answer in the corpus, retrieved, ranked high, used?

## Prompts, structure and tools
- Prompt: role, task, success criteria, context in tagged sections, long documents before the question, output format, a few varied examples.
- Structured output: define a JSON Schema (enums, required fields); use the provider's schema or tool-call enforcement; validate with a parser; retry with the error on failure.
- Tools: few, narrow, typed, clearly described; idempotent where possible; least privilege; confirm side effects; cap steps and spend.
- Security (OWASP LLM Top 10, 2025): prompt injection (LLM01), sensitive information disclosure (LLM02), improper output handling (LLM05), excessive agency (LLM06), vector and embedding weaknesses (LLM08), unbounded consumption (LLM10). Treat retrieved and tool content as untrusted data.

## Evals
- Specific, measurable success criteria; a test set mirroring real traffic plus edge cases (empty, long, ambiguous, adversarial).
- Prefer code grading (exact match, schema validity, numeric tolerance); use LLM judges for subjective qualities with a rubric, defined scale, constrained output and a different model from the generator.
- Judge biases: position (swap order, count only consistent wins), verbosity, self-preference, weak maths or reasoning grading (give a reference answer). Measure judge agreement with human labels (e.g. Cohen's kappa) before relying on it. Strong judges reached over 80% agreement with humans in MT-Bench work, about human-human level.
- Run evals on every prompt, model or retrieval change.

## Cost and latency
Cost = input tokens x input price + output tokens x output price; cache stable prefixes; route easy cases to smaller models; batch offline work; stream to cut time to first token; cap output length. Track p50/p95 latency and cost per successful task.

## Pitfalls
Tuning prompts on the test set; chunking that splits tables; no "not found" path; trusting one judge score; ignoring injection via documents.
