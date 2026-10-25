---
id: ml-responsible-ai
name: Responsible AI
category: ml
description: Assesses and documents AI systems against NIST AI RMF 1.0 and its GenAI profile, ISO/IEC 42001:2023 and EU AI Act risk classes, measures fairness, and writes model cards; general guidance, not legal advice.
keywords: responsible ai, ai governance, nist ai rmf, ai 600-1, iso 42001, eu ai act, high-risk ai, fairness, bias, demographic parity, equalized odds, model card, ai impact assessment, transparency, human oversight
source: NIST - AI Risk Management Framework | https://www.nist.gov/itl/ai-risk-management-framework
source: NIST AI 600-1 - Generative AI Profile | https://www.nist.gov/publications/artificial-intelligence-risk-management-framework-generative-artificial-intelligence
source: European Commission - AI Act regulatory framework | https://digital-strategy.ec.europa.eu/en/policies/regulatory-framework-ai
source: Modulos - ISO/IEC 42001 annexes A-D | https://docs.modulos.ai/frameworks/iso-42001/annexes-a-d.html
source: Fairlearn - Common fairness metrics | https://fairlearn.org/main/user_guide/assessment/common_fairness_metrics.html
---
## When to use
Classifying an AI use case, assessing impact or risk, measuring fairness, preparing governance evidence, writing a model card.

## NIST AI RMF 1.0 (Jan 2023; revision under way)
- Functions: **Govern** (policies, roles, accountability, third-party risk; cross-cutting), **Map** (context, intended use, stakeholders, impacts), **Measure** (test and track risks with metrics), **Manage** (prioritise, treat, monitor, communicate, decommission).
- Trustworthy traits: valid and reliable; safe; secure and resilient; accountable and transparent; explainable and interpretable; privacy-enhanced; fair with harmful bias managed.
- **NIST AI 600-1 GenAI Profile** (26 Jul 2024): 12 risks: CBRN information or capabilities; confabulation; dangerous, violent or hateful content; data privacy; environmental impacts; harmful bias or homogenisation; human-AI configuration; information integrity; information security; intellectual property; obscene, degrading or abusive content; value chain and component integration.

## ISO/IEC 42001:2023 (AI management system, certifiable)
- Clauses 4-10: context, leadership, planning (AI risk assessment, risk treatment, AI system impact assessment), support, operation, performance evaluation, improvement.
- Annex A: 38 controls in A.2-A.10 (policies, internal organisation, resources, impact assessment, life cycle, data, information for interested parties, use, third parties); justify inclusions and exclusions in a Statement of Applicability. Annex B gives guidance. Check for amendments locally.

## EU AI Act (Reg. (EU) 2024/1689, amended by AI Omnibus Reg. (EU) 2026/1744, in force 27 Jul 2026)
| Class | Examples | Duties / dates |
|---|---|---|
| Prohibited (Art. 5) | social scoring, manipulative techniques, workplace emotion recognition, untargeted face scraping | since 2 Feb 2025; new ban on non-consensual intimate or CSAM generation from 2 Dec 2026 |
| High-risk (Art. 6) | Annex III: employment, education, credit, essential services, law enforcement; Annex I: safety components of regulated products | Arts 9-15: risk management, data governance, technical documentation, logging, deployer information, human oversight, accuracy/robustness/cybersecurity; conformity assessment. Apply 2 Dec 2027 (Annex III), 2 Aug 2028 (Annex I) |
| Transparency (Art. 50) | chatbots, generated content, deepfakes | from 2 Aug 2026; 50(2) marking grace to 2 Dec 2026 for systems already on the market |
| Minimal | spam filters | none specific |
GPAI model duties apply since 2 Aug 2025. Name the jurisdiction and refer binding classification to counsel.

## Fairness measurement
- Selection rate SR_g = predicted positives in group g / group size. Demographic parity difference = max SR - min SR; ratio = min/max. The four-fifths (0.8) rule is a US employment screening flag, not proof of fairness.
- Equal opportunity: equal TPR. Equalized odds: equal TPR and FPR. Predictive parity: equal precision.
- With different base rates, calibration and equalized odds cannot both hold unless prediction is perfect. Choose by harm: allocation (jobs, credit) compare selection rates and TPR/FPR; quality of service compare error rates.
- Disaggregate, include intersections, report group sizes and intervals.

## Model card (Mitchell et al.)
Model details; intended use and out-of-scope uses; factors (groups, environments); metrics; evaluation data; training data; quantitative analyses by group; ethical considerations; caveats and recommendations.

## Output
Classification memo with dated sources; risk register (risk, impact, control, owner); fairness table by group; model card.

## Pitfalls
One aggregate metric; legal compliance taken as fairness; impact assessed after launch; no human-oversight design; undated regulatory claims.
