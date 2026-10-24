//! A read-through cache in front of an [`OntologyBackend`]: onto-intelli's
//! deterministic reads (`FindEntities`, `Traverse`, `EventService/List`,
//! `SchemaService/Get` and `ListProposals`) are kept per tenant, caller and
//! request in [`OntoReads`](crate::app::caches::OntoReads) for
//! `NEXC_ONTO_READ_CACHE_SECS`, and callers that ask the same while a read is
//! on the wire share its answer. The caller is part of the key, so an
//! answer onto-intelli filtered for one principal is never served to
//! another.
//!
//! Every write that goes through here (ingest, a vocabulary written, a
//! graph opened, a proposal filed) drops what is kept for its tenant on this
//! replica and, over the realtime fan-out, on every other one. Questions
//! (`Ask`) are not kept here: onto-intelli keeps its answers itself
//! (`answer_cache`, `llm_cache`), and each answer reports its own cost.
//! Vocabularies have their own cache ([`super::vocabulary`]).

use std::sync::Arc;

use serde_json::Value;

use super::{
    AskAnswer, AskRequest, AskStream, Entity, EventQuery, FindQuery, IngestOutcome, OntoError,
    OntoEvent, OntoHealth, OntoSession, OntologyBackend, Principal, SchemaChange,
    SchemaProposalDraft, Subgraph, TraverseQuery,
};
use crate::app::caches::{Caches, Key};
use crate::domain::ontology::Ontology;

/// `inner`, its reads kept in `caches`.
pub struct CachedOntology {
    inner: Arc<dyn OntologyBackend>,
    caches: Arc<Caches>,
}

impl CachedOntology {
    pub fn new(inner: Arc<dyn OntologyBackend>, caches: Arc<Caches>) -> Self {
        CachedOntology { inner, caches }
    }

    /// The key of `method` called as `principal` with `request`.
    fn call(method: &str, principal: &Principal, request: impl std::fmt::Debug) -> String {
        format!("{method} {principal:?} {request:?}")
    }

    /// After a write to `principal`'s tenant, whether it went through or
    /// not (a failed one may have been applied before its answer was lost).
    fn wrote(&self, principal: &Principal) {
        self.caches
            .invalidate(Key::Ontology(principal.tenant.clone()));
    }
}

#[async_trait::async_trait]
impl OntologyBackend for CachedOntology {
    fn location(&self) -> String {
        self.inner.location()
    }

    async fn health(&self) -> Result<OntoHealth, OntoError> {
        self.inner.health().await
    }

    async fn ingest(
        &self,
        principal: &Principal,
        records: Vec<Value>,
    ) -> Result<IngestOutcome, OntoError> {
        let outcome = self.inner.ingest(principal, records).await;
        self.wrote(principal);
        outcome
    }

    async fn ask(&self, principal: &Principal, req: AskRequest) -> Result<AskAnswer, OntoError> {
        self.inner.ask(principal, req).await
    }

    async fn ask_stream(
        &self,
        principal: &Principal,
        req: AskRequest,
    ) -> Result<AskStream, OntoError> {
        self.inner.ask_stream(principal, req).await
    }

    async fn find(&self, principal: &Principal, q: FindQuery) -> Result<Vec<Entity>, OntoError> {
        let call = Self::call("find", principal, &q);
        self.caches
            .onto_read(&principal.tenant, call, self.inner.find(principal, q))
            .await
    }

    async fn traverse(
        &self,
        principal: &Principal,
        q: TraverseQuery,
    ) -> Result<Subgraph, OntoError> {
        let call = Self::call("traverse", principal, &q);
        self.caches
            .onto_read(&principal.tenant, call, self.inner.traverse(principal, q))
            .await
    }

    async fn events(
        &self,
        principal: &Principal,
        q: EventQuery,
    ) -> Result<Vec<OntoEvent>, OntoError> {
        let call = Self::call("events", principal, &q);
        self.caches
            .onto_read(&principal.tenant, call, self.inner.events(principal, q))
            .await
    }

    async fn vocabulary(&self, principal: &Principal, graph: &str) -> Result<Ontology, OntoError> {
        self.inner.vocabulary(principal, graph).await
    }

    async fn set_vocabulary(
        &self,
        principal: &Principal,
        graph: &str,
        vocabulary: &Ontology,
    ) -> Result<Ontology, OntoError> {
        let written = self
            .inner
            .set_vocabulary(principal, graph, vocabulary)
            .await;
        self.wrote(principal);
        written
    }

    async fn init_graph(
        &self,
        principal: &Principal,
        graph: &str,
        title: &str,
        vocabulary: Option<&Ontology>,
    ) -> Result<Ontology, OntoError> {
        let opened = self
            .inner
            .init_graph(principal, graph, title, vocabulary)
            .await;
        self.wrote(principal);
        opened
    }

    async fn schema(&self, principal: &Principal) -> Result<Value, OntoError> {
        let call = Self::call("schema", principal, ());
        self.caches
            .onto_read(&principal.tenant, call, self.inner.schema(principal))
            .await
    }

    async fn plan_schema(
        &self,
        principal: &Principal,
        change: &SchemaChange,
    ) -> Result<Value, OntoError> {
        self.inner.plan_schema(principal, change).await
    }

    async fn propose_schema(
        &self,
        principal: &Principal,
        proposal: &SchemaProposalDraft,
    ) -> Result<Value, OntoError> {
        let filed = self.inner.propose_schema(principal, proposal).await;
        self.wrote(principal);
        filed
    }

    async fn proposals(
        &self,
        principal: &Principal,
        status: Option<&str>,
        limit: u32,
    ) -> Result<Vec<Value>, OntoError> {
        let call = Self::call("proposals", principal, (status, limit));
        self.caches
            .onto_read(
                &principal.tenant,
                call,
                self.inner.proposals(principal, status, limit),
            )
            .await
    }

    async fn mint_session(
        &self,
        principal: &Principal,
        ttl_secs: u64,
    ) -> Result<OntoSession, OntoError> {
        self.inner.mint_session(principal, ttl_secs).await
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;
    use tokio::sync::mpsc;
    use uuid::Uuid;

    use super::*;
    use crate::observability::metrics::Metrics;
    use crate::ontology::stub::StubOntology;

    fn cached(ttl: Duration) -> (Arc<StubOntology>, Arc<Caches>, CachedOntology) {
        let stub = Arc::new(StubOntology::new());
        let caches = Arc::new(
            Caches::new(Duration::from_secs(60), Arc::new(Metrics::default()))
                .with_onto_read_ttl(ttl),
        );
        let backend = CachedOntology::new(stub.clone(), caches.clone());
        (stub, caches, backend)
    }

    fn policies() -> FindQuery {
        FindQuery {
            kind: "policy".into(),
            props: json!({"all_tools": true}),
            state: Vec::new(),
            limit: 10,
        }
    }

    async fn seed(backend: &CachedOntology, principal: &Principal, id: &str) {
        backend
            .ingest(
                principal,
                vec![
                    json!({"op": "upsert", "id": id, "kind": "policy", "title": id,
                            "props": {"all_tools": true}}),
                ],
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn reads_are_kept_per_caller_and_dropped_by_a_write() {
        let (stub, caches, backend) = cached(Duration::from_secs(60));
        let workspace = Uuid::now_v7();
        let service = Principal::service("nexc_", workspace);
        let member = Principal::user(
            "nexc_",
            workspace,
            Uuid::now_v7(),
            crate::domain::workspace::WorkspaceRole::Admin,
        );
        seed(&backend, &service, "policy:a").await;

        let first = backend.find(&member, policies()).await.unwrap();
        let again = backend.find(&member, policies()).await.unwrap();
        assert_eq!(first, again);
        assert_eq!(stub.calls_to("find").len(), 1, "the second read was kept");

        backend.find(&service, policies()).await.unwrap();
        assert_eq!(
            stub.calls_to("find").len(),
            2,
            "another caller is another key: onto-intelli filters per principal"
        );
        let mut wider = policies();
        wider.limit = 11;
        backend.find(&member, wider).await.unwrap();
        assert_eq!(
            stub.calls_to("find").len(),
            3,
            "another request is another key"
        );

        seed(&backend, &service, "policy:b").await;
        let after = backend.find(&member, policies()).await.unwrap();
        assert_eq!(
            stub.calls_to("find").len(),
            4,
            "a write drops the tenant's reads"
        );
        assert_eq!(after.len(), first.len() + 1, "and the next read sees it");

        let other = Principal::service("nexc_", Uuid::now_v7());
        backend.find(&other, policies()).await.unwrap();
        seed(&backend, &other, "policy:c").await;
        backend.find(&member, policies()).await.unwrap();
        assert_eq!(
            stub.calls_to("find").len(),
            5,
            "a write to another tenant leaves this one's reads"
        );
        assert!(
            caches
                .sizes()
                .contains(&("onto_read", caches.onto_reads().len()))
        );
    }

    #[tokio::test]
    async fn errors_are_not_kept_and_a_zero_lifetime_keeps_nothing() {
        let (stub, _, backend) = cached(Duration::from_secs(60));
        let principal = Principal::service("nexc_", Uuid::now_v7());
        stub.fail_next("find", 1, OntoError::Unavailable("down".into()));
        assert!(backend.find(&principal, policies()).await.is_err());
        assert!(backend.find(&principal, policies()).await.is_ok());
        assert!(backend.find(&principal, policies()).await.is_ok());
        assert_eq!(
            stub.calls_to("find").len(),
            2,
            "the failure was asked again"
        );

        let (stub, caches, backend) = cached(Duration::ZERO);
        backend.find(&principal, policies()).await.unwrap();
        backend.find(&principal, policies()).await.unwrap();
        assert_eq!(stub.calls_to("find").len(), 2);
        assert!(caches.onto_reads().is_empty());
    }

    #[tokio::test]
    async fn concurrent_reads_share_one_call() {
        let (stub, _, backend) = cached(Duration::from_secs(60));
        let backend = Arc::new(backend);
        let principal = Principal::service("nexc_", Uuid::now_v7());
        let reads = (0..16).map(|_| {
            let (backend, principal) = (backend.clone(), principal.clone());
            tokio::spawn(async move { backend.events(&principal, EventQuery::default()).await })
        });
        for read in futures::future::join_all(reads).await {
            read.unwrap().unwrap();
        }
        assert_eq!(stub.calls_to("events").len(), 1);
    }

    #[tokio::test]
    async fn writes_go_out_to_the_other_replicas() {
        let (_, caches, backend) = cached(Duration::from_secs(60));
        let (tx, mut rx) = mpsc::channel(4);
        caches.enable_fanout(tx);
        let principal = Principal::service("nexc_", Uuid::now_v7());
        backend
            .propose_schema(
                &principal,
                &SchemaProposalDraft {
                    title: "t".into(),
                    ..SchemaProposalDraft::default()
                },
            )
            .await
            .ok();
        assert_eq!(
            rx.recv().await,
            Some(Key::Ontology(principal.tenant.clone()))
        );
        let wire = serde_json::to_string(&Key::Ontology("ws_1".into())).unwrap();
        assert_eq!(wire, r#"{"kind":"ontology","id":"ws_1"}"#);
    }
}
