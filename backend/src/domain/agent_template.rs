//! Role templates for agents (`docs/AGENTSECOPS.md` §4.6), embedded from
//! `backend/templates/agent-*.json`: a role with its capabilities, its own
//! policy, the knowledge topics it is bound to and the competency questions
//! it is asked once when an agent is made from it.

use std::sync::OnceLock;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::agent::AgentRuntime;
use super::capabilities::Capabilities;
use super::policy_document::PolicyDocument;

/// Most characters of an answer kept with the evaluation.
pub const ANSWER_EXCERPT_CHARS: usize = 300;
/// Most tokens an answer may take.
pub const ANSWER_MAX_TOKENS: u32 = 400;

/// A competency question and what a competent answer mentions.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CompetencyQuestion {
    pub question: String,
    /// Words or phrases of which at least one is expected in the answer (case-insensitive).
    pub expected: Vec<String>,
}

impl CompetencyQuestion {
    /// Whether `answer` mentions what was expected.
    pub fn passes(&self, answer: &str) -> bool {
        let answer = answer.to_lowercase();
        self.expected
            .iter()
            .any(|e| !e.is_empty() && answer.contains(&e.to_lowercase()))
    }
}

/// A role template, as stored.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentTemplateSpec {
    /// The template's name in the URL (`analyst`).
    pub id: String,
    pub name: String,
    pub role: String,
    pub title: String,
    pub description: String,
    pub purpose: String,
    pub system_prompt: String,
    pub runtime: AgentRuntime,
    pub budget_tokens: i64,
    /// Whether the agent's model allow-list is set to the model it is created with.
    #[serde(default)]
    pub pin_model: bool,
    pub capabilities: Capabilities,
    /// The agent-level policy set as version 1 when an agent is made.
    pub policy: PolicyDocument,
    /// Terms of the knowledge topics the agent is bound to, when the workspace has them.
    #[serde(default)]
    pub knowledge: Vec<String>,
    pub questions: Vec<CompetencyQuestion>,
}

/// A template as listed by `GET /agent-templates`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AgentTemplate {
    pub id: String,
    pub name: String,
    pub role: String,
    pub title: String,
    pub description: String,
    pub purpose: String,
    pub runtime: AgentRuntime,
    pub budget_tokens: i64,
    pub capabilities: Capabilities,
    pub policy: PolicyDocument,
    pub knowledge: Vec<String>,
    pub questions: Vec<CompetencyQuestion>,
}

impl AgentTemplateSpec {
    /// The list view of this template.
    pub fn summary(&self) -> AgentTemplate {
        AgentTemplate {
            id: self.id.clone(),
            name: self.name.clone(),
            role: self.role.clone(),
            title: self.title.clone(),
            description: self.description.clone(),
            purpose: self.purpose.clone(),
            runtime: self.runtime,
            budget_tokens: self.budget_tokens,
            capabilities: self.capabilities.clone(),
            policy: self.policy.clone(),
            knowledge: self.knowledge.clone(),
            questions: self.questions.clone(),
        }
    }

    /// Ids of the workspace's topics the template binds to, by the terms
    /// `(id, terms)` of each topic; `None` when none match (no restriction).
    pub fn bound_topics<T: AsRef<str>>(
        &self,
        topics: &[(uuid::Uuid, Vec<T>)],
    ) -> Option<Vec<uuid::Uuid>> {
        if self.knowledge.is_empty() {
            return None;
        }
        let wanted: Vec<String> = self.knowledge.iter().map(|k| k.to_lowercase()).collect();
        let ids: Vec<uuid::Uuid> = topics
            .iter()
            .filter(|(_, terms)| {
                terms
                    .iter()
                    .any(|t| wanted.contains(&t.as_ref().to_lowercase()))
            })
            .map(|(id, _)| *id)
            .collect();
        (!ids.is_empty()).then_some(ids)
    }
}

/// One question as it was answered.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct EvaluatedQuestion {
    pub question: String,
    pub expected: Vec<String>,
    pub passed: bool,
    /// The first characters of the answer.
    pub answer: String,
}

/// The score an agent made from a template got on its competency questions.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AgentEvaluation {
    /// The template's id.
    pub template: String,
    pub score: i32,
    pub total: i32,
    /// The model that answered.
    pub model: String,
    pub questions: Vec<EvaluatedQuestion>,
    pub evaluated_at: DateTime<Utc>,
}

/// Shortens an answer to what the evaluation keeps.
pub fn excerpt(answer: &str) -> String {
    let trimmed = answer.trim();
    if trimmed.chars().count() <= ANSWER_EXCERPT_CHARS {
        return trimmed.to_owned();
    }
    let mut short: String = trimmed.chars().take(ANSWER_EXCERPT_CHARS - 1).collect();
    short.push('…');
    short
}

const SOURCES: [&str; 3] = [
    include_str!("../../templates/agent-analyst.json"),
    include_str!("../../templates/agent-researcher.json"),
    include_str!("../../templates/agent-reviewer.json"),
];

/// Every built-in role template (parsed once; the files are validated by tests).
pub fn builtin() -> &'static [AgentTemplateSpec] {
    static TEMPLATES: OnceLock<Vec<AgentTemplateSpec>> = OnceLock::new();
    TEMPLATES.get_or_init(|| {
        SOURCES
            .iter()
            .map(|src| {
                serde_json::from_str(src)
                    .expect("built-in agent template JSON is validated by tests")
            })
            .collect()
    })
}

/// A built-in template by id, whatever the case.
pub fn find(id: &str) -> Option<&'static AgentTemplateSpec> {
    builtin()
        .iter()
        .find(|t| t.id.eq_ignore_ascii_case(id.trim()))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use uuid::Uuid;

    use super::*;
    use crate::domain::agent::is_valid_role;
    use crate::domain::validation::FieldErrors;

    #[test]
    fn every_template_is_well_formed() {
        let templates = builtin();
        assert_eq!(templates.len(), 3);
        let mut ids = HashSet::new();
        for t in templates {
            assert!(ids.insert(t.id.as_str()), "duplicate id {}", t.id);
            assert!(is_valid_role(&t.role), "{}: role", t.id);
            assert!(!t.name.is_empty() && !t.purpose.is_empty() && !t.system_prompt.is_empty());
            assert!(t.budget_tokens > 0);
            assert_eq!(t.questions.len(), 3, "{}: three competency questions", t.id);
            for q in &t.questions {
                assert!(!q.question.is_empty() && !q.expected.is_empty(), "{}", t.id);
            }
            let mut errors = FieldErrors::default();
            t.capabilities.check(&mut errors);
            t.policy.check(&mut errors);
            assert!(
                errors.into_result().is_ok(),
                "{}: capabilities or policy",
                t.id
            );
            assert_eq!(t.summary().id, t.id);
        }
        assert!(find("Analyst").is_some());
        assert!(find(" reviewer ").is_some());
        assert!(find("plumber").is_none());
    }

    #[test]
    fn questions_pass_on_any_expected_term() {
        let q = CompetencyQuestion {
            question: "What is TAM?".into(),
            expected: vec!["total addressable".into(), "TAM".into()],
        };
        assert!(q.passes("The Total Addressable Market is the whole demand."));
        assert!(q.passes("tam is a sizing term"));
        assert!(!q.passes("I do not know."));
    }

    #[test]
    fn topics_bind_by_term_and_answers_are_shortened() {
        let analyst = find("analyst").unwrap();
        let finance = Uuid::now_v7();
        let other = Uuid::now_v7();
        let topics = vec![
            (finance, vec!["finance".to_owned(), "invoices".to_owned()]),
            (other, vec!["kubernetes".to_owned()]),
        ];
        assert_eq!(analyst.bound_topics(&topics), Some(vec![finance]));
        assert_eq!(
            analyst.bound_topics(&[(other, vec!["kubernetes".to_owned()])]),
            None
        );
        let long = "x".repeat(ANSWER_EXCERPT_CHARS + 50);
        assert_eq!(excerpt(&long).chars().count(), ANSWER_EXCERPT_CHARS);
        assert_eq!(excerpt("  short  "), "short");
    }
}
