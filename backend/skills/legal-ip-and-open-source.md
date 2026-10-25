---
id: legal-ip-and-open-source
name: IP and open-source licensing
category: legal
description: Identifies the IP rights in a product or deal and audits open-source licence obligations, compatibility and contributor terms before code is shipped, distributed or run as a service.
keywords: intellectual property, copyright, patent, trademark, trade secret, open source, oss licence, license compliance, mit, apache-2.0, gpl, lgpl, agpl, copyleft, sbom, cla
source: Apache License, Version 2.0 | https://www.apache.org/licenses/LICENSE-2.0
source: Free Software Foundation - Various licenses and comments about them | https://www.gnu.org/licenses/license-list.html
source: Apache Software Foundation - Third-party license policy (categories A, B, X) | https://www.apache.org/legal/resolved.html
source: Mozilla - MPL 2.0 FAQ | https://www.mozilla.org/en-US/MPL/2.0/FAQ/
source: Apache Software Foundation - GPL compatibility | https://www.apache.org/licenses/GPL-compatibility.html
---
## When to use
Releases and distribution, M&A or investment diligence, inbound or outbound open source, choosing a project licence, contributor terms, IP clauses in deals.

## IP basics
| Right | Protects | Arises | Watch |
|---|---|---|---|
| Copyright | expression: code, docs, UI text | on creation | contractor work needs a written assignment; employee work usually goes to the employer (check local law) |
| Patent | novel, inventive, industrially applicable inventions | grant after filing, about 20 years from filing | public disclosure before filing destroys novelty in Europe; US and Japan allow a 1-year grace period |
| Trademark | brand signs | registration (some use-based rights) | clearance per class, renewals, genericide |
| Trade secret | valuable confidential information | reasonable secrecy steps | NDAs, access control, exit reminders |

## OSS obligations
- Permissive: MIT, BSD-2/3-Clause, ISC: keep the copyright and licence notice.
- Apache-2.0: give a licence copy, mark changed files, keep notices, carry NOTICE attributions (s4); contributor patent licence that ends for anyone who sues claiming the work infringes (s3); no trademark rights (s6).
- Weak copyleft: LGPL: changes to the library stay LGPL and users must be able to relink or replace it; MPL-2.0: file-level copyleft; when shipping executables, make the source of the MPL files (with changes) available to recipients.
- Strong copyleft: GPL-2.0/3.0: distributing object code of a derivative or combined work requires the complete corresponding source under the same licence; GPLv3 adds installation information for consumer devices and a patent licence. AGPL-3.0 s13: users who interact with a modified version over a network must be offered its source, so SaaS triggers it.
- Triggers: GPL on distribution; AGPL also on network use of modified code. Purely internal use generally triggers neither, but copies given to another legal entity (affiliate, customer, devices you sell) usually count as distribution; GPLv3 s2 exempts contractors working exclusively for you.
- Source-available, not OSI open source: SSPL, BUSL-1.1, Commons Clause add field-of-use limits; treat as commercial licences.

## Compatibility (FSF list)
- Apache-2.0 into GPLv3: yes; into GPLv2-only: no.
- MIT, BSD-3 into any GPL: yes; original 4-clause BSD (advertising clause): no.
- LGPLv2.1 with GPLv2 or v3: yes; LGPLv3 with GPLv2-only: no.
- MPL-2.0 with GPL: yes via its secondary-licence clause, unless marked "Incompatible With Secondary Licenses".
- EPL-2.0: no, unless GPL is named as a secondary licence.
- GPLv2-only with GPLv3: no.

## Method
1. Inventory with an SBOM (SPDX, ISO/IEC 5962, or CycloneDX, ECMA-424) and a licence scanner, including transitive and vendored code; compare declared and detected licences and the exact version used.
2. Classify each by use (internal, SaaS, distributed binary, device) against policy, e.g. ASF style: A allowed (MIT, BSD, Apache-2.0), B conditional (weak copyleft such as MPL, EPL, CDDL: binary only, labelled), X barred (GPL, LGPL, AGPL, SSPL, BUSL-1.1, Commons Clause).
3. Find conflicts; remediate: replace, isolate as a separate program, buy a commercial licence, or comply.
4. Ship attribution notices and the source offer where needed.
5. Inbound contributions: CLA (copyright and patent licence) or DCO sign-off; check employer IP policy before staff contribute out.

## Output
Licence table: component / version / SPDX id / use / obligation / conflict / action. IP ownership summary, risks, open questions for counsel.

## Pitfalls
No licence means all rights reserved; snippets pasted from forums; licence changes between versions; copyleft in static links or containers you ship; forgetting NOTICE files.
