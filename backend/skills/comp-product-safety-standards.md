---
id: comp-product-safety-standards
name: Product safety standards
category: product-compliance
description: Applies product safety standards to a design: IEC 62368-1 hazard-based safety, IEC 61010-1, ISO 12100 machine risk assessment, IEC 60204-1, UL listing and the IECEE CB Scheme.
keywords: product safety, iec 62368-1, hbse, iec 61010-1, iso 12100, risk assessment, iec 60204-1, ul listing, nrtl, cb scheme, iecee, creepage, clearance, insulation, safeguards
source: IEC Webstore - IEC 62368-1:2023 edition 4.0 | https://webstore.iec.ch/en/publication/85813
source: IEC Webstore - IEC 61010-1:2010+AMD1:2016 CSV edition 3.1 | https://webstore.iec.ch/en/publication/59769
source: IEC Webstore - IEC 60204-1:2016+AMD1:2021 CSV | https://webstore.iec.ch/publication/26037
source: European Commission - Low Voltage Directive | https://single-market-economy.ec.europa.eu/sectors/electrical-and-electronic-engineering-industries-eei/low-voltage-directive-lvd_en
source: European Commission - Machinery | https://single-market-economy.ec.europa.eu/sectors/mechanical-engineering/machinery_en
---
## When to use
Choosing the safety standard, safety design reviews, preparing UL or CB testing, machine risk assessments, critical component control.

## Pick the standard
- AV/ICT equipment: IEC 62368-1:2023 (ed. 4, May 2023); it superseded IEC 60950-1 and IEC 60065 (their EN versions were withdrawn on 20 December 2020). EU: use the EN 62368-1 version cited under the LVD; US/Canada: UL/CSA 62368-1 with national differences.
- Measurement, control, lab: IEC 61010-1 ed. 3.1 (2010+A1:2016, stability date 2026, watch for ed. 4) with its Part 2 standards (e.g. 61010-2-030 for measuring circuits).
- Machinery: ISO 12100:2010 (type-A risk assessment), IEC 60204-1:2016+A1:2021 (electrical equipment), ISO 13849-1 or IEC 62061 for safety functions.
- Out of scope here: medical (IEC 60601-1), household (IEC 60335-1) unless the task says so.

## IEC 62368-1 hazard-based method
1. Identify energy sources: electrical ES, fire PS, mechanical MS, thermal TS, radiation RS, chemical.
2. Classify 1 (not painful), 2 (painful, not injurious), 3 (injurious). Steady-state ES1: voltage <= 60 V DC or 30 V rms / 42.4 V peak, or touch current <= 2 mA DC or 0.5 mA rms (either within limit is enough); ES2: <= 120 V DC or 50 V rms / 70.7 V peak, or <= 25 mA DC or 5 mA rms; above is ES3 (check Table 4 of the edition used). PS1 at most 15 W after 3 s; PS2 at most 100 W after 5 s; above is PS3.
3. Identify persons: ordinary, instructed, skilled.
4. Place safeguards between class 2 or 3 sources and body parts: ordinary persons need a basic safeguard against class 2 and basic plus supplementary, or reinforced, against class 3. Instruction alone never protects ordinary persons from class 3.
5. Verify: clearance and creepage (working voltage, overvoltage category, pollution degree, material group, altitude above 2000 m), electric strength, touch current, temperature rise, abnormal and single-fault tests, enclosure, flammability class, stability.

## ISO 12100 risk assessment
Set machine limits (use, space, time, lifecycle) -> identify hazards per task -> estimate risk from severity and probability (exposure, hazardous event, ability to avoid) -> evaluate -> reduce in three steps: inherently safe design, then safeguarding and complementary measures, then information for use. Iterate and document each hazard.

## IEC 60204-1 checks
Supply disconnecting device; emergency stop as stop category 0 or 1; protection by automatic disconnection; protective bonding continuity; insulation resistance at least 1 MOhm at 500 V DC; residual voltage at most 60 V within 5 s; PE conductor green-yellow; documentation.

## UL and CB Scheme
- US/Canada: an NRTL (UL, CSA, Intertek, TUV and others) lists the product to the UL/CSA standard and runs factory follow-up visits.
- IECEE CB Scheme: a CBTL tests, an NCB issues the CB Test Certificate and Report against the IEC standard plus declared national differences; other NCBs use it for national certification with little or no retest. Request every target country's differences up front.
- Freeze the critical components list (part, maker, rating, certification); any change needs an amendment.

## Output
Energy source table (source | class | persons | safeguards | verification), critical components list, test plan, risk assessment table for machines, open issues with owner.

## Pitfalls
Citing withdrawn IEC 60950-1; ignoring altitude or pollution degree; swapping a certified part without notice; undeclared persons; treating a safety report as sign-off. Binding safety sign-off belongs to a qualified engineer or the certification body.
