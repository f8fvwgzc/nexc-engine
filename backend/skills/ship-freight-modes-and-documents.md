---
id: ship-freight-modes-and-documents
name: Freight modes and documents
category: shipping
description: Selects ocean FCL or LCL, air, road, rail or multimodal transport, computes chargeable weight, and prepares and checks bills of lading, waybills, CMR notes and SOLAS VGM.
keywords: fcl, lcl, air freight, chargeable weight, bill of lading, sea waybill, air waybill, awb, cmr, rail freight, multimodal, vgm, transit time, freight forwarding, consignment note
source: IMO - Verification of the gross mass of a packed container | https://www.imo.org/en/OurWork/Safety/Pages/Verification-of-the-gross-mass.aspx
source: IATA - Electronic air waybill (e-AWB) | https://www.iata.org/en/programs/cargo/e/eawb/
source: Wikipedia - Intermodal container (ISO dimensions and ratings) | https://en.wikipedia.org/wiki/Intermodal_container
source: Sea-Intelligence - Global schedule reliability, August 2026 | https://www.sea-intelligence.com/press-room/410-global-schedule-reliability-continues-to-plummet-in-august
---
## When to use
Choosing mode or routing, quoting, issuing or checking a transport document, or planning transit against a delivery date.

## Modes and liability
| Mode | Fits | Cargo liability limit |
|---|---|---|
| Ocean FCL | over about 15 m3 or one shipper per box | Hague-Visby: 666.67 SDR per package or 2 SDR/kg, whichever is higher; US COGSA USD 500 per package |
| Ocean LCL | under about 15 m3; CFS handling adds days | as ocean |
| Air | urgent, high value | Montreal Convention 1999: 26 SDR/kg (revised Dec 2024, verify locally) |
| Road | regional door to door | CMR: 8.33 SDR/kg; claims barred after 1 year (3 for wilful misconduct) |
| Rail | China-Europe, inland | CIM/COTIF 17 SDR/kg; SMGS on the 1520 mm network |
| Multimodal | one contract, several modes | network liability; FIATA FBL |

## Formulas
- LCL revenue ton: W/M = max(gross tonnes, cubic metres).
- Air chargeable weight = max(gross kg, volume kg); volume kg = L x W x H in cm / 6000 (IATA; couriers often use 5000). Example: 120 x 80 x 100 cm at 90 kg gives 160 kg chargeable.
- Road: loading metres = deck length used; 13.6 m trailer holds 33 euro pallets (120 x 80 cm); tariffs convert ldm to weight at a stated ratio.
- Container capacity: 20 ft about 33 m3, 40 ft 67 m3, 40 ft HC 76 m3; ISO max gross 36,000 kg, but road limits usually bind first.
- Transit plan = origin cut-offs (booking, SI, VGM, CY) + port to port + transshipment dwell + clearance and delivery + buffer sized to current reliability (global on-time was 49.9 percent in August 2026).

## Documents
- Bill of lading: receipt, evidence of contract and, if negotiable ("to order"), document of title. Variants: shipped on board vs received; straight vs order; clean vs claused; master (carrier) vs house (NVOCC). Surrender the full original set or use telex or express release.
- Sea waybill: non-negotiable, no title; quickest release where no bank or credit risk.
- Electronic B/L: valid where law allows (MLETR-based laws such as the UK Electronic Trade Documents Act 2023) or within a platform rulebook.
- Air waybill: non-negotiable contract of carriage; 11 digits with the 3-digit airline prefix; e-AWB under IATA Resolution 672 multilateral agreement.
- CMR consignment note: three originals (sender, carrier, consignee); e-CMR under the 2008 Protocol where ratified.
- SOLAS VGM (regulation VI/2, since 1 July 2016; guidance MSC.1/Circ.1475): the shipper declares verified gross mass by Method 1 (weigh the packed box) or Method 2 (weigh all contents and add tare, certified method). No VGM, no loading.

## Checklist
- Parties, ports, description, marks, packages, weights, HS code, Incoterm and freight terms (prepaid or collect) match across invoice, packing list, B/L and LC.
- Claused B/L, wrong consignee or late on-board date caught before presentation.

## Output
Option table (mode, routing, door-to-door days with buffer, chargeable weight or W/M, all-in cost, liability regime, CO2e); the document set with issuer and number of originals; cut-off calendar; discrepancies found and how to fix each.

## Pitfalls
- Non-negotiable document where an LC or financing needs title.
- Quoting air on gross weight; ignoring dimensional weight.
- Planning on brochure transit times during congestion.
