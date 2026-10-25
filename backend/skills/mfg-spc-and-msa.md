---
id: mfg-spc-and-msa
name: SPC and measurement systems
category: manufacturing
description: Builds and reads control charts, run rules and capability indices, and qualifies gauges with Gage R&R, bias, linearity and attribute studies following AIAG MSA and the AIAG & VDA SPC manual.
keywords: spc, control chart, xbar-r, i-mr, p chart, u chart, western electric rules, cp, cpk, ppk, process capability, msa, gage r&r, ndc, measurement system analysis, attribute agreement
source: NIST/SEMATECH e-Handbook - Process capability | https://www.itl.nist.gov/div898/handbook/pmc/section1/pmc16.htm
source: NIST/SEMATECH e-Handbook - Shewhart control charts | https://www.itl.nist.gov/div898/handbook/pmc/section3/pmc32.htm
source: AIAG - Quality core tools | https://www.aiag.org/expertise-areas/quality/quality-core-tools
source: AIAG & VDA SPC Manual | https://www.aiag.org/training-and-resources/manuals/details/SPCAV-1
---
## References
AIAG MSA 4th edition (2010). The AIAG & VDA SPC Manual, 1st edition (July 2026), supersedes AIAG SPC 2nd edition (2005). Customer-specific requirements decide which applies.

## Chart choice
| Data | Subgroup | Chart |
|---|---|---|
| variable | 1 | I-MR |
| variable | 2-9 | Xbar-R |
| variable | 10+ | Xbar-S |
| defectives | varies | p (np if n constant) |
| defects per unit | varies | u (c if area constant) |

## Formulas
- Xbar-R: limits = Xbarbar +/- A2*Rbar; UCL_R = D4*Rbar, LCL_R = D3*Rbar; sigma_within = Rbar/d2.
- Constants (A2, D3, D4, d2): n=2: 1.880, 0, 3.267, 1.128; n=3: 1.023, 0, 2.574, 1.693; n=4: 0.729, 0, 2.282, 2.059; n=5: 0.577, 0, 2.114, 2.326.
- I-MR: Xbar +/- 2.66*MRbar; UCL_MR = 3.267*MRbar; sigma = MRbar/1.128.
- p: pbar +/- 3*sqrt(pbar(1-pbar)/n_i); u: ubar +/- 3*sqrt(ubar/n_i); floor LCL at 0.
- Phase I: 20-25 subgroups to set limits, remove only points with a found cause. Phase II: freeze limits; recompute only after a verified process change. Limits come from the process, never from the specification.

## Run rules
One point beyond 3 sigma; 2 of 3 beyond 2 sigma on one side; 4 of 5 beyond 1 sigma on one side; 8 in a row on one side; 6 in a row trending; 14 alternating. Sources differ (7, 8 or 9 points on one side): write the chosen set into the control plan. More rules mean more false alarms (in-control average run length falls from about 370 to about 92). For small sustained shifts (under about 1.5 sigma) use EWMA or CUSUM. Every signal follows the out-of-control action plan.

## Capability
- Cp = (USL - LSL)/(6*sigma_within); Cpk = min(USL - mean, mean - LSL)/(3*sigma_within). Pp and Ppk use the overall standard deviation s. Cpk <= Cp; a large gap means off-centre.
- Valid only for a stable process. Test normality; otherwise transform (Box-Cox, Johnson) or use percentiles: upper (USL - X50)/(X99.865 - X50), lower (X50 - LSL)/(X50 - X0.135), take the smaller.
- Centred Cp: 1.00 -> 0.27% out, 1.33 -> about 64 ppm, 1.67 -> 0.6 ppm, 2.00 -> 2 ppb. One-sided Z = 3*Cpk.
- Typical targets: Cpk >= 1.33 ongoing, Ppk >= 1.67 initial (check the customer). Report n and a lower confidence bound: Cpk_L ~ Cpk - z x sqrt(1/(9n) + Cpk^2/(2(n-1))); with n = 30 and Cpk 1.33 the 95% one-sided bound is only about 1.0.

## MSA
- Resolution at least one tenth of the tolerance or process spread.
- Stability, bias (t-test on reference part) and linearity across the range before R&R.
- Crossed Gage R&R: 10 parts covering the process range x 3 appraisers x 2-3 trials, randomized and blind; ANOVA method (shows appraiser x part interaction).
- %GRR = 100*sigma_GRR/sigma_total (study variation) or 100*6*sigma_GRR/tolerance (% tolerance; MSA 4th edition uses 6 sigma, older studies 5.15); say which. Under 10% acceptable; 10-30% may be acceptable by application and cost; over 30% not acceptable. ndc = 1.41*sigma_parts/sigma_GRR, truncated to an integer, needs >= 5.
- High repeatability (EV) points to the gauge or fixture; high reproducibility (AV) points to method or training.
- Attribute studies: kappa > 0.75 good, < 0.40 poor; effectiveness >= 90%, miss rate <= 2%, false alarm <= 5%.

## Pitfalls
Spec limits on control charts; capability from unstable data; Ppk sold as Cpk; R&R with parts outside the real range; ignoring ndc.
