---
id: appsec-security-code-review
name: Security code review
category: appsec
description: Reviews source code and pull requests for injection, broken access control and IDOR, SSRF, insecure deserialisation, crypto misuse and authentication or session flaws, and reports verified findings with fixes.
keywords: security code review, secure code review, injection, sql injection, xss, idor, broken access control, ssrf, deserialization, crypto misuse, authentication, session management, jwt, cwe, sast triage
source: OWASP Cheat Sheet Series - SSRF Prevention | https://cheatsheetseries.owasp.org/cheatsheets/Server_Side_Request_Forgery_Prevention_Cheat_Sheet.html
source: OWASP Cheat Sheet Series - IDOR Prevention | https://cheatsheetseries.owasp.org/cheatsheets/Insecure_Direct_Object_Reference_Prevention_Cheat_Sheet.html
source: OWASP Cheat Sheet Series - Password Storage | https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html
source: MITRE - 2025 CWE Top 25 Most Dangerous Software Weaknesses | https://cwe.mitre.org/top25/archive/2025/2025_cwe_top25.html
---
## When to use
Reviewing a diff, pull request, module or codebase for vulnerabilities; triaging SAST output; writing a review report.

## Method
1. Scope: what changed, trust boundaries crossed, data classification, auth model, framework and version.
2. List entry points (routes, handlers, consumers, CLI, file parsers) and sinks (SQL/NoSQL, shell, file paths, templates, HTTP clients, deserialisers, crypto, redirects, logs).
3. Trace each untrusted source to its sinks: validation (allow-list, type, length) on input, context-correct encoding or parameterisation at the sink.
4. Check authorisation server-side on every function and object, deny by default; read unchanged callers too.
5. Use scanners as hints; confirm or refute each by reading code.
6. Rate, write the finding, give a fix and a regression test.

## What to look for
- Injection: string-built SQL, NoSQL operators from JSON, shell calls, LDAP, XPath, server-side templates, CRLF in headers. Fix: parameterised queries, argument arrays with no shell, safe template APIs. XSS sinks: innerHTML, dangerouslySetInnerHTML, unescaped template filters; add CSP. (2025 CWE Top 25 starts CWE-79, CWE-89, CWE-352, CWE-862.)
- Access control / IDOR (CWE-639, CWE-862): records fetched by client-supplied id without owner or tenant scope (use current_user.records.find(id)); missing checks on admin or export routes; mass assignment of role or owner fields; role claims trusted from the client. Unguessable UUIDs are not authorisation; return the same 404 for missing and forbidden when existence is sensitive.
- SSRF (CWE-918): server fetches user-influenced URLs (webhooks, previews, PDF rendering). Prefer a destination allow-list; otherwise resolve once, reject loopback, RFC1918, 169.254.0.0/16 (cloud metadata), IPv6 ULA and link-local, connect to the vetted IP, disable redirects, restrict egress, use IMDSv2. Parsers can disagree on the host: reject ambiguous URLs.
- Deserialisation (CWE-502): native object formats on untrusted data (Java ObjectInputStream, .NET BinaryFormatter, Python pickle, PHP unserialize, unsafe YAML load). Use data-only formats with schema validation, type allow-lists, signed payloads.
- Crypto misuse: MD5/SHA-1 for security, ECB, reused GCM nonce, static IV, home-made schemes, non-CSPRNG tokens, hard-coded keys, TLS verification off, non-constant-time MAC compare. Passwords: Argon2id, scrypt, bcrypt or PBKDF2 with current parameters.
- CSRF (CWE-352): state-changing requests accepted on cookie auth with no anti-CSRF token, SameSite cookie or Origin check; GET handlers that change state.
- AuthN and sessions: no rate limit, user enumeration, session id not rotated at login, JWT alg none or signature unchecked, missing aud/iss/exp checks, long-lived tokens without revocation, cookies without Secure, HttpOnly, SameSite, reusable reset tokens, MFA bypass routes.
- Also: path traversal (CWE-22), unrestricted file upload (CWE-434: type, size, storage outside web root), XXE, open redirect, race conditions (TOCTOU, double spend), fail-open error handling, stack traces or secrets in responses and logs.

## Finding format
Title; CWE; location (file:line, commit); description; attack scenario and preconditions; evidence (minimal excerpt, secrets redacted); impact and likelihood; severity (CVSS v4.0 vector or the org matrix); fix with code; test that proves it; references (ASVS v5.0.0 ID, cheat sheet). Report opens with scope, method, counts by severity, top risks and what was not reviewed.

## Pitfalls
Reviewing only the diff; trusting client-side checks; forwarding scanner output unverified; severity inflation; no reproduction path; deny-list fixes.
