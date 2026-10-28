---
id: hw-emc-design
name: EMC design
category: hardware
description: Designs electronics to pass emissions and immunity tests: EMC by design, pre-compliance scans, CISPR 32/35 and IEC 61000-4-x ESD, EFT, surge and radiated immunity, filtering and shielding.
keywords: emc, emi, emissions, immunity, cispr 32, cispr 35, en 55032, iec 61000-4-2, esd, eft, surge, radiated, conducted, pre-compliance, filtering, shielding
source: IEC Webstore - IEC 61000-4-2:2025 edition 3.0 (ESD immunity) | https://webstore.iec.ch/en/publication/68954
source: IEC Webstore - CISPR 32:2015+AMD1:2019 CSV edition 2.1 | https://webstore.iec.ch/en/publication/65836
source: IEC Webstore - IEC 61000-4-3:2020 edition 4.0 (radiated immunity) | https://webstore.iec.ch/publication/59849
source: IEC Webstore - IEC 61000-4-4:2012 edition 3.0 (EFT/burst) | https://webstore.iec.ch/publication/4222
source: European Commission - EMC Directive 2014/30/EU | https://single-market-economy.ec.europa.eu/sectors/electrical-and-electronic-engineering-industries-eei/electromagnetic-compatibility-emc-directive_en
---
## When to use
EMC design reviews, pre-compliance plans, debugging a failed scan, choosing filters, shields and protection, writing the EMC test plan.

## Standards (check editions locally)
- Emissions: CISPR 32 ed. 2.1 (2015+A1:2019, stability date 2026, watch for ed. 3); EU uses EN 55032 as cited in the OJ. Class B residential, Class A commercial (Class A needs the interference warning in the user information). US: FCC Part 15 Subpart B.
- Immunity: CISPR 35 (EN 55035) calls basic standards: IEC 61000-4-2 ESD (current ed. 3, 2025); -4-3 radiated RF (ed. 4, 2020); -4-4 EFT (ed. 3, 2012); -4-5 surge; -4-6 conducted RF; -4-8 magnetic field; -4-11 dips. Test to the basic-standard edition the product standard references (dated references are fixed); record any newer edition the lab uses. Mains products add IEC 61000-3-2/-3-3.
- Typical CISPR 35 levels (verify per port table): ESD 4 kV contact, 8 kV air; RF 3 V/m 80-1000 MHz plus spot frequencies to 5 GHz; EFT 1 kV AC, 0.5 kV signal; surge 1 kV line-line, 2 kV line-earth; conducted 3 V.
- Criteria: A works within spec; B degrades and recovers by itself; C needs user reset, no damage.

## Limits to remember
- Radiated Class B at 3 m: CISPR 32 40 dBuV/m (30-230 MHz), 47 (230-1000 MHz), quasi-peak. FCC Class B at 3 m: 40, 43.5, 46, 54 dBuV/m at 30-88, 88-216, 216-960, >960 MHz.
- Conducted Class B mains: QP 66 to 56 dBuV (0.15-0.5 MHz), 56 (0.5-5), 60 (5-30); average 10 dB lower.

## Formulas
- Common-mode cable emission: E = 1.26e-6 * f * L * I_cm / r (V/m; f Hz, L cable m, I A, r m). At 100 MHz, 1 m, 3 m, about 2.4 uA of CM current already reaches 40 dBuV/m.
- Differential loop: E = 1.3e-14 * f^2 * A * I / r (A loop area m^2, free space; about double over a ground plane). Halve the loop area: -6 dB.
- Aperture: shielding falls off once the longest slot dimension nears lambda/2; aim below lambda/20 at the highest frequency of concern. Slot length matters, not area.

## Design rules
1. Control sources: slowest acceptable edges, series resistors, clock routing on inner layers, spread spectrum only as a last step (it lowers peaks, not energy).
2. Shrink hot loops: switcher input loop, gate drive, switch-node copper area; snubbers where ringing shows.
3. Cables are the antennas: filter and protect at the connector, referenced to chassis; CM chokes on pairs; ferrites; 360-degree shield bonding at entry, no pigtails.
4. ESD and surge: TVS at the connector with a short path to chassis; keep the discharge path away from logic; series R or ferrite after the TVS.
5. Solid reference plane under I/O; chassis bond at the I/O edge; no plane splits under connectors.
6. Y-capacitors are limited by touch current under the safety standard.

## Pre-compliance
- Near-field probes to locate sources; spectrum analyser in peak first, then QP/average; current clamp on each cable vs the CM limit above; LISN for conducted.
- Keep 6 dB margin to the limit for lab uncertainty and unit spread. Test every operating mode and cable arrangement.

## Output
EMC plan: standards and editions, class, port table (AC, DC, signal, telecom, enclosure), tests and levels per port, criteria per function, operating modes, monitoring, EUT setup, sample count. Reviews: finding, risk, fix, how verified.

## Pitfalls
Pigtail shields; filters far from the connector; ESD current through signal ground; one test mode only; Y-cap leakage over the safety limit; mixing basic-standard editions without checking which one the product standard references.
