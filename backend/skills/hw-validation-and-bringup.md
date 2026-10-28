---
id: hw-validation-and-bringup
name: Validation and bring-up
category: hardware
description: Plans and runs board bring-up and hardware validation: EVT/DVT/PVT gates, test plans, HALT/HASS, reliability math (MTBF, FIT, acceleration factors) and failure analysis.
keywords: bring-up, board bring-up, evt, dvt, pvt, validation, test plan, halt, hass, reliability, mtbf, fit, arrhenius, failure analysis, environmental test, iec 60068
source: Wikipedia - Engineering validation test (EVT, DVT) | https://en.wikipedia.org/wiki/Engineering_validation_test
source: Wikipedia - Highly accelerated life test | https://en.wikipedia.org/wiki/Highly_accelerated_life_test
source: Wikipedia - Failure rate (FIT, MTBF, series systems) | https://en.wikipedia.org/wiki/Failure_rate
---
## When to use
First power-on of a new board, build-phase planning, test plans, reliability prediction or demonstration, and root-cause work on failures.

## Bring-up procedure
1. Before power: visual or AOI check, polarity and orientation, straps and jumpers; measure each rail to ground (a near-zero reading means a short).
2. Bench supply with a low current limit; ramp slowly; compare current to the expected value; thermal camera for hot spots.
3. Rails one at a time: voltage, ripple (scope with a short ground spring), sequencing, power-good and reset timing; then clocks.
4. Debug path next (JTAG/SWD/UART), boundary scan for connectivity, memory test, then peripherals one by one.
5. Keep a bring-up log by serial number; every rework wire becomes an ECO candidate.

## Phase gates
| Gate | Build | Proves | Typical exit |
|---|---|---|---|
| EVT | engineering prototypes | design meets spec | functions, power, SI, EMC pre-scan, thermal four-corner pass; issues ranked |
| DVT | production-intent design and tooling | all specs, environment, reliability, certification | compliance tests passed, reliability tests done, design frozen |
| PVT | production line and process | process capability and yield | yield target met, Cpk >= 1.33 on key parameters, test coverage, line ready |

## Test plan structure
Requirement ID -> test case -> method -> equipment -> sample size -> pass/fail criterion -> result -> evidence file. Cover four corners (V_min/V_max x T_min/T_max) and margining. Environmental tests from IEC 60068-2: -1 cold, -2 dry heat, -14 change of temperature, -30 and -78 damp heat, -6 sine and -64 random vibration, -27 shock.

## HALT and HASS
- HALT (design stage, test to fail): cold and hot steps of about 10 degC with dwell, rapid thermal transitions (chamber 50 degC/min or more), stepped six-axis random vibration, then combined stress; power cycling and voltage margins. Record operating and destruct limits, fix root causes.
- HASS (production screen): stresses inside the HALT operating limits; run a proof of screen on samples to show it finds defects without using up significant life.

## Reliability math
- Constant failure rate: MTBF = 1 / lambda; R(t) = exp(-lambda * t); FIT = failures per 1e9 device-hours, so MTBF (h) = 1e9 / FIT. Series system: lambda_sys = sum of lambda_i.
- Arrhenius AF = exp[(Ea / k) * (1/T_use - 1/T_stress)], k = 8.617e-5 eV/K, temperatures in kelvin. Ea = 0.7 eV, 55 -> 125 degC gives AF about 78. State the Ea assumed.
- Thermal cycling (Coffin-Manson): AF = (dT_test / dT_use)^n, n around 2 for solder joints; state n.
- Demonstration test: device-hours T = MTBF_target * chi2(CL; 2r + 2) / 2 for r allowed failures. Zero failures: T = -ln(1 - CL) * MTBF_target, so 2.3 x MTBF at 90 %; r = 1 needs 3.89 x, r = 2 needs 5.32 x.
- Prediction handbooks (Telcordia SR-332, IEC 61709, MIL-HDBK-217F) give estimates, not lifetimes; name the method and conditions.

## Failure analysis
Contain, describe, reproduce, then isolate non-destructively (X-ray, acoustic microscopy, curve trace, thermal imaging) before destructive steps (decap, cross-section, SEM/EDS). Classify: EOS, ESD, workmanship, component defect, design margin, wear-out. Use 8D or 5 Whys; verify the fix on new units and close the loop in the design rules.

## Output
Bring-up report or validation plan: scope, units and serials, setup, results vs criteria table, failures with status, risks, gate recommendation (go, go with actions, no-go).

## Pitfalls
MTBF read as lifetime; first power without a current limit; HASS beyond operating limits; untraceable samples; several changes between builds; skipping retest after an ECO.
