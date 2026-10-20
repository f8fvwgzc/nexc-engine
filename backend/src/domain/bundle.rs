//! A run's evidence bundle (`GET /runs/{id}/evidence`): everything needed to
//! show what the run's agents did without nexc's database. It carries every
//! span of the run in chain order, the run's signed seal, what was recorded
//! of the spans (without the content), the workspace's audit entries about
//! the run and its graph as chained, and an attestation: the backend's
//! signature over a manifest of all of it. [`verify`] checks a bundle with
//! the published public keys alone (`nexc-verify`, `make verify-bundle`).

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::audit_chain::{self, ChainedEntry};
use super::recording::PayloadKind;
use super::run::RunStatus;
use super::seal::{SEAL_TYPE, SignedSeal};
use super::trace::{self, Span};
use crate::kernel;
use crate::security::signing::{self, Jwks, Signer};

/// What a bundle says it is.
pub const FORMAT: &str = "nexc-evidence-bundle/v1";

/// A run's evidence, exported.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EvidenceBundle {
    /// [`FORMAT`]: `nexc-evidence-bundle/v1`.
    pub format: String,
    pub exported_at: DateTime<Utc>,
    pub run: BundleRun,
    /// Every span of the run, in chain order.
    pub spans: Vec<Span>,
    /// What was recorded of the spans (`span_payloads`), without the content.
    pub payloads: Vec<PayloadMeta>,
    /// The run's signed seal; `null` while it runs, for a run that traced
    /// nothing, and for runs sealed before seals were signed.
    #[schema(required = true)]
    pub seal: Option<BundleSeal>,
    /// The workspace's audit entries about the run and its graph, as chained,
    /// in chain order.
    pub audit: Vec<ChainedEntry>,
    /// The backend's signature over the bundle's manifest.
    pub attestation: Attestation,
}

/// The run a bundle is about.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct BundleRun {
    pub id: Uuid,
    pub graph_id: Uuid,
    pub graph_name: String,
    /// When the graph was deleted; its runs' evidence stays until the retention purge.
    #[schema(required = true)]
    pub graph_deleted_at: Option<DateTime<Utc>>,
    #[schema(required = true)]
    pub workspace_id: Option<Uuid>,
    pub status: RunStatus,
    pub created_at: DateTime<Utc>,
    #[schema(required = true)]
    pub started_at: Option<DateTime<Utc>>,
    #[schema(required = true)]
    pub finished_at: Option<DateTime<Utc>>,
    /// Hex hash sealed when the run finished.
    #[schema(required = true)]
    pub trace_hash: Option<String>,
}

/// One recorded payload of a span, without its content.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PayloadMeta {
    pub span_id: Uuid,
    pub kind: PayloadKind,
    /// Bytes of the content as kept.
    pub size: i64,
    pub redactions: i64,
    pub created_at: DateTime<Utc>,
}

/// A run's seal as a bundle carries it.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct BundleSeal {
    /// The seal statement exactly as signed (canonical JSON, `nexc.run_seal.v1`).
    pub statement: String,
    /// Hex Ed25519 signature of `statement`.
    pub signature: String,
    pub kid: String,
}

/// The backend's signature over a bundle's [`Manifest`].
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Attestation {
    /// The manifest exactly as signed (canonical JSON).
    pub manifest: String,
    /// Hex Ed25519 signature of `manifest`.
    pub signature: String,
    pub kid: String,
}

/// What an attestation signs: the bundle's parts by their hashes. Fields are
/// declared in sorted order, so serialising gives canonical JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Each audit entry by id, position, hash and the digest of all of it, in the bundle's order.
    pub audit: Vec<ManifestEntry>,
    /// How many entries of the log were about the run and its graph when it
    /// was exported; the bundle carries all of them or, past
    /// `BUNDLE_AUDIT_MAX`, the seal's, the deletion's and the newest.
    pub audit_matching: i64,
    /// Hex SHA-256 of the canonical JSON (keys sorted, no whitespace) of `[run, payloads]`.
    pub body_sha256: String,
    /// Hex hash of the last span, `null` when there is none.
    pub chain_hash: Option<String>,
    pub exported_at: String,
    pub format: String,
    pub kid: String,
    pub run_id: Uuid,
    /// Hex signature of the seal, `null` when the bundle has none.
    pub seal_signature: Option<String>,
    pub spans: i64,
    pub workspace_id: Option<Uuid>,
}

/// One audit entry of a [`Manifest`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestEntry {
    pub hash: String,
    pub id: Uuid,
    /// Hex SHA-256 of the entry's canonical JSON: its names and salt too,
    /// which its chain hash covers only as digests.
    pub json_sha256: String,
    pub seq: i64,
}

/// Hex SHA-256 of `value` as canonical JSON: object keys sorted, no
/// whitespace, values as the bundle carries them. Any JSON library that
/// sorts keys reproduces it from the bundle's text.
fn canonical_sha256<T: Serialize>(value: &T) -> String {
    let value = serde_json::to_value(value).expect("a bundle part serialises");
    let text = serde_json::to_vec(&value).expect("a JSON value serialises");
    hex::encode(kernel::sha256(&text))
}

fn body_sha256(run: &BundleRun, payloads: &[PayloadMeta]) -> String {
    canonical_sha256(&(run, payloads))
}

fn timestamp(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Micros, true)
}

fn manifest(bundle_parts: &Parts<'_>, kid: &str) -> Manifest {
    Manifest {
        audit: bundle_parts
            .audit
            .iter()
            .map(|e| ManifestEntry {
                hash: hex::encode(e.hash),
                id: e.id,
                json_sha256: canonical_sha256(e),
                seq: e.seq,
            })
            .collect(),
        audit_matching: bundle_parts.audit_matching,
        body_sha256: body_sha256(bundle_parts.run, bundle_parts.payloads),
        chain_hash: bundle_parts.spans.last().map(|s| hex::encode(s.hash)),
        exported_at: timestamp(bundle_parts.exported_at),
        format: FORMAT.to_owned(),
        kid: kid.to_owned(),
        run_id: bundle_parts.run.id,
        seal_signature: bundle_parts.seal.map(|s| s.signature.clone()),
        spans: bundle_parts.spans.len() as i64,
        workspace_id: bundle_parts.run.workspace_id,
    }
}

struct Parts<'a> {
    exported_at: DateTime<Utc>,
    audit_matching: i64,
    run: &'a BundleRun,
    spans: &'a [Span],
    payloads: &'a [PayloadMeta],
    seal: Option<&'a BundleSeal>,
    audit: &'a [ChainedEntry],
}

impl EvidenceBundle {
    /// Puts a bundle together and signs its manifest with `signer`.
    /// `audit_matching` is how many entries of the log were about the run
    /// (at least `audit.len()`).
    pub fn assemble(
        run: BundleRun,
        spans: Vec<Span>,
        payloads: Vec<PayloadMeta>,
        seal: Option<&SignedSeal>,
        audit: Vec<ChainedEntry>,
        audit_matching: i64,
        signer: &Signer,
    ) -> Self {
        // Microseconds, as the manifest writes it, so the two always agree.
        let exported_at = trace::truncate_micros(Utc::now());
        let seal = seal.map(|s| BundleSeal {
            statement: String::from_utf8_lossy(&s.signed).into_owned(),
            signature: hex::encode(&s.signature),
            kid: s.kid.clone(),
        });
        let parts = Parts {
            exported_at,
            audit_matching: audit_matching.max(audit.len() as i64),
            run: &run,
            spans: &spans,
            payloads: &payloads,
            seal: seal.as_ref(),
            audit: &audit,
        };
        let manifest =
            serde_json::to_string(&manifest(&parts, signer.kid())).expect("a manifest serialises");
        let attestation = Attestation {
            signature: hex::encode(signer.sign(manifest.as_bytes())),
            manifest,
            kid: signer.kid().to_owned(),
        };
        EvidenceBundle {
            format: FORMAT.to_owned(),
            exported_at,
            run,
            spans,
            payloads,
            seal,
            audit,
            attestation,
        }
    }
}

/// One check of [`verify`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Check {
    pub name: &'static str,
    pub ok: bool,
    pub detail: String,
}

/// What [`verify`] found.
#[derive(Debug, Clone, Serialize)]
pub struct BundleReport {
    /// True when every check passed.
    pub ok: bool,
    pub run_id: Uuid,
    pub checks: Vec<Check>,
}

/// Checks a bundle with `keys` (the published key set, or the one key the
/// caller trusts) and nothing else: the span chain recomputes and ends at
/// the sealed hash; the seal is signed by a known key and seals this chain;
/// every audit entry hashes as stored, adjacent entries link, and the log
/// records the seal; the attestation is signed by a known key and covers
/// the run, the spans, the seal, the payloads and every audit entry.
pub fn verify(bundle: &EvidenceBundle, keys: &Jwks) -> BundleReport {
    let mut checks = Vec::new();
    let mut check = |name: &'static str, result: Result<String, String>| {
        let (ok, detail) = match result {
            Ok(detail) => (true, detail),
            Err(detail) => (false, detail),
        };
        checks.push(Check { name, ok, detail });
    };
    let run = &bundle.run;
    check(
        "format",
        if bundle.format == FORMAT {
            Ok(FORMAT.to_owned())
        } else {
            Err(format!("`{}` is not {FORMAT}", bundle.format))
        },
    );
    check("spans", spans(bundle));
    check("seal", seal(bundle, keys));
    check("audit", audit(bundle));
    check("attestation", attestation(bundle, keys));
    BundleReport {
        ok: checks.iter().all(|c| c.ok),
        run_id: run.id,
        checks,
    }
}

fn decode32(hex_text: &str) -> Option<[u8; 32]> {
    hex::decode(hex_text).ok()?.try_into().ok()
}

fn spans(bundle: &EvidenceBundle) -> Result<String, String> {
    let run = &bundle.run;
    if let Some(stray) = bundle.spans.iter().find(|s| s.run_id != run.id) {
        return Err(format!("span {} belongs to another run", stray.id));
    }
    let sealed = match &run.trace_hash {
        Some(text) => Some(decode32(text).ok_or("the run's trace hash is not a digest")?),
        None => None,
    };
    let verified = trace::verify(&bundle.spans, sealed);
    match verified.broken {
        None if verified.ok => Ok(format!(
            "{} span(s) chain from genesis{}",
            verified.spans,
            match &verified.hash {
                Some(hash) if sealed.is_some() => format!(" to the sealed hash {hash}"),
                Some(hash) => format!(" to {hash} (the run is not sealed)"),
                None => String::new(),
            }
        )),
        Some(broken) => Err(format!(
            "span {} (seq {}): {}",
            broken.span_id, broken.seq, broken.reason
        )),
        None => Err("the chain does not end at the sealed hash".into()),
    }
}

/// The `trace_sealed` entries of the bundle that are about its run.
fn seal_entries(bundle: &EvidenceBundle) -> Vec<&ChainedEntry> {
    let run = bundle.run.id.to_string();
    bundle
        .audit
        .iter()
        .filter(|e| e.action == "trace_sealed" && e.subject == run)
        .collect()
}

/// The key the audit log says the seal was signed with, when it says so.
fn recorded_signer(entry: &ChainedEntry) -> Option<&str> {
    entry
        .detail
        .split_once(", signed by ")
        .map(|(_, kid)| kid.trim())
}

fn seal(bundle: &EvidenceBundle, keys: &Jwks) -> Result<String, String> {
    let run = &bundle.run;
    let recorded = seal_entries(bundle).into_iter().find_map(recorded_signer);
    let Some(seal) = &bundle.seal else {
        if let Some(kid) = recorded {
            return Err(format!(
                "the audit log records a seal signed by {kid}; the bundle has none"
            ));
        }
        return match &run.trace_hash {
            Some(_) => Ok("sealed without a signature (sealed before seals were signed)".into()),
            None => Ok("not sealed".into()),
        };
    };
    if let Some(kid) = recorded
        && kid != seal.kid
    {
        return Err(format!(
            "the audit log records the seal as signed by {kid}, the bundle's by {}",
            seal.kid
        ));
    }
    let signed = SignedSeal {
        signed: seal.statement.as_bytes().to_vec(),
        signature: hex::decode(&seal.signature).map_err(|_| "the signature is not hex")?,
        kid: seal.kid.clone(),
    };
    let chain = bundle
        .spans
        .last()
        .map(|last| (bundle.spans.len() as i64, last.hash));
    let checked = signed.check(run.id, chain, keys.find(&seal.kid).as_ref());
    match checked.reason {
        None => Ok(format!(
            "{SEAL_TYPE} signed by {} at {}",
            checked.kid,
            checked.sealed_at.unwrap_or_default()
        )),
        Some(reason) => Err(reason),
    }
}

fn audit(bundle: &EvidenceBundle) -> Result<String, String> {
    let run = &bundle.run;
    if bundle.audit.is_empty() {
        return match run.workspace_id {
            Some(_) => Err("the bundle has no audit entries".into()),
            None => Ok("no audit log: the graph belongs to no workspace".into()),
        };
    }
    for entry in &bundle.audit {
        if Some(entry.workspace_id) != run.workspace_id {
            return Err(format!("entry {} is of another workspace", entry.id));
        }
        if let Some(reason) = audit_chain::misfit(entry, entry.seq, &entry.prev_hash) {
            return Err(format!("entry {} (seq {}): {reason}", entry.id, entry.seq));
        }
    }
    let mut linked = 0;
    for pair in bundle.audit.windows(2) {
        if pair[1].seq <= pair[0].seq {
            return Err(format!("entry {} is out of chain order", pair[1].id));
        }
        if pair[1].seq == pair[0].seq + 1 {
            if pair[1].prev_hash != pair[0].hash {
                return Err(format!(
                    "entry {} does not link to entry {}",
                    pair[1].id, pair[0].id
                ));
            }
            linked += 1;
        }
    }
    let sealed = seal_entries(bundle);
    match &run.trace_hash {
        Some(hash) => {
            if sealed.is_empty() {
                return Err("no trace_sealed entry records the run's seal".into());
            }
            if let Some(other) = sealed.iter().find(|e| !e.detail.contains(hash.as_str())) {
                return Err(format!(
                    "trace_sealed entry {} records another hash than the run's",
                    other.id
                ));
            }
        }
        None if !sealed.is_empty() => {
            return Err("the audit log records a seal the run does not have".into());
        }
        None => {}
    }
    if run.graph_deleted_at.is_some() {
        let graph = run.graph_id.to_string();
        let recorded = bundle
            .audit
            .iter()
            .any(|e| e.action == "graph_deleted" && e.detail.contains(graph.as_str()));
        if !recorded {
            return Err("the graph is deleted and no graph_deleted entry records it".into());
        }
    }
    Ok(format!(
        "{} entr{} hash as stored ({linked} adjacent pair(s) link){}{}",
        bundle.audit.len(),
        if bundle.audit.len() == 1 { "y" } else { "ies" },
        if run.trace_hash.is_some() {
            "; trace_sealed records the seal"
        } else {
            ""
        },
        if run.graph_deleted_at.is_some() {
            "; graph_deleted records the deletion"
        } else {
            ""
        }
    ))
}

fn attestation(bundle: &EvidenceBundle, keys: &Jwks) -> Result<String, String> {
    let att = &bundle.attestation;
    let key = keys
        .find(&att.kid)
        .ok_or_else(|| format!("no published key has the id {}", att.kid))?;
    let signature = hex::decode(&att.signature).map_err(|_| "the signature is not hex")?;
    if !signing::verify(&key, att.manifest.as_bytes(), &signature) {
        return Err("the signature does not verify with the published key".into());
    }
    let signed: Manifest =
        serde_json::from_str(&att.manifest).map_err(|_| "the manifest is not readable")?;
    if signed.kid != att.kid {
        return Err("the manifest names another key".into());
    }
    let parts = Parts {
        exported_at: bundle.exported_at,
        audit_matching: signed.audit_matching,
        run: &bundle.run,
        spans: &bundle.spans,
        payloads: &bundle.payloads,
        seal: bundle.seal.as_ref(),
        audit: &bundle.audit,
    };
    let found = manifest(&parts, &att.kid);
    let differs = |what: &str| Err(format!("the bundle's {what} is not what was signed"));
    if signed.format != found.format || signed.run_id != found.run_id {
        return differs("run");
    }
    if signed.workspace_id != found.workspace_id || signed.body_sha256 != found.body_sha256 {
        return differs("run or payloads");
    }
    if signed.spans != found.spans || signed.chain_hash != found.chain_hash {
        return differs("span chain");
    }
    if signed.seal_signature != found.seal_signature {
        return differs("seal");
    }
    if signed.audit != found.audit || signed.audit_matching < found.audit.len() as i64 {
        return differs("audit entries");
    }
    if signed.exported_at != found.exported_at {
        return differs("export time");
    }
    let left_out = signed.audit_matching - signed.audit.len() as i64;
    Ok(format!(
        "signed by {} at {}: covers the run, {} span(s), the seal, {} payload(s) and {} audit entr{}{}",
        att.kid,
        signed.exported_at,
        signed.spans,
        bundle.payloads.len(),
        signed.audit.len(),
        if signed.audit.len() == 1 { "y" } else { "ies" },
        if left_out > 0 {
            format!(" ({left_out} older entries about the run left out)")
        } else {
            String::new()
        }
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::domain::trace::{GENESIS, SpanDraft, SpanKind, SpanStatus};
    use crate::security::signing::{Jwk, KeyStatus, derive_seed};

    fn bundle(signer: &Signer) -> EvidenceBundle {
        let run_id = Uuid::now_v7();
        let workspace = Uuid::now_v7();
        let mut prev = GENESIS;
        let spans: Vec<Span> = (0..5)
            .map(|i| {
                let draft = SpanDraft {
                    id: Uuid::now_v7(),
                    node_id: Uuid::now_v7(),
                    agent_id: None,
                    principal: "writer".into(),
                    parent_id: None,
                    kind: SpanKind::ToolCall,
                    name: format!("tool-{i}"),
                    started_at: Utc::now(),
                    ended_at: Some(Utc::now()),
                    status: SpanStatus::Ok,
                    attributes: json!({"bytes": i}),
                };
                let span = trace::link(draft, run_id, i, prev);
                prev = span.hash;
                span
            })
            .collect();
        let seal = SignedSeal::sign(signer, run_id, 5, &prev);
        let salt = [3u8; 16];
        let mut audit_prev = [4u8; 32];
        let audit: Vec<ChainedEntry> = [
            (
                "trace_sealed",
                run_id.to_string(),
                format!(
                    "5 span(s), sha256 {}, signed by {}",
                    hex::encode(prev),
                    signer.kid()
                ),
            ),
            (
                "evidence_exported",
                run_id.to_string(),
                "by Alice".to_owned(),
            ),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (action, subject, detail))| {
            let mut e = ChainedEntry {
                id: Uuid::now_v7(),
                workspace_id: workspace,
                seq: 40 + i as i64,
                action: action.into(),
                actor_id: None,
                actor_name: "system".into(),
                subject: subject.clone(),
                detail,
                created_at: trace::truncate_micros(Utc::now()),
                salt: Some(salt),
                actor_digest: audit_chain::text_digest(&salt, "system"),
                subject_digest: audit_chain::text_digest(&salt, &subject),
                prev_hash: audit_prev,
                hash: GENESIS,
            };
            e.hash = audit_chain::entry_hash(&audit_prev, &e);
            audit_prev = e.hash;
            e
        })
        .collect();
        let run = BundleRun {
            id: run_id,
            graph_id: Uuid::now_v7(),
            graph_name: "Quarterly report".into(),
            graph_deleted_at: None,
            workspace_id: Some(workspace),
            status: RunStatus::Succeeded,
            created_at: Utc::now(),
            started_at: Some(Utc::now()),
            finished_at: Some(Utc::now()),
            trace_hash: Some(hex::encode(prev)),
        };
        let payloads = vec![PayloadMeta {
            span_id: spans[1].id,
            kind: PayloadKind::Prompt,
            size: 120,
            redactions: 1,
            created_at: Utc::now(),
        }];
        EvidenceBundle::assemble(run, spans, payloads, Some(&seal), audit, 2, signer)
    }

    fn keys(signer: &Signer) -> Jwks {
        Jwks {
            keys: vec![Jwk::of(&signer.public_key(), KeyStatus::Current)],
        }
    }

    fn failing(report: &BundleReport) -> Vec<&'static str> {
        report
            .checks
            .iter()
            .filter(|c| !c.ok)
            .map(|c| c.name)
            .collect()
    }

    /// The bundle as JSON, changed by `edit`, read back.
    fn tampered(
        original: &EvidenceBundle,
        edit: impl FnOnce(&mut serde_json::Value),
    ) -> EvidenceBundle {
        let mut value = serde_json::to_value(original).unwrap();
        edit(&mut value);
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn a_bundle_verifies_with_the_public_key_alone_and_survives_json() {
        let signer = Signer::from_seed(&derive_seed(b"bundle")).unwrap();
        let original = bundle(&signer);
        let text = serde_json::to_string_pretty(&original).unwrap();
        let read: EvidenceBundle = serde_json::from_str(&text).unwrap();
        let report = verify(&read, &keys(&signer));
        assert!(report.ok, "{report:#?}");
        let other = Signer::from_seed(&derive_seed(b"other")).unwrap();
        let report = verify(&read, &keys(&other));
        assert_eq!(failing(&report), ["seal", "attestation"]);
    }

    #[test]
    fn tampering_with_a_span_a_seal_or_an_audit_entry_is_found() {
        let signer = Signer::from_seed(&derive_seed(b"bundle")).unwrap();
        let keys = keys(&signer);
        let original = bundle(&signer);

        // A span's content.
        let b = tampered(&original, |v| v["spans"][2]["name"] = json!("rm -rf"));
        assert_eq!(failing(&verify(&b, &keys)), ["spans"]);
        // A span removed from the end, with the run's hash moved to match.
        let b = tampered(&original, |v| {
            let spans = v["spans"].as_array_mut().unwrap();
            spans.pop();
            let last = spans.last().unwrap()["hash"].clone();
            v["run"]["trace_hash"] = last;
        });
        let failed = failing(&verify(&b, &keys));
        assert!(
            failed.contains(&"seal") && failed.contains(&"attestation"),
            "{failed:?}"
        );

        // The seal's statement, or the seal dropped.
        let b = tampered(&original, |v| {
            let statement = v["seal"]["statement"]
                .as_str()
                .unwrap()
                .replace("\"spans\":5", "\"spans\":4");
            v["seal"]["statement"] = json!(statement);
        });
        assert_eq!(failing(&verify(&b, &keys)), ["seal"]);

        // An audit entry's detail; then the same with its hash recomputed.
        let b = tampered(&original, |v| v["audit"][1]["detail"] = json!("by Mallory"));
        assert_eq!(failing(&verify(&b, &keys)), ["audit", "attestation"]);
        let mut b = original.clone();
        b.audit[1].detail = "by Mallory".into();
        b.audit[1].hash = audit_chain::entry_hash(&b.audit[1].prev_hash, &b.audit[1]);
        assert_eq!(failing(&verify(&b, &keys)), ["attestation"]);
        // An audit entry removed.
        let mut b = original.clone();
        b.audit.remove(1);
        assert_eq!(failing(&verify(&b, &keys)), ["attestation"]);

        // A name changed where the salt was dropped: the chain cannot see it, the attestation does.
        let b = tampered(&original, |v| {
            v["audit"][1]["salt"] = json!(null);
            v["audit"][1]["actor_name"] = json!("Alice");
        });
        assert_eq!(failing(&verify(&b, &keys)), ["attestation"]);
        // A seal stripped, the run's hash and the attestation left as the log says: the log's
        // trace_sealed entry names the key that signed it.
        let b = tampered(&original, |v| v["seal"] = json!(null));
        assert_eq!(failing(&verify(&b, &keys)), ["seal", "attestation"]);
        // A deleted graph whose deletion the bundle does not carry.
        let b = tampered(&original, |v| {
            v["run"]["graph_deleted_at"] = json!(Utc::now())
        });
        assert_eq!(failing(&verify(&b, &keys)), ["audit", "attestation"]);

        // The run's metadata or the payloads.
        let b = tampered(&original, |v| {
            v["run"]["graph_name"] = json!("Something else")
        });
        assert_eq!(failing(&verify(&b, &keys)), ["attestation"]);
        let b = tampered(&original, |v| v["payloads"][0]["size"] = json!(1));
        assert_eq!(failing(&verify(&b, &keys)), ["attestation"]);
    }
}
