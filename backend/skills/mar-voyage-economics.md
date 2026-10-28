---
id: mar-voyage-economics
name: Voyage economics
category: maritime
description: Builds voyage estimates, converts freight to time-charter equivalent, models bunker use against speed and emissions cost, and reads Baltic and container indices and freight market drivers.
keywords: voyage estimate, tce, time charter equivalent, bunker consumption, slow steaming, freight rate, baltic dry index, bdi, ffa, breakeven, tonne-miles, voyage costs, eua cost, shipping market
source: Wikipedia - Baltic Dry Index | https://en.wikipedia.org/wiki/Baltic_Dry_Index
source: European Commission - Reducing emissions from the shipping sector | https://climate.ec.europa.eu/eu-action/transport-decarbonisation/reducing-emissions-shipping-sector_en
source: Sea-Intelligence - Global schedule reliability, August 2026 | https://www.sea-intelligence.com/press-room/410-global-schedule-reliability-continues-to-plummet-in-august
source: IMO - EEXI and CII FAQ | https://www.imo.org/en/MediaCentre/HotTopics/Pages/EEXI-CII-FAQ.aspx
---
## When to use
Pricing a voyage charter, comparing a voyage offer with a time charter, choosing speed, testing a freight budget, or explaining a rate move.

## Voyage estimate
1. Itinerary: ballast leg, laden leg, ports, canals; distances in nautical miles.
2. Sea days = distance / (speed in knots x 24), plus a weather margin (often 5 percent). Port days = cargo / handling rate + waiting + turn time.
3. Bunkers = sea days x sea consumption at that speed + port days x port consumption; price each grade (VLSFO, MGO, LNG).
4. Voyage costs = bunkers + port disbursements + canal dues + agency + owner's cargo costs + EU ETS cost + FuelEU deficit or pool cost where in scope.
5. Net freight = cargo x rate x (1 - commissions); address commission often 3.75 percent, brokerage 1.25 percent.
6. TCE per day = (net freight - voyage costs) / total voyage days (ballast + laden + port).
7. Compare TCE with the time-charter market and breakeven (daily opex + daily capital cost). Breakeven freight per tonne = (voyage costs + required daily earning x total days) / (cargo x (1 - commissions)).
Check: net freight USD 2.0 m, voyage costs USD 0.8 m, 40 days: TCE = 1.2 m / 40 = USD 30,000 per day.

## Speed, fuel, emissions
- Daily consumption scales roughly with speed cubed: C2 = C1 x (V2 / V1)^3 (exponent 2.5-3.5 by hull and draught; prefer the ship's own curve). Fuel per voyage then scales about with speed squared.
- Example: 40 t/day at 14 kn falls to about 29 t/day at 12.6 kn (0.9^3 = 0.729).
- Slow down while the fuel saved per day is worth more than the extra day at TCE or hire. High bunker prices, weak rates and CII pressure favour slow steaming.
- CO2 = fuel tonnes x Cf (t CO2 per t fuel: HFO 3.114, LFO 3.151, MGO or MDO 3.206, LNG 2.750).
- ETS cost = CO2 in scope (100 percent intra-EU and at berth, 50 percent extra-EU) x surrender share (100 percent from 2026 emissions) x EUA price.

## Indices
- Baltic Dry Index: 40 percent Capesize, 30 percent Panamax, 30 percent Supramax time-charter averages, times 0.1 (since March 2018; Handysize published separately). Sub-indices BCI, BPI, BSI, BHSI; tankers BDTI and BCTI. Panel brokers assess the routes daily.
- FFAs settle against Baltic routes to hedge freight exposure.
- Containers: SCFI, Drewry WCI, Freightos FBX, Xeneta; often used to index contracts.

## Market drivers
- Demand in tonne-miles (volume x distance): routing round the Cape of Good Hope instead of Suez adds roughly 10-14 days Asia to North Europe and absorbs capacity.
- Supply: fleet growth = deliveries - demolition; orderbook-to-fleet ratio; fleet speed; congestion (Asian port congestion pushed global liner reliability to 49.9 percent in August 2026, tying up ships).
- Also: bunker prices, seasonality (grain seasons, Chinese New Year), sanctions and shadow fleets, CII, ETS and FuelEU, canal draft limits.

## Output
Estimate sheet: assumptions (speed, consumption, prices, port days, EUA price), leg-by-leg days and bunkers, cost lines, net freight, TCE per day, breakeven freight rate, and a sensitivity table (speed, bunker price, port delay, rate) with the decision it supports.

## Pitfalls
- Comparing freight with hire before converting to TCE.
- Leaving out ballast days, commissions or ETS.
- Assuming fuel use is linear in speed.
