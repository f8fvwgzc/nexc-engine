//! Hot-read caches, one set per instance: agent cards, resolved policies per
//! agent and policy version, workspace guardrails and workspace LLM
//! settings. A write drops the entry here and tells every other replica to
//! drop it too, over the realtime fan-out (PostgreSQL `NOTIFY`, see
//! [`crate::realtime::fanout`]); `NEXC_CACHE_TTL_SECS` bounds how long an
//! entry may live regardless (0 turns the caches off).

use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::domain::agent::{Agent, AgentOwner, AgentTeam};
use crate::domain::agent_template::AgentEvaluation;
use crate::domain::guardrails::Guardrails;
use crate::domain::policy::PolicyVersions;
use crate::domain::policy_document::Policy;
use crate::engine::policy::{Resolved, levels_for, resolve, versions_of};
use crate::observability::metrics::Metrics;
use crate::repo;
use crate::repo::settings::StoredLlmSettings;

/// Entries kept at most, per cache, before the expired ones are swept.
const SWEEP_AT: usize = 10_000;

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
            fanout: OnceLock::new(),
        }
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
        }
    }

    /// Sweeps expired entries (housekeeping).
    pub fn prune(&self) {
        self.cards.prune(self.ttl);
        self.policies.prune(self.ttl);
        self.resolved.prune(self.ttl);
        self.guardrails.prune(self.ttl);
        self.workspace_llm.prune(self.ttl);
    }

    /// What is kept right now, per cache (for tests and the operator).
    pub fn sizes(&self) -> Vec<(&'static str, usize)> {
        vec![
            (self.cards.name, self.cards.entries.len()),
            (self.policies.name, self.policies.entries.len()),
            (self.resolved.name, self.resolved.entries.len()),
            (self.guardrails.name, self.guardrails.entries.len()),
            (self.workspace_llm.name, self.workspace_llm.entries.len()),
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
