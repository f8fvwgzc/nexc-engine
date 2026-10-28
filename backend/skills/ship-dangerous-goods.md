---
id: ship-dangerous-goods
name: Dangerous goods
category: shipping
description: Classifies dangerous goods by hazard class, UN number and packing group, and checks packing, marking, documents and segregation under the IMDG Code, IATA DGR and ADR, including lithium batteries.
keywords: dangerous goods, hazmat, imdg code, iata dgr, adr, un number, packing group, hazard class, dg declaration, lithium batteries, un3480, un3481, segregation, limited quantity, sds
source: IATA - Battery guidance document, revised for the 2026 regulations | https://www.iata.org/contentassets/05e6d8742b0047259bf3a700bc9d42b9/lithium-battery-guidance-document.pdf
source: IATA - Dangerous Goods Regulations | https://www.iata.org/en/publications/dgr/
source: IMO - Dangerous goods (IMDG Code) | https://www.imo.org/en/OurWork/Safety/Pages/DangerousGoods-default.aspx
source: IMO - MSC 108 meeting summary | https://www.imo.org/en/MediaCentre/MeetingSummaries/Pages/MSC-108th-session.aspx
---
## When to use
An SDS shows a UN number, the goods contain batteries, gases, flammables or chemicals, or a carrier rejects a booking. You prepare and check; a trained shipper signs the declaration and safety sign-off stays with that competent person.

## Current editions
- IMDG Code Amendment 42-24, mandatory from 1 January 2026; two-year cycle (next amendment optional 2027, mandatory 2028, verify locally).
- IATA DGR 67th edition for 2026, based on the 2025-2026 ICAO Technical Instructions; the 68th edition applies from 1 January 2027. Check State and operator variations.
- ADR 2025 for European road (next edition expected 2027, verify locally). All follow the UN Model Regulations.

## Classification
| Class | Content |
|---|---|
| 1 | explosives, divisions 1.1-1.6 |
| 2 | gases: 2.1 flammable, 2.2 non-flammable non-toxic, 2.3 toxic |
| 3 | flammable liquids, flash point up to 60 C |
| 4 | 4.1 flammable solids, 4.2 spontaneously combustible, 4.3 dangerous when wet |
| 5 | 5.1 oxidisers, 5.2 organic peroxides |
| 6 | 6.1 toxic, 6.2 infectious |
| 7 | radioactive |
| 8 | corrosive |
| 9 | miscellaneous: batteries, UN3077/UN3082 environmentally hazardous |

- Packing group: I high, II medium, III low danger. Class 3: PG I boiling point 35 C or less; PG II flash point under 23 C; PG III flash point 23-60 C.
- Steps: SDS section 14, UN number and proper shipping name, class, subsidiary hazard, PG, marine pollutant, special provisions, limited or excepted quantity, packing instruction, UN packaging code (X for PG I-III, Y for II-III, Z for III), marks and labels, document.

## By mode
- Sea (IMDG): DG List gives stowage category A-E (class 1: 01-05) and EmS (F-x fire, S-x spillage); segregate by the chapter 7.2 table, from "away from" to "separated longitudinally by an intervening compartment"; multimodal DG form (5.4) plus container packing certificate; placards on all four sides.
- Air (DGR): Shipper's Declaration; passenger versus cargo-aircraft-only package limits.
- Road (ADR): transport document, orange plates, tunnel codes, 1.1.3.6 small-load exemption, driver certificate, appointed DGSA.

## Batteries by air (2026)
- UN3480 Li-ion alone, UN3481 with or in equipment; UN3090/UN3091 lithium metal; UN3551/UN3552 sodium-ion.
- UN 38.3 test summary required; Section II limits: cell 20 Wh, battery 100 Wh (lithium metal 1 g and 2 g).
- UN3480 (PI 965) and UN3551 (PI 976) alone: forbidden on passenger aircraft, Cargo Aircraft Only label, state of charge max 30 percent (higher only with State approval, A331). Power banks count as batteries (UN3480 or UN3090), not equipment.
- From 1 January 2026 Li-ion packed with equipment (PI 966) also ships at max 30 percent (Section II above 2.7 Wh); for batteries contained in equipment (PI 967) 30 percent is strongly recommended.
- Batteries identified as damaged or defective for safety reasons are forbidden by air (Special Provision A154); route them by sea or road under the damaged-battery provisions.

## Output
One line per item: UN number, proper shipping name, class and subsidiary risk, PG, net quantity per package, packing instruction or method, marks and labels, document, mode or aircraft restriction, open questions; then the corrections needed and who must sign.

## Pitfalls
- Accepting a "non-DG" claim without SDS section 14 or a battery test summary.
- Reusing sea classification for air without checking DGR variations.
- PSN or UN number mismatch between declaration, B/L and labels; missing emergency response information required by the country (e.g. US 49 CFR).
