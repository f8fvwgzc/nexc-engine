---
id: perf-analytics-and-tracking
name: Analytics and tracking
category: marketing
description: Designs and audits marketing measurement: GA4 event model and key events, UTM governance, consent mode v2, server-side tagging and enhanced conversions, with a tracking plan and QA checklist.
keywords: ga4, google analytics 4, event tracking, key events, conversions, utm, campaign tagging, consent mode, consent mode v2, server-side tagging, sgtm, google tag manager, enhanced conversions, tracking plan, data layer, measurement plan
source: Google Analytics Help - About key events | https://support.google.com/analytics/answer/13128484
source: Google Analytics Help - Collect campaign data with custom URLs (UTM) | https://support.google.com/analytics/answer/10917952
source: Google Tag Platform - Consent mode implementation guide | https://developers.google.com/tag-platform/security/guides/consent
source: Google Tag Platform - Server-side tagging introduction | https://developers.google.com/tag-platform/tag-manager/server-side/intro
source: Google Ads Help - About enhanced conversions | https://support.google.com/google-ads/answer/9888656
---
## When to use
Writing a measurement or tracking plan, auditing GA4 or tag setups, fixing campaign attribution, or making tracking consent-compliant.

## GA4 event model
- Every interaction is an event with parameters; user properties describe the user. Event types: automatically collected, enhanced measurement, recommended (use Google's exact names: purchase, generate_lead, sign_up, add_to_cart, begin_checkout), custom (only when nothing fits; register custom dimensions to report them).
- Key events (formerly called conversions in GA4): star an event in Admin > Events; up to 30 per standard property, 50 on 360. Google Ads conversions are separate objects; they can be created from key events.
- Limits: event and parameter names 40 chars; 25 parameters per event; parameter values 100 chars (page_location 1,000); 25 user properties; distinct event names unlimited on web streams, 500 per app user on app streams.
- Ecommerce: items array, currency, value and a unique transaction_id for deduplication.
- Attribution settings: reporting model (data-driven by default) and lookback windows in Admin. Event data retention (2 or 14 months on standard) limits explorations and funnels only; set 14 early, deleted data never returns.

## Tracking plan
Table: event | trigger | parameters (type, example) | key event | destinations (GA4, Ads, Meta CAPI) | consent category | owner. Give developers a data layer spec; snake_case verb_noun names; version the plan.

## UTM governance
- Always utm_source, utm_medium, utm_campaign; add utm_id and utm_source_platform; utm_term and utm_content as needed. utm_creative_format and utm_marketing_tactic are not currently reported.
- Values are case-sensitive: lowercase and a controlled vocabulary; one source value per platform, one medium per channel (cpc, paid_social, email, affiliate) so default channel grouping works.
- Never tag internal links. Keep Google Ads auto-tagging on. Audit "(not set)" and Unassigned traffic monthly.

## Consent mode v2
- Signals: ad_storage, analytics_storage, ad_user_data, ad_personalization (the last two added in November 2023).
- Set a default before any config or event command, update when the user acts in the CMP, persist the choice; region-specific defaults with ISO 3166-2 codes; wait_for_update gives an async CMP time. In GTM use the consent APIs.
- Basic mode blocks tags until the user interacts with the banner. Advanced mode loads tags with denied defaults and sends cookieless pings while denied. Basic gets general conversion modelling, advanced advertiser-specific modelling.
- EEA traffic must carry the v2 signals to keep Google ads personalisation and measurement (EU user consent policy). Choosing defaults and the lawful basis is a legal decision.

## Server-side tagging
Browser sends to a server container on a first-party subdomain; clients claim requests and turn them into events; tags forward to vendors. Gains: control and redaction of data, fewer third-party scripts. Costs: hosting and operations. It never removes the need for consent.

## Enhanced conversions
Send SHA-256 hex hashes of email, phone (E.164), name and address, normalised first (trim, lowercase, drop dots before @gmail.com or @googlemail.com). Web version for on-site conversions; leads version for offline sales imported later.

## QA checklist
Tag Assistant and DebugView; one event per action; transaction_id dedupe; GA4 vs backend orders reconciled; consent states tested per region; cross-domain and payment-referral exclusions; staging excluded.

## Pitfalls
- UTM on internal banners resetting sessions.
- Too many key events diluting bidding signals.
- Expecting GA4 key events and Google Ads conversions to match (different counting and attribution).
