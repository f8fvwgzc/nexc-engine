---
id: hw-schematic-and-pcb
name: Schematic and PCB design
category: hardware
description: Designs and reviews schematics and PCB layouts to IPC-2221C and IPC-2152: conductor sizing, clearance, stack-up, grounding and return paths, DFM/DFA and release checklists.
keywords: pcb, schematic, layout, ipc-2221, ipc-2152, trace width, current capacity, stack-up, grounding, return path, dfm, dfa, design review, footprint, fabrication drawing, gerber
source: ANSI Webstore - IPC-2221C (2023) Generic Standard on Printed Board Design | https://webstore.ansi.org/standards/ipc/ipc2221c2023
source: IPC - CID and CID+ designer certification program | https://www.electronics.org/ipc-designer-certification-program
source: Sierra Circuits - Optimizing PCB traces with IPC-2152 | https://www.protoexpress.com/blog/how-to-optimize-your-pcb-trace-using-ipc-2152-standard/
source: Altium - IPC-2221 calculator for trace current and heating | https://resources.altium.com/p/ipc-2221-calculator-pcb-trace-current-and-heating
---
## When to use
Schematic review, stack-up definition, conductor sizing, layout review and the fab/assembly release package.

## Key standards
- IPC-2221C (2023, replaces 2221B): generic board design rules and the conductor spacing table (Table 6-1 in 2221B: B1 internal, B2/B3 external uncoated below/above 3050 m, B4 coated board, A5-A7 assembly); check the table number in the C revision. IPC-2222 for rigid boards.
- IPC-2152 (2009): conductor sizing from measured data. IPC-6012 board qualification, IPC-A-610 and J-STD-001 assembly. Set the product class first: 1 general, 2 dedicated service, 3 high reliability.
- Mains clearance and creepage come from the safety standard (IEC 62368-1, IEC 61010-1, IEC 60664-1); it usually governs over IPC values.

## Formulas
- IPC-2221 legacy: I = k * dT^0.44 * A^0.725. I in A, dT rise in degC, A cross-section in mil^2; k = 0.048 external, 0.024 internal. Width = A / t; 1 oz Cu is about 1.37 mil (35 um).
- Example: 3 A, 10 degC, external: A = (3 / (0.048 * 10^0.44))^(1/0.725) = 74 mil^2, so about 54 mil (1.38 mm) in 1 oz.
- IPC-2152 is the better basis: inner traces run close to outer ones at equal current; nearby planes cool traces; thin boards and vacuum run hotter. The 2221 inner curve (half of k) oversizes inner traces.
- Resistance R = rho * L / (w * t), rho(Cu) = 1.72e-8 ohm*m at 20 degC, +0.39 %/degC. Budget IR drop per rail; put several vias on power transitions.

## Stack-up and return paths
- Each signal layer next to a solid reference plane; power and ground planes closely spaced; symmetric build to limit bow and twist.
- 4 layers: Sig/GND/PWR/Sig minimum; Sig/GND/GND/Sig with poured power is quieter. 6 layers: Sig/GND/Sig/PWR/GND/Sig, with fast nets on layers next to ground.
- Fast return current flows under the trace, not along the shortest path. Never route across plane splits or slots. On a layer change, add a ground stitching via (same reference) or stitching capacitor (different nets) within a few mm.
- One solid ground; partition by placement, not by cutting. Keep the switching-regulator hot loop (input cap, switches) tiny.

## Schematic checklist
- Every IC: all power pins, datasheet decoupling, exposed pad, straps and reset defined, unused inputs tied.
- Power tree: rails, sequencing, inrush, reverse polarity, fuse, TVS at external connectors.
- Ratings derated; pull-up values; I2C bus capacitance; level shifting between domains.
- Test points on every rail, debug and programming access; ERC clean or waivers documented.

## Layout checklist
- DRC set to the fab's capability: trace/space, annular ring, drill aspect ratio (often <= 10:1), mask dams, copper-to-edge.
- DFA: spacing for rework and AOI, pin-1 and polarity marks, thermal reliefs on plane pads, filled and capped via-in-pad, balanced copper on small passives against tombstoning, three fiducials, panel rails.
- DFT: probe-accessible test points per the ICT or flying-probe vendor rules.
- Release: Gerber X2 or ODB++/IPC-2581, drill, fab drawing (stack-up, impedance table, laminate Tg/Td, finish, class), assembly drawing, pick-and-place, BOM.

## Output
Review table: ID | sheet or layer and location | finding | severity | fix | how verified. Add calculations with units and the stack-up table.

## Pitfalls
- Footprint errors are a top respin cause: check against the datasheet land pattern and the 3D model.
- Sizing power traces with the 2221 inner curve or ignoring neighbouring heat sources.
- Slots or splits under high-speed nets; missing creepage slots on mains boards.
