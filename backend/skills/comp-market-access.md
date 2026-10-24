---
id: comp-market-access
name: Market access certification
category: product-compliance
description: Maps a product to the approvals each market requires: CE marking (LVD, EMC, RED with EN 18031, Machinery Regulation), UKCA, FCC Part 15 and SDoC, ISED, Japan PSE and Radio Act, Korea KC, China CCC.
keywords: market access, certification, ce marking, red, en 18031, lvd, emc directive, machinery regulation, ukca, fcc, sdoc, ised, pse, telec, kc mark, ccc
source: European Commission - Radio Equipment Directive and Delegated Regulation 2022/30 | https://single-market-economy.ec.europa.eu/sectors/electrical-and-electronic-engineering-industries-eei/radio-equipment-directive-red_en
source: European Commission - Machinery (Regulation (EU) 2023/1230) | https://single-market-economy.ec.europa.eu/sectors/mechanical-engineering/machinery_en
source: European Commission - Cyber Resilience Act | https://digital-strategy.ec.europa.eu/en/policies/cyber-resilience-act
source: GOV.UK - Using the UKCA marking | https://www.gov.uk/guidance/using-the-ukca-marking
source: Cornell LII - 47 CFR 2.1077 compliance information for SDoC | https://www.law.cornell.edu/cfr/text/47/2.1077
---
## When to use
Approval plans and budgets per market; checking a CE, UKCA or FCC route.

## Method
1. Profile: supply, radio, battery, moving parts, connectivity, consumer or professional, markets, launch date, product life.
2. Per market: regimes, route (self-declaration or third party), standards with editions, samples, lead time.
3. Check which dates fall in the launch and product-life window.
4. Test in DVT; build technical file, declarations, labels.

## EU (CE)
- LVD 2014/35/EU: 50-1000 V AC, 75-1500 V DC; internal production control, no notified body.
- EMC 2014/30/EU: Module A, or EU-type examination (Module B+C) by choice.
- RED 2014/53/EU: safety and EMC of radio products, Art. 3(1) (no lower voltage limit), plus spectrum, Art. 3(2). Module A always allowed for 3(1); for 3(2)/3(3) only with harmonised standards applied in full, else B+C or H with a notified body.
- RED cybersecurity: Delegated Regulation (EU) 2022/30 activates Art. 3(3)(d) network, (e) privacy, (f) fraud for internet-connected radio, toys and wearables, applied since 1 August 2025. EN 18031-1/-2/-3:2024 were cited in the OJ in January 2025 with restrictions (e.g., designs letting users skip passwords); a restriction hit means a notified body. Check later amendments to Implementing Decision (EU) 2022/2191. Delegated Regulation (EU) 2026/339 repeals 2022/30 from 11 December 2027, when the CRA takes over.
- CRA (EU) 2024/2847: reporting of exploited vulnerabilities and severe incidents since 11 September 2026; main obligations from 11 December 2027.
- Machinery Regulation (EU) 2023/1230 applies from 20 January 2027, replacing 2006/42/EC; Annex I Part A needs a notified body, Part B only if harmonised standards are not fully applied; digital instructions allowed.
- Also RoHS, GPSR (EU) 2023/988 for consumer goods, ecodesign (e.g. standby (EU) 2023/826), Battery Regulation, and an EU economic operator (Regulation (EU) 2019/1020 Art. 4).

## UK
- GB recognises CE marking for most regimes under the Product Safety and Metrology (Amendment) Regulations 2024; UKCA remains an option, and may sit on a label or document until 31 December 2027. Northern Ireland follows EU rules. Marks at least 5 mm; UK importer address.

## US and Canada
- FCC Part 15: unintentional radiators (Subpart B, Class A or B) by SDoC or certification (47 CFR 15.101); intentional radiators by certification through a TCB with an FCC ID. SDoC responsible party must be in the US; compliance information goes with the product (47 CFR 2.1077).
- Safety: NRTL listing (OSHA scheme) is expected by workplaces and retailers.
- ISED: ICES-003 for ITE (label CAN ICES-003(B)/NMB-003(B)); radio needs certification to RSS-Gen plus the band RSS (verify issues locally).

## Asia
- Japan PSE (Electrical Appliances and Materials Safety Act): specified products (diamond mark) need a registered conformity assessment body; others (round mark) self-check; notify METI. Radio Act: technical conformity certification by a registered certification body (TELEC is one), Giteki mark; telecom terminals need separate approval.
- Korea KC: radio and EMC via RRA under the Radio Waves Act; electrical safety KC under KATS.
- China CCC (SAMR/CNCA catalogue): type test, factory inspection, follow-up; radio needs SRRC type approval; telecom NAL.
- Asian schemes change often: verify locally.

## Output
Certification matrix: market | regime | standard and edition | route | body or lab | samples | lead time | cost | marking | status. Gaps with owner and date.

## Pitfalls
- A certified radio module does not certify the host: EMC, safety, RF exposure still apply.
- Using a harmonised standard against its OJ restriction.
- Assuming an FCC grant covers Canada or Japan.
- Missing importer or EU operator details on the label.
