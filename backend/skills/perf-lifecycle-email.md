---
id: perf-lifecycle-email
name: Lifecycle and email marketing
category: marketing
description: Designs lifecycle and email programmes: segments and automated flows, deliverability (SPF, DKIM, DMARC, RFC 8058, Gmail and Yahoo bulk-sender rules) and anti-spam law (CAN-SPAM, GDPR/ePrivacy, Japan).
keywords: email marketing, lifecycle marketing, crm, marketing automation, drip, segmentation, deliverability, spf, dkim, dmarc, one-click unsubscribe, rfc 8058, bulk sender, can-spam, gdpr, opt-in
source: Google Workspace Admin Help - Email sender guidelines | https://support.google.com/a/answer/81126
source: Yahoo Sender Hub - Sender best practices | https://senders.yahooinc.com/best-practices/
source: FTC - CAN-SPAM Act: A Compliance Guide for Business | https://www.ftc.gov/business-guidance/resources/can-spam-act-compliance-guide-business
source: ICO - Electronic mail marketing under PECR | https://ico.org.uk/for-organisations/direct-marketing-and-privacy-and-electronic-communications/guide-to-pecr/electronic-and-telephone-marketing/electronic-mail-marketing/
source: MIC Japan - Key points of the Specified Electronic Mail Act (Japanese) | https://www.soumu.go.jp/main_sosiki/joho_tsusin/d_syohi/pdf/m_mail_pamphlet.pdf
---
## When to use
Planning CRM or lifecycle programmes, writing flows, fixing inbox placement, or checking a send against mailbox-provider rules and anti-spam law.

## Lifecycle flows
- Core flows: welcome and onboarding (3-5 messages over 7-14 days, one action each); activation nudges on missed milestones; browse and cart abandonment (first within hours); post-purchase (how-to, review, cross-sell); replenishment; win-back, then suppression; transactional.
- Segment by lifecycle stage, RFM (recency, frequency, monetary value), engagement recency, stated preferences and predicted churn.
- Measure with a holdout group (5-10 %): revenue per recipient, conversion, unsubscribe and complaint rates. Apple Mail Privacy Protection inflates opens.

## Deliverability
- SPF (at most 10 DNS lookups), DKIM (1024-bit minimum, 2048 recommended), DMARC (bulk senders at least p=none, then quarantine or reject, with rua reports); From domain aligned with the SPF or DKIM domain; valid forward/reverse DNS; TLS; RFC 5322 formatting.
- Gmail (since 1 Feb 2024; enforcement stepped up from November 2025 with temporary and permanent rejections): bulk sender = close to 5,000+ messages in 24 hours to personal Gmail accounts per primary domain, a status that is permanent once reached. Needs all of the authentication above; Postmaster spam rate under 0.10 % and never 0.30 % or more; one-click unsubscribe plus a visible body link on marketing mail; honour within 48 hours.
- Yahoo (since Feb 2024): same authentication, DMARC must pass, complaints under 0.3 %, one-click unsubscribe (RFC 8058 POST preferred), honour within 2 days.
- RFC 8058 (Jan 2017): List-Unsubscribe with an HTTPS URI plus List-Unsubscribe-Post: List-Unsubscribe=One-Click; DKIM must cover both headers; endpoint takes a POST with no cookies or auth and must not redirect.
- Hygiene: confirmed opt-in for risky sources, drop hard bounces at once, sunset contacts unengaged 90-180 days, warm new IPs and domains gradually, split marketing and transactional subdomains, never buy lists.

## Law (general information; confirm with counsel for the jurisdiction)
- US CAN-SPAM: accurate headers, non-deceptive subject, ad disclosure, valid postal address, clear opt-out honoured within 10 business days and working at least 30 days after sending, no fee. Advertiser and sender can both be liable; the FTC guide cites up to USD 53,088 per violating email (inflation-indexed). The primary-purpose test separates commercial from transactional mail.
- EU and UK: ePrivacy Directive Art. 13 and UK PECR reg. 22 need prior consent from individuals; soft opt-in only for existing customers and similar products, with an opt-out at collection and in each message. Consent must meet GDPR Art. 4(11) and Art. 7 and be recorded. ICO PECR guidance is under review after the Data (Use and Access) Act 2025.
- Japan: Act on Regulation of Transmission of Specified Electronic Mail (Act No. 26 of 2002; opt-in since 2008; SMS included). Prior consent, except narrow cases such as an address handed over on a business card. Record when and how consent was given, kept 1 month after the last ad mail. Show sender name, the right to refuse and an opt-out address or URL; nothing after refusal. Breaches bring MIC and Consumer Affairs Agency orders; falsified sender data or defying an order: up to 1 year prison or JPY 1 million, companies up to JPY 30 million. Mail-order sellers are also bound by the Act on Specified Commercial Transactions (records kept 3 years).

## Output
Flow map (trigger, delay, message, exit), segment definitions, KPI targets with holdout design, authentication and compliance checklist, risks.

## Pitfalls
- Unsubscribe hidden or behind a login; marketing in transactional mail.
- One domain for everything; sudden volume spikes.
