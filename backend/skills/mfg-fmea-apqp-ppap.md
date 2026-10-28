---
id: mfg-fmea-apqp-ppap
name: FMEA, APQP and PPAP
category: manufacturing
description: Runs automotive-style quality planning: AIAG-VDA design and process FMEA with action priority, APQP phases and gates, control plans, and PPAP elements and submission levels.
keywords: fmea, pfmea, dfmea, aiag-vda, action priority, rpn, apqp, ppap, control plan, psw, part submission warrant, iatf 16949, special characteristics, fmea-msr, core tools
source: AIAG - Quality core tools and manual editions | https://www.aiag.org/expertise-areas/quality/quality-core-tools
source: AIAG & VDA FMEA Handbook | https://www.aiag.org/training-and-resources/manuals/details/FMEAAV-1
---
## When to use
New part or process, engineering or supplier change, a customer request for FMEA, control plan or PPAP, or launch readiness.

## Reference documents
AIAG & VDA FMEA Handbook, 1st edition (2019; 2nd printing 2022); AIAG APQP 3rd edition and AIAG Control Plan 1st edition (both March 2024); AIAG PPAP 4th edition. IATF 16949:2016 and customer-specific requirements (CSRs) override the manuals.

## FMEA in seven steps
1. Planning and preparation: scope, the 5Ts (intent, timing, team, tasks, tools), lessons learned.
2. Structure analysis: DFMEA system -> subsystem -> component; PFMEA process item -> step -> work element (4M: man, machine, material, environment).
3. Function analysis: functions and requirements at each level.
4. Failure analysis: chain of failure effect -> failure mode -> failure cause.
5. Risk analysis: current prevention and detection controls; rate S, O, D on 1-10 tables; derive Action Priority.
6. Optimization: actions with owner and date; re-rate after evidence.
7. Results documentation and communication.
- Action Priority: High = must act or justify, Medium = should act, Low = could act. AP weights S first, then O, then D, through the handbook table; RPN is not used to rank. Severity changes only with a design change; prevention lowers O; detection controls lower D.
- FMEA-MSR rates field diagnostics by severity, frequency and monitoring.
- Special characteristics flow from DFMEA severity to PFMEA to the control plan and drawing symbols.

## APQP phases
1. Plan and define: VOC, goals, preliminary BOM and process flow, special characteristics list.
2. Product design and development: DFMEA, DFM/DFA, design verification, prototype build and control plan.
3. Process design and development: process flow, floor plan, PFMEA, pre-launch control plan, MSA and capability plans, packaging.
4. Product and process validation: significant production run, MSA, preliminary capability, production validation tests, PPAP, production control plan.
5. Feedback, assessment and corrective action: launch, variation reduction, delivery and service.
Gate reviews use red/amber/green status (3rd edition adds gated management, sourcing, change management and program metrics); keep process flow, PFMEA and control plan linked line by line.

## Control plan
Phases: prototype, pre-launch, safe launch (added in CP 1st edition), production. Columns: process step, machine or tool, characteristic (product or process), special class, specification, measurement technique, sample size and frequency, control method, reaction plan. Every special characteristic and every high-AP cause must appear.

## PPAP (4th edition)
- 18 elements: design records; engineering change documents; customer engineering approval; DFMEA; process flow; PFMEA; control plan; MSA studies; dimensional results; material and performance tests; initial process studies; qualified laboratory documentation; appearance approval report; sample parts; master sample; checking aids; CSR records; part submission warrant (PSW).
- Levels: 1 PSW only (plus AAR if asked); 2 PSW, samples, limited data; 3 PSW, samples, complete data (default when unspecified); 4 PSW plus what the customer defines; 5 PSW, samples and complete data reviewed at the supplier's site.
- Run on production tooling at rate: 1-8 hours, at least 300 consecutive parts unless the customer says otherwise.
- Initial studies: Ppk > 1.67 acceptable; 1.33-1.67 may be acceptable with customer agreement; < 1.33 not acceptable without a plan.
- Triggers: new part, design change, new or moved tooling, new material or sub-supplier, process change, long-inactive tooling.

## Pitfalls
Copy-paste FMEAs; detection ratings that assume perfect inspection; control plan not matching the floor; PSW signed before data are complete.
