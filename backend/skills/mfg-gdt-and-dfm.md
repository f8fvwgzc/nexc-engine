---
id: mfg-gdt-and-dfm
name: GD&T and design for manufacture
category: manufacturing
description: Reads and specifies ASME Y14.5-2018 GD&T, builds worst-case and RSS tolerance stack-ups, and reviews part designs for manufacture and assembly by process.
keywords: gd&t, asme y14.5, datum, feature control frame, true position, mmc, bonus tolerance, tolerance stack-up, worst case, rss, dfm, dfa, dfma, draft angle, wall thickness, gdtp
source: ASME - Y14.5 Dimensioning and Tolerancing | https://www.asme.org/codes-standards/find-codes-standards/y14-5-dimensioning-tolerancing
source: Wikipedia - Geometric dimensioning and tolerancing | https://en.wikipedia.org/wiki/Geometric_dimensioning_and_tolerancing
source: Wikipedia - Design for manufacturability | https://en.wikipedia.org/wiki/Design_for_manufacturability
---
## When to use
Interpreting or writing drawings, checking fit, building stack-ups, or reviewing a design before quoting or tooling.

## ASME Y14.5-2018 essentials
- Current edition: Y14.5-2018, reaffirmed 2024, in stabilized maintenance. Concentricity and symmetry were removed; use position, profile or runout. ISO drawings follow ISO GPS (ISO 1101, 5458, 5459, 8015) with different defaults: ISO 8015 applies independency, so size limits do not control form unless the envelope symbol (E) is shown. Never mix the systems.
- Characteristics: form (flatness, straightness, circularity, cylindricity), orientation (perpendicularity, angularity, parallelism), location (position), profile (line, surface), runout (circular, total).
- Datum reference frame: primary datum feature removes up to 3 degrees of freedom, secondary 2, tertiary 1; order follows function (how the part mounts and locates). Modifiers on datum features: MMB, LMB, RMB.
- Feature control frame: symbol | zone (diameter sign for cylindrical) and modifier | datums in precedence order.
- Rule #1: the size limits of a regular feature of size also control its form (envelope at MMC) unless independency is noted. Tolerances apply RFS unless MMC or LMC is shown.
- Bonus at MMC: allowed tolerance = stated + |actual size - MMC size|. Virtual condition: internal feature = MMC - geometric tolerance; external = MMC + tolerance.
- Fasteners at MMC: floating, T = H - F; fixed, T1 + T2 = H - F (H = hole MMC, F = fastener max diameter); use a projected zone for threaded or press-fit holes.
- Measured position: diameter zone used = 2*sqrt(dx^2 + dy^2).
- Profile of a surface to datums can control size, form, orientation and location at once: the default choice for castings, mouldings and freeform.

## Tolerance stack-ups
1. Define the gap or requirement; draw the loop; assign + and - directions.
2. Convert every dimension to mean +/- equal bilateral; add GD&T contributors (position as +/- half the zone, plus bonus or shift where it can occur).
3. Worst case: T_wc = sum(t_i); guarantees fit, can force tight, costly tolerances.
4. RSS: T_rss = sqrt(sum(t_i^2)), assuming independent, centred, normal contributors with t_i = 3 sigma. Inflate (e.g. 1.5 x RSS) or use sigma_i = t_i/(3*Cpk_i) when processes are not centred.
5. Use worst case for few contributors, safety-critical fits or low volume; RSS for many contributors from capable, controlled processes; Monte Carlo for non-linear or non-normal cases. Mark statistically toleranced features on the drawing.
6. Example: four contributors at +/-0.10 each give +/-0.40 worst case and +/-0.20 RSS.

## DFM by process (rules of thumb; confirm with the supplier)
- CNC: fewest setups; internal corner radius larger than the cutter; tool reach near 3x diameter is cheap; avoid thin walls; standard drill and thread sizes; loosest tolerance that works, per feature.
- Injection moulding: uniform walls; ribs 50-60% of wall; draft >= 1 deg, more for texture; no undercuts unless side actions are budgeted; inside radius about 0.5 x wall.
- Sheet metal: inside bend radius >= thickness; holes at least 2.5 x thickness + radius from bends; flange length >= 4 x thickness.
- Castings: draft, even sections, generous fillets, machining stock on datum faces.

## DFA
Cut part count (does it move, need another material, or need separate service?); design efficiency = 3 x N_min / total assembly time (s); self-locating, top-down assembly; symmetric or clearly asymmetric parts; common fasteners; poka-yoke.

## Pitfalls
Datums that do not match function; RSS for a 2-part stack; ignoring bonus or datum shift; GD&T copied from an old drawing.
