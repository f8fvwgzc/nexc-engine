//! The expert catalog (`docs/AGENT-CATALOG.md`): a new workspace gets the 30
//! experts once, an existing one gets them with the core team's prompts and
//! skills brought up to date (an edited prompt is kept), skills are listed,
//! validated and audited and reach the model when a node runs, the Planner's
//! status answers at its address and its former one, and an agent made from
//! a template is evaluated again on request.
#![forbid(unsafe_code)]

mod common;

use axum::http::{Method, StatusCode};
use common::{TestApp, collect_until};
use nexc::domain::agent::DEFAULT_ORG;
use nexc::domain::policy_document::{PolicyDocument, PolicyScope};
use nexc::domain::{agent_template, skill};
use nexc::planner::{self, CatalogSeeded};
use nexc::repo;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

const SEED_ON: &[(&str, &str)] = &[("NEXC_SEED_EXPERT_AGENTS", "true")];

async fn agents(app: &TestApp, token: &str) -> Vec<Value> {
    let r = app
        .request(Method::GET, "/api/v1/agents", Some(token), None)
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    r.body.as_array().unwrap().clone()
}

async fn workspace(app: &TestApp, token: &str) -> Uuid {
    let r = app
        .request(Method::GET, "/api/v1/workspaces", Some(token), None)
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    r.body[0]["id"].as_str().unwrap().parse().unwrap()
}

async fn user(pool: &PgPool, email: &str) -> Uuid {
    sqlx::query_scalar("SELECT id FROM users WHERE email = $1")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn audit(app: &TestApp, token: &str, wid: Uuid) -> Vec<Value> {
    let r = app
        .request(
            Method::GET,
            &format!("/api/v1/workspaces/{wid}/audit?limit=200"),
            Some(token),
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    r.body.as_array().unwrap().clone()
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_new_workspace_gets_the_expert_catalog_once(pool: PgPool) {
    let app = TestApp::new(pool.clone(), SEED_ON).await;
    let (token, _) = app.register("owner@example.com").await;
    let list = agents(&app, &token).await;
    assert_eq!(
        list.len(),
        DEFAULT_ORG.len() + agent_template::experts().count()
    );
    assert_eq!(agent_template::experts().count(), 30);
    let planner_id = list.iter().find(|a| a["role"] == "planner").unwrap()["id"].clone();
    for template in agent_template::experts() {
        let a = list
            .iter()
            .find(|a| a["role"] == template.role.as_str())
            .unwrap_or_else(|| panic!("{} is missing", template.id));
        assert_eq!(a["name"], template.name.as_str());
        assert_eq!(a["title"], template.title.as_str());
        assert_eq!(a["reports_to"], planner_id, "{}", template.id);
        assert_eq!(a["template_id"], template.id.as_str());
        assert_eq!(a["skills"], json!(template.skills));
        assert_eq!(a["status"], "active");
        assert!(
            a["capabilities"]["models"].is_null(),
            "{}: the allow-list stays open",
            template.id
        );
        assert_eq!(
            a["capabilities"]["tools"],
            json!(template.capabilities.tools)
        );
    }
    for seed in DEFAULT_ORG {
        let a = list.iter().find(|a| a["role"] == seed.role).unwrap();
        assert_eq!(a["system_prompt"], seed.system_prompt);
        assert_eq!(a["skills"], json!(seed.skills));
    }

    let wid = workspace(&app, &token).await;
    let owner = user(&pool, "owner@example.com").await;
    let entries = audit(&app, &token, wid).await;
    let created = entries
        .iter()
        .filter(|e| {
            e["action"] == "agent_created"
                && e["detail"]
                    .as_str()
                    .unwrap()
                    .contains("expert catalog experts-v1")
        })
        .count();
    assert_eq!(created, 30);
    let policies = entries
        .iter()
        .filter(|e| {
            e["action"] == "policy_changed"
                && e["detail"].as_str().unwrap().contains("from template")
        })
        .count();
    assert_eq!(policies, 30, "each expert runs under its template's policy");

    // The ~70 entries the seed wrote in one transaction all come back when the log is paged.
    let mut paged = Vec::new();
    let mut before: Option<String> = None;
    loop {
        let path = match &before {
            Some(at) => format!(
                "/api/v1/workspaces/{wid}/audit?limit=50&before={}",
                at.replace('+', "%2B")
            ),
            None => format!("/api/v1/workspaces/{wid}/audit?limit=50"),
        };
        let r = app.request(Method::GET, &path, Some(&token), None).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.body);
        let page = r.body.as_array().unwrap().clone();
        if page.is_empty() {
            break;
        }
        before = page.last().unwrap()["created_at"]
            .as_str()
            .map(str::to_owned);
        paged.extend(page);
    }
    assert_eq!(
        paged
            .iter()
            .filter(|e| e["action"] == "agent_created")
            .count(),
        DEFAULT_ORG.len() + 30,
        "paging by time skips nothing"
    );

    // Once: a second seed does nothing, and an expert deleted afterwards stays deleted.
    let mut conn = pool.acquire().await.unwrap();
    assert_eq!(
        planner::seed_catalog(&mut conn, owner, wid, "m")
            .await
            .unwrap(),
        None
    );
    let analyst = list.iter().find(|a| a["role"] == "analyst").unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let r = app
        .request(
            Method::DELETE,
            &format!("/api/v1/agents/{analyst}"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{}", r.body);
    planner::seed_catalog_everywhere(&app.state).await.unwrap();
    assert_eq!(agents(&app, &token).await.len(), list.len() - 1);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn an_existing_workspace_gets_the_catalog_and_keeps_edited_prompts(pool: PgPool) {
    let app = TestApp::new(pool.clone(), &[]).await;
    let (token, _) = app.register("owner@example.com").await;
    assert_eq!(agents(&app, &token).await.len(), DEFAULT_ORG.len());
    let wid = workspace(&app, &token).await;
    let owner = user(&pool, "owner@example.com").await;
    // As an earlier release left them: the former prompts and no skills; the writer's prompt edited.
    for seed in DEFAULT_ORG {
        let prompt = if seed.role == "writer" {
            "My own writer prompt."
        } else {
            seed.former_prompts[0]
        };
        sqlx::query("UPDATE agents SET skills = '{}', system_prompt = $2 WHERE workspace_id = $1 AND role = $3")
            .bind(wid)
            .bind(prompt)
            .bind(seed.role)
            .execute(&pool)
            .await
            .unwrap();
    }
    // An agent made from a template before skills existed.
    let r = app
        .request(
            Method::POST,
            "/api/v1/agents/from-template/risk-manager",
            Some(&token),
            Some(json!({"evaluate": false})),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    let risk = r.body["agent"]["id"].as_str().unwrap().to_owned();
    sqlx::query("UPDATE agents SET skills = '{}' WHERE id = $1::uuid")
        .bind(&risk)
        .execute(&pool)
        .await
        .unwrap();
    // A hardened workspace: its experts' policies leave out what would widen it.
    let hardened: PolicyDocument = serde_json::from_value(json!({
        "injection": {"action": "block"}, "alerts": {"risk_score": 4}
    }))
    .unwrap();
    repo::policies::insert(&pool, wid, PolicyScope::Workspace, None, &hardened, owner)
        .await
        .unwrap();
    // A name the catalog would use is taken: that expert is skipped.
    let r = app
        .request(
            Method::POST,
            "/api/v1/agents",
            Some(&token),
            Some(json!({"name": "Financial Analyst", "role": "money"})),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);

    let mut tx = pool.begin().await.unwrap();
    let seeded = planner::seed_catalog(&mut tx, owner, wid, "m")
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        seeded,
        Some(CatalogSeeded {
            created: 28,
            upgraded: 6,
            skipped: 2
        })
    );

    let list = agents(&app, &token).await;
    assert_eq!(list.len(), DEFAULT_ORG.len() + 2 + 28);
    let risk = list.iter().find(|a| a["id"] == risk.as_str()).unwrap();
    assert_eq!(
        risk["skills"],
        json!(agent_template::find("risk-manager").unwrap().skills)
    );
    // Each expert has its policy, narrowed where the workspace is stricter, and the audit says so.
    let architect = list
        .iter()
        .find(|a| a["role"] == "cloud-architect")
        .unwrap();
    let r = app
        .request(
            Method::GET,
            &format!(
                "/api/v1/workspaces/{wid}/policies?scope=agent&scope_id={}",
                architect["id"].as_str().unwrap()
            ),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let document = &r.body[0]["document"];
    assert!(document["injection"]["action"].is_null(), "{document}");
    assert!(document["alerts"]["risk_score"].is_null(), "{document}");
    assert_eq!(
        document["alerts"]["quarantine_risk_score"].as_f64(),
        Some(8.0),
        "the rest is kept: {document}"
    );
    let narrowed = audit(&app, &token, wid)
        .await
        .into_iter()
        .filter(|e| {
            e["action"] == "policy_changed"
                && e["detail"]
                    .as_str()
                    .unwrap()
                    .contains("left out, the workspace's stricter rule applies")
        })
        .count();
    assert_eq!(narrowed, 28);
    for seed in DEFAULT_ORG {
        let a = list.iter().find(|a| a["role"] == seed.role).unwrap();
        assert_eq!(a["skills"], json!(seed.skills), "{}", seed.role);
        if seed.role == "writer" {
            assert_eq!(
                a["system_prompt"], "My own writer prompt.",
                "an edited prompt is kept"
            );
        } else {
            assert_eq!(a["system_prompt"], seed.system_prompt, "{}", seed.role);
            assert_eq!(a["purpose"], seed.purpose);
        }
    }
    assert!(
        list.iter().all(|a| a["role"] != "analyst"),
        "the analyst's name was taken"
    );
    let upgrades = audit(&app, &token, wid)
        .await
        .into_iter()
        .filter(|e| e["action"] == "agent_skills_changed")
        .count();
    assert_eq!(upgrades, 6);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_skill_library_is_listed_and_read(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let (token, _) = app.register("reader@example.com").await;
    let r = app
        .request(Method::GET, "/api/v1/skills", Some(&token), None)
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let all = r.body.as_array().unwrap();
    assert_eq!(all.len(), skill::library().len());
    assert!(all.len() >= 140, "the library holds the catalog's skills");
    let first = &all[0];
    for key in [
        "id",
        "name",
        "category",
        "description",
        "keywords",
        "sources",
        "chars",
    ] {
        assert!(!first[key].is_null(), "{key}");
    }
    assert!(
        first.get("body").is_none(),
        "the list leaves the bodies out"
    );

    let r = app
        .request(
            Method::GET,
            "/api/v1/skills?category=finance",
            Some(&token),
            None,
        )
        .await;
    let finance = r.body.as_array().unwrap();
    assert!(!finance.is_empty() && finance.iter().all(|s| s["category"] == "finance"));

    let r = app
        .request(
            Method::GET,
            "/api/v1/skills/FIN-EQUITY-VALUATION",
            Some(&token),
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["id"], "fin-equity-valuation");
    assert!(r.body["body"].as_str().unwrap().contains("## "));
    assert!(
        r.body["sources"][0]["url"]
            .as_str()
            .unwrap()
            .starts_with("http")
    );

    let r = app
        .request(Method::GET, "/api/v1/skills/nope", Some(&token), None)
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    let r = app.request(Method::GET, "/api/v1/skills", None, None).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn an_agents_skills_are_validated_and_audited(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let (token, _) = app.register("owner@example.com").await;
    let r = app
        .request(
            Method::POST,
            "/api/v1/agents",
            Some(&token),
            Some(json!({"name": "Valuer", "role": "valuer",
                        "skills": ["fin-equity-valuation", " FIN-FIXED-INCOME ", "fin-equity-valuation"]})),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    assert_eq!(
        r.body["skills"],
        json!(["fin-equity-valuation", "fin-fixed-income"])
    );
    assert!(r.body["template_id"].is_null());
    let id = r.body["id"].as_str().unwrap().to_owned();

    for skills in [
        json!(["no-such-skill"]),
        json!(
            skill::library()
                .iter()
                .take(13)
                .map(|s| s.id.clone())
                .collect::<Vec<_>>()
        ),
    ] {
        let r = app
            .request(
                Method::POST,
                "/api/v1/agents",
                Some(&token),
                Some(json!({"name": "Bad", "role": "bad", "skills": skills})),
            )
            .await;
        assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{}", r.body);
        assert!(r.body.to_string().contains("skills"), "{}", r.body);
    }

    let r = app
        .request(
            Method::PATCH,
            &format!("/api/v1/agents/{id}"),
            Some(&token),
            Some(json!({"skills": ["fin-fixed-income", "core-quantitative-reasoning"]})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(
        r.body["skills"],
        json!(["fin-fixed-income", "core-quantitative-reasoning"])
    );
    let r = app
        .request(
            Method::PATCH,
            &format!("/api/v1/agents/{id}"),
            Some(&token),
            Some(json!({"title": "Bond desk"})),
        )
        .await;
    assert_eq!(
        r.body["skills"],
        json!(["fin-fixed-income", "core-quantitative-reasoning"]),
        "kept"
    );

    let wid = workspace(&app, &token).await;
    let changes: Vec<String> = audit(&app, &token, wid)
        .await
        .into_iter()
        .filter(|e| e["action"] == "agent_skills_changed")
        .map(|e| e["detail"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        changes,
        vec!["added core-quantitative-reasoning; removed fin-equity-valuation".to_owned()]
    );
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_node_runs_with_the_skills_that_fit_it(pool: PgPool) {
    let app = TestApp::new(pool, SEED_ON).await;
    let (token, _) = app.register("owner@example.com").await;
    let gid = app.graph(&token, "Value Acme Robotics").await;
    let node = app
        .node(
            &token,
            &gid,
            json!({"title": "Value Acme with a DCF", "kind": "task", "agent_role": "analyst", "executor": "llm",
                   "content": "Build a DCF: estimate the WACC, project free cash flow to the firm and check the terminal value."}),
        )
        .await;
    let mut rx = app.events(&gid);
    let r = app
        .request(
            Method::POST,
            &format!("/api/v1/graphs/{gid}/runs"),
            Some(&token),
            Some(json!({})),
        )
        .await;
    assert_eq!(r.status, StatusCode::ACCEPTED, "{}", r.body);
    let events = collect_until(&mut rx, "run.finished").await;
    let logged: Vec<&str> = events
        .iter()
        .filter(|(e, d)| e == "node.log" && d["node_id"] == node.as_str())
        .filter_map(|(_, d)| d["message"].as_str())
        .collect();
    let applied = logged
        .iter()
        .find(|m| m.starts_with("skills applied:"))
        .unwrap_or_else(|| panic!("no skills line in {logged:?}"));
    let valuation = skill::find("fin-equity-valuation").unwrap();
    assert!(
        applied.starts_with("skills applied: fin-equity-valuation"),
        "most relevant first: {applied}"
    );

    let analyst = agent_template::find("analyst").unwrap();
    let systems = app.fake.systems.lock().unwrap().clone();
    let system = systems
        .iter()
        .find(|s| s.starts_with(analyst.system_prompt.trim()))
        .expect("the node ran with the analyst's prompt");
    assert!(system.contains("## Your skills"));
    assert!(system.contains(&format!("### {}", valuation.name)));
    assert!(
        system.chars().count()
            <= analyst.system_prompt.chars().count() + skill::PROMPT_BUDGET_CHARS + 2_000
    );
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_planner_status_answers_at_its_address_and_its_former_one(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let (token, _) = app.register("owner@example.com").await;
    let now = app
        .request(Method::GET, "/api/v1/planner/status", Some(&token), None)
        .await;
    assert_eq!(now.status, StatusCode::OK, "{}", now.body);
    let before = app
        .request(
            Method::GET,
            "/api/v1/orchestrator/status",
            Some(&token),
            None,
        )
        .await;
    assert_eq!(before.status, StatusCode::OK, "{}", before.body);
    for key in [
        "demo_mode",
        "queue_depth",
        "running_nodes",
        "active_runs",
        "agents_active",
        "backends",
    ] {
        assert_eq!(now.body[key], before.body[key], "{key}");
    }
    let spec = nexc::http::openapi();
    let spec = serde_json::to_value(&spec).unwrap();
    assert!(spec["paths"]["/api/v1/planner/status"]["get"].is_object());
    assert_eq!(
        spec["paths"]["/api/v1/orchestrator/status"]["get"]["deprecated"],
        true
    );
    assert!(spec["paths"]["/api/v1/skills"]["get"].is_object());
    assert!(spec["paths"]["/api/v1/agents/{id}/evaluate"]["post"].is_object());
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn an_agent_made_from_a_template_is_evaluated_again(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let (token, _) = app.register("owner@example.com").await;
    let r = app
        .request(
            Method::POST,
            "/api/v1/agents/from-template/analyst",
            Some(&token),
            Some(json!({"evaluate": false})),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    assert!(r.body["evaluation"].is_null());
    assert_eq!(r.body["agent"]["template_id"], "analyst");
    let analyst = agent_template::find("analyst").unwrap();
    assert_eq!(r.body["agent"]["skills"], json!(analyst.skills));
    let id = r.body["agent"]["id"].as_str().unwrap().to_owned();

    let r = app
        .request(
            Method::POST,
            &format!("/api/v1/agents/{id}/evaluate"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["template"], "analyst");
    assert_eq!(r.body["total"], 5);
    assert_eq!(r.body["questions"].as_array().unwrap().len(), 5);
    let card = app
        .request(
            Method::GET,
            &format!("/api/v1/agents/{id}"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(card.body["evaluation"]["total"], 5);

    let planner = agents(&app, &token)
        .await
        .into_iter()
        .find(|a| a["role"] == "planner")
        .unwrap();
    let r = app
        .request(
            Method::POST,
            &format!(
                "/api/v1/agents/{}/evaluate",
                planner["id"].as_str().unwrap()
            ),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{}", r.body);
    let r = app
        .request(
            Method::POST,
            &format!("/api/v1/agents/{}/evaluate", Uuid::now_v7()),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
}
