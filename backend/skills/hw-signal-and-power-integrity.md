---
id: hw-signal-and-power-integrity
name: Signal and power integrity
category: hardware
description: Budgets and reviews high-speed interconnects and power delivery: controlled impedance, reflections and termination, crosstalk, decoupling and PDN target impedance, thermal design and derating.
keywords: signal integrity, power integrity, si, pi, impedance, termination, reflection, crosstalk, decoupling, pdn, target impedance, ddr, serdes, thermal, junction temperature, derating
source: Wikipedia - Signal integrity | https://en.wikipedia.org/wiki/Signal_integrity
source: Wikipedia - Power integrity (target impedance) | https://en.wikipedia.org/wiki/Power_integrity
source: Texas Instruments SPRA953D (rev. March 2024) - Semiconductor and IC Package Thermal Metrics | https://www.ti.com/lit/pdf/spra953
source: NASA GSFC EEE-INST-002 (2003, Addendum 1 2008) - parts selection and derating | https://nepp.nasa.gov/docuploads/FFB52B88-36AE-4378-A05B2C084B5EE2CC/EEE-INST-002_add1.pdf
---
## When to use
DDR, PCIe, USB, Ethernet, MIPI or any edge under about 1 ns; PDN design for FPGAs and SoCs; junction-temperature and derating checks.

## SI formulas
- Bandwidth of an edge: BW = 0.35 / t_r (10-90 %).
- Delay t_pd = sqrt(e_eff) / c. FR-4 stripline (e_r about 4): about 6.7 ps/mm (170 ps/in); microstrip is faster.
- Treat a net as a transmission line when one-way delay > t_r / 6 (critical length L = t_r / (6 * t_pd)).
- Reflection coefficient G = (Z_L - Z_0) / (Z_L + Z_0); 50 ohm into 75 ohm gives 0.2.
- Microstrip estimate (IPC-2141): Z_0 = 87 / sqrt(e_r + 1.41) * ln(5.98h / (0.8w + t)), roughly valid for 0.1 < w/h < 2; use a field solver for release; fab tolerance is often +/-10 %. Differential Z_diff = 2 * Z_odd; common targets 85, 90, 100 ohm.

## Termination and topology
- Series at the source: R_s = Z_0 - R_driver, for point-to-point. Parallel to V_TT at the load (DDR fly-by), Thevenin, or AC (RC); on-die termination where the part offers it; 100 ohm across a differential receiver.
- Keep stubs short (delay < t_r / 10); back-drill or blind vias above roughly 10 Gb/s.
- Loss: Nyquist f_N = bit rate / 2 for NRZ, / 4 for PAM4. Compare insertion loss at f_N with the protocol channel budget; if over, use a lower-Df laminate, smoother copper, shorter route or a redriver/retimer.

## Crosstalk
- Coupling falls about as 1 / (1 + (s/h)^2): s = spacing, h = height above plane. Lower h before adding spacing.
- 3W rule (centre spacing 3 x width) for ordinary nets; more for clocks. NEXT saturates with coupled length; FEXT grows with length and is near zero in homogeneous stripline.
- Route adjacent signal layers orthogonally; guard traces only if stitched to ground; length-match to the protocol skew budget.

## Power integrity
- Target impedance Z_t = V_rail * ripple fraction / dI (load step). Example: 0.9 V, 3 %, 10 A gives 2.7 mOhm.
- Keep |Z_PDN(f)| below Z_t from DC to the frequency the package and die take over: VRM (DC to tens of kHz), bulk caps, MLCCs (to tens of MHz), plane capacitance and package caps (about 100 MHz up).
- Capacitor self-resonance f = 1 / (2 * pi * sqrt(L * C)); mounted inductance (pads, vias, spreading) dominates, so use short wide connections, via-in-pad, several vias, thin power-ground dielectric.
- Watch anti-resonance peaks between cap values and from ferrite beads on analog rails; damp them and simulate.
- DC: budget IR drop (often 1-2 % of rail), use remote sense; check MLCC DC-bias loss on vendor curves.

## Thermal and derating
- R_thetaJA is a JEDEC JESD51 board comparison, not a design value. Estimate with T_j = T_top + Psi_JT * P or T_j = T_board + Psi_JB * P, or a thermal model.
- Altitude reduces convection; give exposed pads copper and thermal vias.
- Space-grade derating example (NASA EEE-INST-002): ceramic caps 0.6 of rated voltage, solid tantalum 0.5 at 70 degC and 0.3 at 110 degC, IC supply 0.9 (digital) and 0.8 (linear), T_j <= 110 degC for ICs and 125 degC for discretes, or 40 degC below T_jmax if lower. Commercial policies are lighter; name the policy used.

## Output
Per interface: budget (impedance, loss, skew, ripple), stack-up and impedance table, models used (IBIS, S-parameters), results vs limits, violations, fixes and how each is verified (TDR, VNA, scope with short ground).

## Pitfalls
- Plane splits under fast nets; ignoring via and connector discontinuities.
- Using R_thetaJA to predict T_j; ignoring MLCC DC bias.
- Simulating with generic models and calling it signed off.
