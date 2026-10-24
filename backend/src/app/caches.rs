//! Hot-read caches, one set per instance: agent cards, resolved policies per
//! agent and policy version, workspace guardrails, workspace LLM settings,
//! graph vocabularies read from onto-intelli and onto-intelli's other reads
//! ([`OntoReads`]). A write drops the entry here and tells every other
//! replica to drop it too, over the realtime fan-out (PostgreSQL `NOTIFY`,
//! see [`crate::realtime::fanout`]); `NEXC_CACHE_TTL_SECS` bounds how long an
//! entry may live regardless (0 turns the caches off). Vocabularies have
//! their own lifetime (`NEXC_ONTO_VOCABULARY_TTL_SECS`), and one past it is
//! still served while onto-intelli cannot be reached; onto-intelli's other
//! reads live `NEXC_ONTO_READ_CACHE_SECS` and are never served past it.

use std::any::Any;
use std::future::Future;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tokio::sync::{OnceCell, mpsc};
use uuid::Uuid;

use crate::domain::agent::{Agent, AgentOwner, AgentTeam};
use crate::domain::agent_template::AgentEvaluation;
use crate::domain::guardrails::Guardrails;
use crate::domain::ontology::Ontology;
use crate::domain::policy::PolicyVersions;
use crate::domain::policy_document::Policy;
use crate::engine::policy::{Resolved, levels_for, resolve, versions_of};
use crate::observability::metrics::Metrics;
use crate::repo;
use crate::repo::settings::StoredLlmSettings;

/// Entries kept at most, per cache, before the expired ones are swept.
const SWEEP_AT: usize = 10_000;
/// How long a vocabulary past its lifetime is kept to be served while
/// onto-intelli cannot be reached.
const VOCABULARY_KEPT: Duration = Duration::from_secs(24 * 3600);

/// What a write invalidates; the wire form between replicas.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum Key {
    /// An agent changed: its card and its resolved policies.
    Agent(Uuid),
    /// Every agent changed (a sweep over the table).
    Agents,
    /// A policy of the workspace was added: its latest policies and every resolution under them.
    Policies(Uuid),
    Guardrails(Uuid),
    WorkspaceLlm(Uuid),
    /// A graph's vocabulary changed in onto-intelli through nexc.
    Vocabulary(Uuid),
    /// nexc wrote to this onto-intelli tenant: every read kept for it.
    Ontology(String),
}

/// The part of an agent's card that only its writes change (its recent
/// spans are read live).
#[derive(Debug, Clone)]
pub struct AgentCardCore {
    pub workspace_id: Uuid,
    pub agent: Agent,
    pub owner: Option<AgentOwner>,
    pub team: Option<AgentTeam>,
    pub evaluation: Option<AgentEvaluation>,
}

/// An agent's effective policy and the versions it was resolved under.
#[derive(Debug, Clone)]
pub struct ResolvedPolicy {
    pub resolved: Arc<Resolved>,
    pub versions: PolicyVersions,
}

struct Entry<T> {
    at: Instant,
    value: Arc<T>,
}

/// One cache: a map with a time-to-live.
struct Cache<K: Eq + std::hash::Hash, T> {
    name: &'static str,
    entries: DashMap<K, Entry<T>>,
}

impl<K: Eq + std::hash::Hash + Clone, T> Cache<K, T> {
    fn new(name: &'static str) -> Self {
        Cache {
            name,
            entries: DashMap::new(),
        }
    }

    fn get(&self, key: &K, ttl: Duration, metrics: &Metrics) -> Option<Arc<T>> {
        let found = self
            .entries
            .get(key)
            .filter(|e| e.at.elapsed() < ttl)
            .map(|e| e.value.clone());
        metrics.cache_lookups.add(
            &[
                ("cache", self.name),
                ("result", if found.is_some() { "hit" } else { "miss" }),
            ],
            1,
        );
        found
    }

    fn put(&self, key: K, value: Arc<T>, ttl: Duration) {
        if ttl.is_zero() {
            return;
        }
        if self.entries.len() >= SWEEP_AT {
            self.entries.retain(|_, e| e.at.elapsed() < ttl);
        }
        self.entries.insert(
            key,
            Entry {
                at: Instant::now(),
                value,
            },
        );
    }

    fn prune(&self, ttl: Duration) {
        self.entries.retain(|_, e| e.at.elapsed() < ttl);
    }
}

/// What one read of onto-intelli answered, whatever its type.
type Answer = Arc<dyn Any + Send + Sync>;

/// A read of a tenant, under the tenant's generation when it was asked.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ReadKey {
    tenant: String,
    generation: u64,
    /// The method, who asked and what: the whole request, compared exactly.
    call: String,
}

/// One read: answered once, by the first caller; the others that ask the
/// same while it is on the wire wait for that answer instead of sending
/// their own.
struct Slot {
    at: Instant,
    cell: Arc<OnceCell<Answer>>,
}

/// How a read was answered (`nexc_cache_lookups_total{cache="onto_read"}`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReadOutcome {
    /// Kept from an earlier call.
    Hit,
    /// Asked onto-intelli.
    Miss,
    /// Waited for the same read another caller had on the wire.
    Coalesced,
}

impl ReadOutcome {
    fn as_str(self) -> &'static str {
        match self {
            ReadOutcome::Hit => "hit",
            ReadOutcome::Miss => "miss",
            ReadOutcome::Coalesced => "coalesced",
        }
    }
}

/// onto-intelli's reads (find, traverse, events, schema, proposals), kept
/// per tenant, caller and request for `NEXC_ONTO_READ_CACHE_SECS`. Every
/// write nexc makes to a tenant moves the tenant to a new generation here
/// and on every other replica, so a read never answers from before a write
/// nexc made; a change made in onto-intelli itself shows within the
/// lifetime. Errors are never kept.
pub struct OntoReads {
    ttl: Duration,
    generations: DashMap<String, u64>,
    entries: DashMap<ReadKey, Slot>,
}

impl OntoReads {
    /// Reads kept `ttl` (zero: every read asks onto-intelli).
    pub fn new(ttl: Duration) -> Self {
        OntoReads {
            ttl,
            generations: DashMap::new(),
            entries: DashMap::new(),
        }
    }

    fn generation(&self, tenant: &str) -> u64 {
        self.generations.get(tenant).map_or(0, |g| *g)
    }

    /// Moves `tenant` to a new generation: every read kept for it is gone.
    pub fn invalidate_tenant(&self, tenant: &str) {
        *self.generations.entry(tenant.to_owned()).or_insert(0) += 1;
        self.entries.retain(|k, _| k.tenant != tenant);
    }

    /// Sweeps expired reads.
    pub fn prune(&self) {
        self.entries.retain(|_, s| s.at.elapsed() < self.ttl);
    }

    /// Reads kept right now.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nothing is kept.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `call` in `tenant`, answered from what is kept, from the same read
    /// already on the wire, or by `load`. `call` must name the method, the
    /// caller and every field of the request: it is the whole key.
    pub async fn read<T, E, F>(
        &self,
        tenant: &str,
        call: String,
        metrics: &Metrics,
        load: F,
    ) -> Result<T, E>
    where
        T: Clone + Send + Sync + 'static,
        F: Future<Output = Result<T, E>>,
    {
        if self.ttl.is_zero() {
            return load.await;
        }
        let key = ReadKey {
            tenant: tenant.to_owned(),
            generation: self.generation(tenant),
            call,
        };
        let cell = {
            if self.entries.len() >= SWEEP_AT {
                self.prune();
            }
            let mut slot = self.entries.entry(key).or_insert_with(|| Slot {
                at: Instant::now(),
                cell: Arc::new(OnceCell::new()),
            });
            if slot.at.elapsed() >= self.ttl {
                *slot = Slot {
                    at: Instant::now(),
                    cell: Arc::new(OnceCell::new()),
                };
            }
            slot.cell.clone()
        };
        let mut outcome = if cell.initialized() {
            ReadOutcome::Hit
        } else {
            ReadOutcome::Coalesced
        };
        let answer = cell
            .get_or_try_init(|| {
                outcome = ReadOutcome::Miss;
                async move { load.await.map(|value| Arc::new(value) as Answer) }
            })
            .await;
        metrics
            .cache_lookups
            .add(&[("cache", "onto_read"), ("result", outcome.as_str())], 1);
        let answer = answer?;
        Ok(answer
            .downcast_ref::<T>()
            .expect("a call's key names its method, so its answer has the method's type")
            .clone())
    }
}

/// The caches of this instance.
pub struct Caches {
    ttl: Duration,
    metrics: Arc<Metrics>,
    cards: Cache<Uuid, AgentCardCore>,
    policies: Cache<Uuid, Vec<Policy>>,
    /// By workspace, agent and the agent's policy version.
    resolved: Cache<(Uuid, Uuid, i32), ResolvedPolicy>,
    guardrails: Cache<Uuid, Guardrails>,
    workspace_llm: Cache<Uuid, Option<StoredLlmSettings>>,
    /// By graph; served fresh for `vocabulary_ttl`, kept longer as a fallback.
    vocabularies: Cache<Uuid, Ontology>,
    vocabulary_ttl: Duration,
    onto_reads: OntoReads,
    fanout: OnceLock<mpsc::Sender<Key>>,
}

impl Caches {
    /// Caches whose entries live `ttl` at most (zero: no caching).
    pub fn new(ttl: Duration, metrics: Arc<Metrics>) -> Self {
        Caches {
            ttl,
            metrics,
            cards: Cache::new("agent_card"),
            policies: Cache::new("policies"),
            resolved: Cache::new("resolved_policy"),
            guardrails: Cache::new("guardrails"),
            workspace_llm: Cache::new("workspace_llm"),
            vocabularies: Cache::new("vocabulary"),
            vocabulary_ttl: Duration::from_secs(30),
            onto_reads: OntoReads::new(Duration::ZERO),
            fanout: OnceLock::new(),
        }
    }

    /// Keeps onto-intelli's reads for `ttl` (zero, the default: always ask).
    pub fn with_onto_read_ttl(mut self, ttl: Duration) -> Self {
        self.onto_reads = OntoReads::new(ttl);
        self
    }

    /// onto-intelli's reads kept here.
    pub fn onto_reads(&self) -> &OntoReads {
        &self.onto_reads
    }

    /// `call` in onto-intelli `tenant`, read through [`OntoReads`].
    pub async fn onto_read<T, E, F>(&self, tenant: &str, call: String, load: F) -> Result<T, E>
    where
        T: Clone + Send + Sync + 'static,
        F: Future<Output = Result<T, E>>,
    {
        self.onto_reads
            .read(tenant, call, &self.metrics, load)
            .await
    }

    /// Serves graph vocabularies fresh for `ttl` (zero: always ask onto-intelli).
    pub fn with_vocabulary_ttl(mut self, ttl: Duration) -> Self {
        self.vocabulary_ttl = ttl;
        self
    }

    /// The vocabulary kept for `graph` and whether it is still fresh; a stale
    /// one is what a run falls back on while onto-intelli cannot be reached.
    pub fn vocabulary(&self, graph: Uuid) -> Option<(Arc<Ontology>, bool)> {
        let found = self
            .vocabularies
            .entries
            .get(&graph)
            .filter(|e| e.at.elapsed() < VOCABULARY_KEPT)
            .map(|e| (e.value.clone(), e.at.elapsed() < self.vocabulary_ttl));
        let result = match &found {
            Some((_, true)) => "hit",
            Some((_, false)) => "stale",
            None => "miss",
        };
        self.metrics
            .cache_lookups
            .add(&[("cache", self.vocabularies.name), ("result", result)], 1);
        found
    }

    /// Keeps `vocabulary` as `graph`'s (kept even with a zero lifetime, as a fallback).
    pub fn put_vocabulary(&self, graph: Uuid, vocabulary: Arc<Ontology>) {
        self.vocabularies.put(graph, vocabulary, VOCABULARY_KEPT);
    }

    /// How long an entry lives.
    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    /// Sends every invalidation to `tx` as well, for the other replicas.
    pub fn enable_fanout(&self, tx: mpsc::Sender<Key>) {
        let _ = self.fanout.set(tx);
    }

    /// Drops what `key` names here and on every other replica.
    pub fn invalidate(&self, key: Key) {
        self.drop_local(&key);
        if let Some(tx) = self.fanout.get()
            && tx.try_send(key).is_err()
        {
            tracing::warn!(
                "cache invalidation queue full; other replicas keep the entry until its TTL"
            );
        }
    }

    /// Drops what `key` names on this instance only.
    pub fn drop_local(&self, key: &Key) {
        match key {
            Key::Agent(id) => {
                self.cards.entries.remove(id);
                self.resolved.entries.retain(|(_, agent, _), _| agent != id);
            }
            Key::Agents => {
                self.cards.entries.clear();
                self.resolved.entries.clear();
            }
            Key::Policies(workspace) => {
                self.policies.entries.remove(workspace);
                self.resolved.entries.retain(|(w, _, _), _| w != workspace);
            }
            Key::Guardrails(workspace) => {
                self.guardrails.entries.remove(workspace);
            }
            Key::WorkspaceLlm(workspace) => {
                self.workspace_llm.entries.remove(workspace);
            }
            Key::Vocabulary(graph) => {
                self.vocabularies.entries.remove(graph);
            }
            Key::Ontology(tenant) => {
                self.onto_reads.invalidate_tenant(tenant);
            }
        }
    }

    /// Sweeps expired entries (housekeeping).
    pub fn prune(&self) {
        self.cards.prune(self.ttl);
        self.policies.prune(self.ttl);
        self.resolved.prune(self.ttl);
        self.guardrails.prune(self.ttl);
        self.workspace_llm.prune(self.ttl);
        self.vocabularies.prune(VOCABULARY_KEPT);
        self.onto_reads.prune();
    }

    /// What is kept right now, per cache (for tests and the operator).
    pub fn sizes(&self) -> Vec<(&'static str, usize)> {
        vec![
            (self.cards.name, self.cards.entries.len()),
            (self.policies.name, self.policies.entries.len()),
            (self.resolved.name, self.resolved.entries.len()),
            (self.guardrails.name, self.guardrails.entries.len()),
            (self.workspace_llm.name, self.workspace_llm.entries.len()),
            (self.vocabularies.name, self.vocabularies.entries.len()),
            ("onto_read", self.onto_reads.len()),
        ]
    }

    /// Whether a card is kept for `agent` (tests).
    pub fn has_card(&self, agent: Uuid) -> bool {
        self.cards
            .entries
            .get(&agent)
            .is_some_and(|e| e.at.elapsed() < self.ttl)
    }

    /// Whether guardrails are kept for `workspace` (tests).
    pub fn has_guardrails(&self, workspace: Uuid) -> bool {
        self.guardrails
            .entries
            .get(&workspace)
            .is_some_and(|e| e.at.elapsed() < self.ttl)
    }

    /// The card's core of agent `id`, whatever its workspace: the cached one,
    /// or loaded after one lookup of the workspace it belongs to.
    pub async fn agent_card_by_id(
        &self,
        db: &PgPool,
        id: Uuid,
    ) -> Result<Option<Arc<AgentCardCore>>, sqlx::Error> {
        if let Some(core) = self.cards.get(&id, self.ttl, &self.metrics) {
            return Ok(Some(core));
        }
        let Some(Some(workspace)) = repo::agents::workspace_of(db, id).await? else {
            return Ok(None);
        };
        self.agent_card(db, workspace, id).await
    }

    /// The card's core of agent `id` in `workspace`, loaded once per change.
    pub async fn agent_card(
        &self,
        db: &PgPool,
        workspace: Uuid,
        id: Uuid,
    ) -> Result<Option<Arc<AgentCardCore>>, sqlx::Error> {
        if let Some(core) = self.cards.get(&id, self.ttl, &self.metrics) {
            if core.workspace_id == workspace {
                return Ok(Some(core));
            }
            return Ok(None);
        }
        let Some(agent) = repo::agents::find(db, workspace, id).await? else {
            return Ok(None);
        };
        let owner = repo::agents::owner(db, agent.owner_user_id).await?;
        let team = repo::agents::team(db, agent.team_id).await?;
        let evaluation = repo::agent_evaluations::find(db, id).await?;
        let core = Arc::new(AgentCardCore {
            workspace_id: workspace,
            agent,
            owner,
            team,
            evaluation,
        });
        self.cards.put(id, core.clone(), self.ttl);
        Ok(Some(core))
    }

    /// The latest version of every policy level of `workspace`.
    pub async fn policies(
        &self,
        db: &PgPool,
        workspace: Uuid,
    ) -> Result<Arc<Vec<Policy>>, sqlx::Error> {
        if let Some(found) = self.policies.get(&workspace, self.ttl, &self.metrics) {
            return Ok(found);
        }
        let latest = Arc::new(repo::policies::latest_all(db, workspace).await?);
        self.policies.put(workspace, latest.clone(), self.ttl);
        Ok(latest)
    }

    /// The effective policy of `agent` in `workspace`, resolved once per
    /// policy version and per change of the workspace's policies.
    pub async fn resolved(
        &self,
        db: &PgPool,
        workspace: Uuid,
        agent: &Agent,
    ) -> Result<Arc<ResolvedPolicy>, sqlx::Error> {
        let key = (workspace, agent.id, agent.policy_version);
        if let Some(found) = self.resolved.get(&key, self.ttl, &self.metrics) {
            return Ok(found);
        }
        let policies = self.policies(db, workspace).await?;
        let levels = levels_for(&policies, agent.team_id, Some(agent.id));
        let resolved = Arc::new(ResolvedPolicy {
            versions: versions_of(&levels),
            resolved: Arc::new(resolve(&levels, Some(&agent.capabilities))),
        });
        self.resolved.put(key, resolved.clone(), self.ttl);
        Ok(resolved)
    }

    /// The guardrails of `workspace` (the permissive defaults when it set none).
    pub async fn guardrails(
        &self,
        db: &PgPool,
        workspace: Uuid,
    ) -> Result<Arc<Guardrails>, sqlx::Error> {
        if let Some(found) = self.guardrails.get(&workspace, self.ttl, &self.metrics) {
            return Ok(found);
        }
        let guardrails = Arc::new(
            repo::workspaces::guardrails(db, workspace)
                .await?
                .unwrap_or_default(),
        );
        self.guardrails.put(workspace, guardrails.clone(), self.ttl);
        Ok(guardrails)
    }

    /// The LLM credential of `workspace`, if it holds one.
    pub async fn workspace_llm(
        &self,
        db: &PgPool,
        workspace: Uuid,
    ) -> Result<Arc<Option<StoredLlmSettings>>, sqlx::Error> {
        if let Some(found) = self.workspace_llm.get(&workspace, self.ttl, &self.metrics) {
            return Ok(found);
        }
        let stored = Arc::new(repo::settings::find_for_workspace(db, workspace).await?);
        self.workspace_llm.put(workspace, stored.clone(), self.ttl);
        Ok(stored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caches(ttl: Duration) -> Caches {
        Caches::new(ttl, Arc::new(Metrics::default()))
    }

    #[test]
    fn entries_live_until_dropped_or_expired() {
        let c = caches(Duration::from_secs(60));
        let (w, a) = (Uuid::now_v7(), Uuid::now_v7());
        c.guardrails.put(w, Arc::new(Guardrails::default()), c.ttl);
        c.resolved.put(
            (w, a, 1),
            Arc::new(ResolvedPolicy {
                resolved: Arc::new(resolve(&[], None)),
                versions: PolicyVersions::default(),
            }),
            c.ttl,
        );
        c.resolved.put(
            (Uuid::now_v7(), Uuid::now_v7(), 1),
            Arc::new(ResolvedPolicy {
                resolved: Arc::new(resolve(&[], None)),
                versions: PolicyVersions::default(),
            }),
            c.ttl,
        );
        assert!(c.has_guardrails(w));
        assert_eq!(c.resolved.entries.len(), 2);

        c.drop_local(&Key::Agent(a));
        assert_eq!(c.resolved.entries.len(), 1, "the agent's resolution went");
        c.drop_local(&Key::Policies(w));
        assert!(c.has_guardrails(w), "another key leaves the guardrails");
        c.drop_local(&Key::Guardrails(w));
        assert!(!c.has_guardrails(w));
        c.drop_local(&Key::Agents);
        assert_eq!(c.resolved.entries.len(), 0);

        let expired = caches(Duration::ZERO);
        expired
            .guardrails
            .put(w, Arc::new(Guardrails::default()), expired.ttl);
        assert!(!expired.has_guardrails(w), "a zero TTL keeps nothing");
        let m = Arc::new(Metrics::default());
        let short = Caches::new(Duration::from_millis(1), m.clone());
        short
            .guardrails
            .put(w, Arc::new(Guardrails::default()), short.ttl);
        std::thread::sleep(Duration::from_millis(5));
        assert!(short.guardrails.get(&w, short.ttl, &m).is_none());
        short.prune();
        assert!(short.guardrails.entries.is_empty());
        assert!(
            m.render()
                .contains("nexc_cache_lookups_total{cache=\"guardrails\",result=\"miss\"} 1")
        );
    }

    #[tokio::test]
    async fn invalidations_go_out_to_the_other_replicas() {
        let c = caches(Duration::from_secs(60));
        let (tx, mut rx) = mpsc::channel(4);
        c.enable_fanout(tx);
        let w = Uuid::now_v7();
        c.guardrails.put(w, Arc::new(Guardrails::default()), c.ttl);
        c.invalidate(Key::Guardrails(w));
        assert!(!c.has_guardrails(w));
        assert_eq!(rx.recv().await, Some(Key::Guardrails(w)));
        let wire = serde_json::to_string(&Key::Policies(w)).unwrap();
        assert_eq!(wire, format!("{{\"kind\":\"policies\",\"id\":\"{w}\"}}"));
        assert_eq!(
            serde_json::from_str::<Key>(&wire).unwrap(),
            Key::Policies(w)
        );
    }
}
