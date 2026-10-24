---
id: sec-identity-and-zero-trust
name: Identity and zero trust
category: security
description: Designs identity and access controls and a zero trust roadmap per NIST SP 800-207 and SP 800-63-4, covering phishing-resistant MFA, least privilege, privileged access and joiner-mover-leaver.
keywords: zero trust, iam, identity, mfa, phishing-resistant, fido2, passkeys, webauthn, pam, privileged access, least privilege, joiner mover leaver, access review, nist 800-207, nist 800-63, conditional access
source: NIST - SP 800-207 Zero Trust Architecture | https://csrc.nist.gov/pubs/sp/800/207/final
source: NIST - SP 800-63-4 Digital Identity Guidelines | https://csrc.nist.gov/pubs/sp/800/63/4/final
source: NIST - SP 800-63B-4 Authentication and Authenticator Management | https://pages.nist.gov/800-63-4/sp800-63b.html
source: CISA - Zero Trust Maturity Model | https://www.cisa.gov/zero-trust-maturity-model
source: NIST NCCoE - SP 1800-35 Implementing a Zero Trust Architecture | https://pages.nist.gov/zero-trust-architecture/
---
## When to use
IAM design, MFA rollout, zero trust roadmap, privileged access, access reviews, identity lifecycle, non-human and AI-agent identities.

## Key standards
- **NIST SP 800-207** (Aug 2020): no implicit trust from network location or ownership; access granted per session after authentication and authorisation; policy is dynamic (identity, device posture, behaviour, environment); all assets monitored. Components: policy engine (decides), policy administrator (opens or closes the session), policy enforcement point (gate). Approaches: enhanced identity governance, micro-segmentation, software-defined perimeter. SP 1800-35 (June 2025): 19 example builds.
- **CISA ZTMM v2.0**: pillars Identity, Devices, Networks, Applications and Workloads, Data; cross-cutting Visibility and Analytics, Automation and Orchestration, Governance; stages Traditional, Initial, Advanced, Optimal.
- **NIST SP 800-63-4** (July 2025, replaces 63-3): IAL, AAL, FAL. AAL2 verifiers must offer a phishing-resistant option; AAL3 requires a phishing-resistant authenticator with a non-exportable key, so syncable passkeys fit AAL2, not AAL3. Passwords: at least 15 characters when single-factor, 8 when part of MFA; accept at least 64; no composition rules; no forced periodic change (change on evidence of compromise); screen against a blocklist. Reauthentication: AAL2 at most 24 h overall / 1 h idle; AAL3 12 h overall / 15 min idle. PSTN (SMS, voice) is a restricted authenticator.

## Method
1. Inventory identities: workforce, customers, service accounts, workloads, API keys, AI agents. HR is the workforce source of truth.
2. Authenticate: FIDO2/WebAuthn passkeys or PIV smart cards first for admins, remote access, email and cloud consoles, then everyone. TOTP, push and SMS are not phishing-resistant; push needs number matching against fatigue attacks. Block legacy protocols that bypass MFA.
3. Authorise: RBAC for birthright access, ABAC or policy rules for context; least privilege; SoD matrix of toxic combinations.
4. PAM: vault and rotate privileged secrets, just-in-time elevation with approval and expiry, session recording, separate admin accounts and privileged access workstations. At least two cloud-only break-glass accounts: phishing-resistant keys unlike daily admin methods, kept in separate safes, excluded from conditional access rules that could block them, alert on every sign-in, test at least every 90 days.
5. Joiner-mover-leaver: joiner gets role access from the start date; mover loses old access when the new role is granted; leaver disabled at termination (at once for hostile exits), sessions and tokens revoked, devices recovered.
6. Reviews: privileged access quarterly, other access at least yearly by manager or data owner; remove unattested access.
7. Non-human identities: short-lived federated tokens (OIDC) instead of static keys, secrets manager, owner per account, no interactive logon. AI agents: own identity, narrow short-lived tokens, on-behalf-of rights never wider than the user's, human approval for high-impact actions.
8. Zero trust roadmap: define the protect surface, map flows, place enforcement points, write policy, monitor. Quick wins: MFA everywhere, device posture in conditional access, ZTNA in place of flat VPN, segment crown jewels.

## Output and metrics
ZTMM stage per pillar (current vs target), identity inventory, access model, phased MFA/PAM plan with owners. Track phishing-resistant MFA %, standing admins, privileged accounts in PAM %, orphan accounts, leaver deprovisioning time (median, p95), review completion %.

## Pitfalls
- MFA exemptions for executives or legacy apps.
- Service accounts with unrotated passwords and interactive logon.
- Zero trust bought as a product, not an architecture.
- Rubber-stamp access reviews.
