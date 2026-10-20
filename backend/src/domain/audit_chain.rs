//! The audit log's hash chain: each workspace's entries form one chain in
//! the order they were written (`seq`). The database computes it, in the
//! trigger of migration `20261020000201_audit_chain` (no writer can skip
//! it); this module recomputes it, byte for byte, to verify a stored log or
//! an exported evidence bundle without the database.
//!
//! An entry's hash covers the previous entry's hash, its workspace, its
//! position, its id, when it was written, who wrote it, what it records, and
//! its names as salted digests (`SHA-256(salt || name)`): erasing an account
//! replaces its name and drops the salt, and the chain still verifies.

use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::kernel;

/// The hash before the first entry of a workspace's chain.
pub const GENESIS: [u8; 32] = [0; 32];
/// What every entry's hashed bytes start with.
pub const TAG: &[u8] = b"nexc-audit-v1";

/// One entry of a workspace's audit chain with every field its hash covers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ChainedEntry {
    pub id: Uuid,
    pub workspace_id: Uuid,
    /// Position in the workspace's chain, from 0.
    pub seq: i64,
    /// The action as stored (`AuditAction` spelling).
    pub action: String,
    #[schema(required = true)]
    pub actor_id: Option<Uuid>,
    pub actor_name: String,
    pub subject: String,
    pub detail: String,
    pub created_at: DateTime<Utc>,
    /// Hex of the 16-byte salt of the name digests; `null` once the names were erased.
    #[serde(with = "hex_opt16")]
    #[schema(value_type = Option<String>, required = true)]
    pub salt: Option<[u8; 16]>,
    /// Hex SHA-256 of the salt and `actor_name`, as written.
    #[serde(with = "hex32")]
    #[schema(value_type = String)]
    pub actor_digest: [u8; 32],
    /// Hex SHA-256 of the salt and `subject`, as written.
    #[serde(with = "hex32")]
    #[schema(value_type = String)]
    pub subject_digest: [u8; 32],
    /// Hex hash of the previous entry (zeros for the first).
    #[serde(with = "hex32")]
    #[schema(value_type = String)]
    pub prev_hash: [u8; 32],
    #[serde(with = "hex32")]
    #[schema(value_type = String)]
    pub hash: [u8; 32],
}

/// Hex (de)serialisation of a 32-byte digest.
pub mod hex32 {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(bytes: &[u8; 32], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 32], D::Error> {
        let text = String::deserialize(d)?;
        let bytes = hex::decode(&text).map_err(D::Error::custom)?;
        bytes
            .try_into()
            .map_err(|_| D::Error::custom("a digest is 32 bytes"))
    }
}

mod hex_opt16 {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(bytes: &Option<[u8; 16]>, s: S) -> Result<S::Ok, S::Error> {
        match bytes {
            Some(bytes) => s.serialize_str(&hex::encode(bytes)),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<[u8; 16]>, D::Error> {
        let Some(text) = Option::<String>::deserialize(d)? else {
            return Ok(None);
        };
        let bytes = hex::decode(&text).map_err(D::Error::custom)?;
        bytes
            .try_into()
            .map(Some)
            .map_err(|_| D::Error::custom("a salt is 16 bytes"))
    }
}

/// `SHA-256(salt || text)`: how a name is covered by the chain.
pub fn text_digest(salt: &[u8; 16], text: &str) -> [u8; 32] {
    let mut material = Vec::with_capacity(16 + text.len());
    material.extend_from_slice(salt);
    material.extend_from_slice(text.as_bytes());
    kernel::sha256(&material)
}

/// Microseconds since 2000-01-01 UTC: PostgreSQL's own `timestamptz` value.
fn pg_micros(at: DateTime<Utc>) -> i64 {
    let epoch = Utc.with_ymd_and_hms(2000, 1, 1, 0, 0, 0).unwrap();
    (at - epoch).num_microseconds().unwrap_or(i64::MAX)
}

fn framed(out: &mut Vec<u8>, text: &str) {
    out.extend_from_slice(&(text.len() as u32).to_be_bytes());
    out.extend_from_slice(text.as_bytes());
}

/// The hash of `entry` after `prev`, as `audit_entry_hash` computes it in the database.
pub fn entry_hash(prev: &[u8; 32], entry: &ChainedEntry) -> [u8; 32] {
    let mut out = Vec::with_capacity(256 + entry.detail.len());
    out.extend_from_slice(TAG);
    out.extend_from_slice(prev);
    out.extend_from_slice(entry.workspace_id.as_bytes());
    out.extend_from_slice(&entry.seq.to_be_bytes());
    out.extend_from_slice(entry.id.as_bytes());
    out.extend_from_slice(&pg_micros(entry.created_at).to_be_bytes());
    match entry.actor_id {
        None => out.push(0),
        Some(actor) => {
            out.push(1);
            out.extend_from_slice(actor.as_bytes());
        }
    }
    out.extend_from_slice(&entry.actor_digest);
    framed(&mut out, &entry.action);
    out.extend_from_slice(&entry.subject_digest);
    framed(&mut out, &entry.detail);
    kernel::sha256(&out)
}

/// The first entry that does not fit a chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct BrokenEntry {
    /// `null` when the chain ends short of its head.
    #[schema(required = true)]
    pub entry_id: Option<Uuid>,
    pub seq: i64,
    pub reason: String,
}

/// Why `entry` does not follow `prev` at position `seq`, if it does not.
pub fn misfit(entry: &ChainedEntry, seq: i64, prev: &[u8; 32]) -> Option<String> {
    if entry.seq != seq {
        return Some(format!("entry {seq} is missing from the chain"));
    }
    if &entry.prev_hash != prev {
        return Some("prev_hash does not link to the previous entry".into());
    }
    if let Some(salt) = &entry.salt
        && (text_digest(salt, &entry.actor_name) != entry.actor_digest
            || text_digest(salt, &entry.subject) != entry.subject_digest)
    {
        return Some("the names do not match their digests".into());
    }
    if entry_hash(prev, entry) != entry.hash {
        return Some("hash does not match the entry's content".into());
    }
    None
}

/// Checks a workspace's chain entry by entry, in `seq` order, without
/// holding it in memory: push every entry, then [`ChainCheck::finish`].
#[derive(Debug, Clone)]
pub struct ChainCheck {
    prev: [u8; 32],
    next: i64,
    entries: i64,
    redacted: i64,
    broken: Option<BrokenEntry>,
}

impl Default for ChainCheck {
    fn default() -> Self {
        ChainCheck {
            prev: GENESIS,
            next: 0,
            entries: 0,
            redacted: 0,
            broken: None,
        }
    }
}

impl ChainCheck {
    /// Takes the next entry; false once the chain is broken (later entries
    /// change nothing).
    pub fn push(&mut self, entry: &ChainedEntry) -> bool {
        if self.broken.is_some() {
            return false;
        }
        if let Some(reason) = misfit(entry, self.next, &self.prev) {
            self.broken = Some(BrokenEntry {
                entry_id: Some(entry.id),
                seq: entry.seq,
                reason,
            });
            return false;
        }
        self.entries += 1;
        if entry.salt.is_none() {
            self.redacted += 1;
        }
        self.prev = entry.hash;
        self.next += 1;
        true
    }

    /// The verdict, given the chain's recorded head (`seq`, `hash`), when it has one.
    pub fn finish(mut self, head: Option<(i64, [u8; 32])>) -> AuditVerification {
        let last = (self.next - 1, self.prev);
        if self.broken.is_none() {
            match head {
                Some(head) if head != last => {
                    self.broken = Some(BrokenEntry {
                        entry_id: None,
                        seq: head.0,
                        reason: format!(
                            "the chain ends at entry {} but its head records entry {}: entries were removed or changed",
                            last.0, head.0
                        ),
                    });
                }
                None if self.next > 0 => {
                    self.broken = Some(BrokenEntry {
                        entry_id: None,
                        seq: last.0,
                        reason: "the chain has entries but no recorded head".into(),
                    });
                }
                _ => {}
            }
        }
        AuditVerification {
            ok: self.broken.is_none(),
            entries: self.entries,
            hash: (self.next > 0).then(|| hex::encode(self.prev)),
            head_seq: head.map(|h| h.0),
            head_hash: head.map(|h| hex::encode(h.1)),
            redacted: self.redacted,
            broken: self.broken,
        }
    }
}

/// Result of recomputing a workspace's audit chain.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AuditVerification {
    /// True when every entry hashes as stored, every link holds, every name
    /// that still has its salt matches its digest, and the chain ends at its
    /// recorded head.
    pub ok: bool,
    /// Entries checked before the first break (all of them when `ok`).
    pub entries: i64,
    /// Hex hash of the last entry that fits, `null` for an empty log.
    #[schema(required = true)]
    pub hash: Option<String>,
    /// Position and hex hash of the chain's head as the database records it,
    /// `null` before the first entry.
    #[schema(required = true)]
    pub head_seq: Option<i64>,
    #[schema(required = true)]
    pub head_hash: Option<String>,
    /// Entries whose names were erased with their account: chained, but
    /// their names can no longer be checked.
    pub redacted: i64,
    /// The first entry that does not fit, when `ok` is false.
    #[schema(required = true)]
    pub broken: Option<BrokenEntry>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(seq: i64, prev: [u8; 32]) -> ChainedEntry {
        let salt = [7u8; 16];
        let mut e = ChainedEntry {
            id: Uuid::now_v7(),
            workspace_id: Uuid::nil(),
            seq,
            action: "member_added".into(),
            actor_id: Some(Uuid::now_v7()),
            actor_name: "Alice".into(),
            subject: "Bob <bob@example.com>".into(),
            detail: format!("entry {seq}"),
            created_at: "2026-10-20T09:58:09.083326Z".parse().unwrap(),
            salt: Some(salt),
            actor_digest: text_digest(&salt, "Alice"),
            subject_digest: text_digest(&salt, "Bob <bob@example.com>"),
            prev_hash: prev,
            hash: GENESIS,
        };
        e.hash = entry_hash(&prev, &e);
        e
    }

    fn chain(n: i64) -> Vec<ChainedEntry> {
        let mut prev = GENESIS;
        (0..n)
            .map(|seq| {
                let e = entry(seq, prev);
                prev = e.hash;
                e
            })
            .collect()
    }

    fn check(entries: &[ChainedEntry], head: Option<(i64, [u8; 32])>) -> AuditVerification {
        let mut check = ChainCheck::default();
        for e in entries {
            check.push(e);
        }
        check.finish(head)
    }

    #[test]
    fn a_chain_verifies_against_its_head_and_names_the_first_break() {
        let entries = chain(4);
        let head = Some((3, entries[3].hash));
        let ok = check(&entries, head);
        assert!(ok.ok && ok.entries == 4 && ok.redacted == 0, "{ok:?}");
        assert!(check(&[], None).ok);

        let mut changed = entries.clone();
        changed[1].detail.push('!');
        let v = check(&changed, head);
        assert_eq!(v.broken.unwrap().entry_id, Some(entries[1].id));

        let mut renamed = entries.clone();
        renamed[2].actor_name = "Mallory".into();
        let v = check(&renamed, head);
        assert!(v.broken.unwrap().reason.contains("names"));

        // Erased names: the salt goes, the digests stay, the chain holds.
        let mut erased = entries.clone();
        erased[2].actor_name = "Deleted user".into();
        erased[2].salt = None;
        let v = check(&erased, head);
        assert!(v.ok && v.redacted == 1, "{v:?}");

        let v = check(&entries[..3], head);
        assert!(
            !v.ok && v.broken.unwrap().entry_id.is_none(),
            "a removed tail"
        );
        let mut gap = entries.clone();
        gap.remove(1);
        assert!(check(&gap, head).broken.unwrap().reason.contains("missing"));
    }

    #[test]
    fn the_layout_is_the_documented_one() {
        let e = entry(0, GENESIS);
        let mut expected = b"nexc-audit-v1".to_vec();
        expected.extend_from_slice(&GENESIS);
        expected.extend_from_slice(Uuid::nil().as_bytes());
        expected.extend_from_slice(&0i64.to_be_bytes());
        expected.extend_from_slice(e.id.as_bytes());
        // 2026-10-20T09:58:09.083326Z in microseconds since 2000-01-01, as
        // `timestamptz_send` writes it (0x000301416fb9d3be).
        expected.extend_from_slice(&845_805_489_083_326i64.to_be_bytes());
        expected.push(1);
        expected.extend_from_slice(e.actor_id.unwrap().as_bytes());
        expected.extend_from_slice(&e.actor_digest);
        expected.extend_from_slice(&12u32.to_be_bytes());
        expected.extend_from_slice(b"member_added");
        expected.extend_from_slice(&e.subject_digest);
        expected.extend_from_slice(&7u32.to_be_bytes());
        expected.extend_from_slice(b"entry 0");
        assert_eq!(e.hash, kernel::sha256(&expected));
    }

    #[test]
    fn entries_round_trip_through_json() {
        let e = entry(0, GENESIS);
        let text = serde_json::to_string(&e).unwrap();
        assert!(text.contains(&hex::encode(e.hash)));
        let back: ChainedEntry = serde_json::from_str(&text).unwrap();
        assert_eq!(back, e);
        let mut erased = e.clone();
        erased.salt = None;
        let back: ChainedEntry =
            serde_json::from_str(&serde_json::to_string(&erased).unwrap()).unwrap();
        assert_eq!(back.salt, None);
    }
}
