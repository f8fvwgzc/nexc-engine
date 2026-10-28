---
id: hw-components-and-bom
name: Components and BOM
category: hardware
description: Selects and reviews components and the bill of materials: derating, lifecycle and obsolescence (PCN, EOL, last-time buy), second sources, counterfeit avoidance, BOM hygiene and cost.
keywords: bom, bill of materials, component selection, derating, obsolescence, eol, pcn, last time buy, ltb, second source, alternate part, counterfeit, avl, lifecycle, lead time, cost down
source: NASA GSFC EEE-INST-002 (2003, Addendum 1 2008) - parts selection, screening and derating | https://nepp.nasa.gov/docuploads/FFB52B88-36AE-4378-A05B2C084B5EE2CC/EEE-INST-002_add1.pdf
source: Wikipedia - Obsolescence (IEC 62402:2019 obsolescence management) | https://en.wikipedia.org/wiki/Obsolescence
source: Wikipedia - Counterfeit electronic component (SAE AS5553, AS6081) | https://en.wikipedia.org/wiki/Counterfeit_electronic_component
---
## When to use
Part selection, BOM review before each build, PCN or EOL responses, shortages, second-source qualification, cost-down and counterfeit risk.

## Method
1. Select for the worst case: temperature grade (commercial 0 to 70 degC, industrial -40 to 85 degC, AEC-Q100 grade 1 -40 to 125 degC; AEC-Q101 discretes, AEC-Q200 passives), lifecycle (active, NRND, LTB, EOL, obsolete), lead time, number of sources, MSL, RoHS/REACH data.
2. Derate to a written policy. Space-grade example (NASA EEE-INST-002, Table 4 per family): ceramic caps 0.6 x rated voltage (ripple peak plus DC); solid tantalum 0.5 at 70 degC and 0.3 at 110 degC; most resistors 0.6 x power, 0.8 x voltage; transistors 0.6 x power, 0.75 x current and voltage, MOSFET V_GS 0.6; diodes 0.5 x forward current, 0.7 x PIV; ICs 0.9 x supply (digital), 0.8 (linear). T_j: ICs <= 110 degC, discretes <= 125 degC, or 40 degC below T_jmax if lower. Plastic-encapsulated parts get stricter limits. Commercial policies are lighter; cite the one used.
3. Check effective values: Class 2 MLCC capacitance after DC bias, temperature and ageing; aluminium electrolytic life roughly L = L0 * 2^((T_rated - T_use)/10).
4. Lifecycle: subscribe to supplier PCN and EOL notices (JEDEC J-STD-046 and JESD48 set notice practice; check current revisions). Manage obsolescence per IEC 62402:2019, proactively for long-life products.
5. Mitigation ladder: approved alternate (form-fit-function) -> authorised aftermarket source -> last-time buy or bridge buy -> emulation -> redesign.
6. LTB quantity = remaining-life demand + spares + yield loss + safety stock - stock on hand. Add storage plan (dry pack, MSL, shelf life) and holding cost.
7. Second sources: compare absolute maximums, electrical tables, timing, thermal and package drawings, then sample-test; record the approved manufacturer list (AML) on the BOM.
8. Counterfeit avoidance: buy from the OCM or franchised distributors. If a broker is unavoidable, apply SAE AS5553 (equipment makers) and AS6081 (distributors) with AS6171 test methods: traceability to the OCM, external visual and marking checks, X-ray, XRF, decapsulation, electrical tests. Quarantine suspects and report them (GIDEP, ERAI).

## BOM hygiene checklist
- One controlled BOM in PLM; revision matches the board.
- One line per MPN with reference designators; quantity equals designator count.
- Manufacturer name plus MPN, never a distributor SKU alone; standard descriptions (value, tolerance, voltage, dielectric, package).
- DNP marked; alternates listed; lifecycle, lead time, and compliance (RoHS exemption, SVHC, conflict minerals) per line.
- No "or equivalent" without the parameters that define equivalent.

## Cost
Fewer unique values, standard E-series values, common packages, reel-size and MOQ effects, price breaks, country of origin and tariffs, assembly cost of tiny packages, and lifetime-buy financing.

## Output
BOM risk register: MPN | function | sources | lifecycle | lead time | derating margin | compliance | risk H/M/L | action | owner.

## Pitfalls
- Designator and quantity mismatch; PCNs that change package or reflow profile.
- Broker parts without certificate of conformance or traceability.
- LTB with no storage plan; ignoring DC bias and ripple current.
