---
id: emb-functional-safety
name: Functional safety
category: embedded
description: Plans and checks functional safety work for electronic control systems: hazard analysis, SIL, ASIL and PL targets, FMEDA metrics, safety requirements and verification under IEC 61508, ISO 26262, ISO 13849-1 and IEC 62061.
keywords: functional safety, iec 61508, sil, iso 26262, asil, hara, iso 13849, performance level, iec 62061, fmeda, safe failure fraction, diagnostic coverage, pfh, pfd, safety case, safety lifecycle
source: IEC Webstore - IEC 61508-1:2010 general requirements | https://webstore.iec.ch/en/publication/5515
source: IEC Webstore - IEC 62061:2021+AMD1:2024 | https://webstore.iec.ch/en/publication/59927
source: IFA (DGUV) - SISTEMA for ISO 13849-1 | https://www.dguv.de/ifa/praxishilfen/practical-solutions-machine-safety/software-sistema/index.jsp
source: Wikipedia - Safety integrity level | https://en.wikipedia.org/wiki/Safety_integrity_level
source: Wikipedia - Automotive Safety Integrity Level | https://en.wikipedia.org/wiki/Automotive_Safety_Integrity_Level
---
## When to use
Setting integrity targets, writing safety requirements, checking an architecture or FMEDA, or planning verification for a safety function.

## Key standards
- IEC 61508:2010 Ed. 2.0, parts 1-7, the basic safety publication (stability date 2027; Ed. 3 in preparation, verify locally).
- ISO 26262:2018, 2nd edition, parts 1-12 (road vehicles; part 11 semiconductors).
- ISO 13849-1:2023 (machinery SRP/CS, PL a-e; IFA SISTEMA 3.x implements this edition).
- IEC 62061:2021+AMD1:2024 (machinery, SIL 1-3). Sector peers: IEC 61511 process, IEC 62304 medical software.

## Lifecycle method
1. Define the item or EUC, boundaries, modes and interfaces.
2. Hazard analysis and risk assessment:
   - ISO 26262 HARA: S0-S3, E0-E4, C0-C3 per hazardous event. With all classes >= 1 (any class 0 gives QM): S+E+C = 10 gives ASIL D, 9 C, 8 B, 7 A, lower QM. Write a safety goal, safe state and FTTI.
   - ISO 13849-1 risk graph: severity S1/S2, exposure F1/F2, avoidance P1/P2 -> PLr a-e.
   - IEC 61508/61511: risk graph, LOPA or calibrated matrix -> risk reduction -> SIL; low demand (<= 1 demand per year) vs high/continuous.
3. Safety requirements: function, safe state, response time within process safety time or FTTI, integrity level, demand mode. ASIL decomposition (D -> C(D)+A(D) or B(D)+B(D)) needs evidence of independence.
4. Architecture: HFT (1oo1, 1oo2, 2oo3), diagnostics, common-cause defences, systematic capability.
5. Quantify (FMEDA), verify, validate, assess with the required independence.

## Targets
| SIL | PFDavg (low demand) | PFH (per hour) |
|---|---|---|
| 1 | 1e-2 to 1e-1 | 1e-6 to 1e-5 |
| 2 | 1e-3 to 1e-2 | 1e-7 to 1e-6 |
| 3 | 1e-4 to 1e-3 | 1e-8 to 1e-7 |
| 4 | 1e-5 to 1e-4 | 1e-9 to 1e-8 |
- PL by PFHd: a 1e-5 to 1e-4, b 3e-6 to 1e-5, c 1e-6 to 3e-6, d 1e-7 to 1e-6, e 1e-8 to 1e-7.
- ISO 26262-5 hardware targets: SPFM >= 90/97/99% and LFM >= 60/80/90% for ASIL B/C/D; PMHF < 100 FIT (B, C), < 10 FIT (D).

## FMEDA formulas
- Split each failure rate into lambda_S, lambda_DD, lambda_DU.
- DC = lambda_DD/(lambda_DD + lambda_DU); SFF = (lambda_S + lambda_DD)/(lambda_S + lambda_D); no-effect failures are excluded.
- 1oo1: PFDavg ~ lambda_DU*T1/2 + lambda_DD*MTTR. 1oo2 with beta factor: ~ ((1-beta)*lambda_DU*T1)^2/3 + beta*lambda_DU*T1/2. T1 = proof-test interval. High demand 1oo1: PFH ~ lambda_DU.
- IEC 61508-2 Route 1H, max SIL at HFT 0 for SFF < 60 / 60-90 / 90-99 / >= 99%: type A 1/2/3/3, type B not allowed/1/2/3. Each extra HFT adds one SIL (cap SIL 4). Route 2H relies on field-proven reliability data instead.
- ISO 13849-1: category (B, 1-4) + MTTFd (low 3-10 y, medium 10-30 y, high 30-100 y) + DCavg (low 60-90%, medium 90-99%, high >= 99%) + CCF score >= 65 for categories 2-4 -> PL from the standard's chart or SISTEMA. Categories 3 and 4 tolerate a single fault.

## Checklist
- Trace hazard -> goal -> requirement -> design -> test, both ways.
- Software methods by level (IEC 61508-3, ISO 26262-6): coding standard, static analysis, structural coverage up to MC/DC for ASIL D.
- Tool confidence (ISO 26262-8 TCL; IEC 61508-3 T1-T3); proof-test and diagnostic intervals stated.

## Output
Safety plan, HARA table, safety requirements, FMEDA with metrics, verification report, safety case outline.

## Pitfalls
Quoting a SIL without demand mode; DC claims without fault-injection evidence; ignoring common cause. Formal sign-off belongs to a qualified, independent assessor.
