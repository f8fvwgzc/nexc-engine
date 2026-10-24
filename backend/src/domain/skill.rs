//! Skills: the professional working knowledge an agent brings to a step,
//! embedded from `backend/skills/*.md` (`docs/AGENT-CATALOG.md`).
//!
//! A skill file is a small frontmatter (`id`, `name`, `category`,
//! `description`, `keywords`, one `source:` line per public source as
//! `title | url`) between `---` lines, then Markdown. An agent lists the ids of
//! its skills; when it runs a step, [`compose`] adds to its system prompt the
//! skills that bear most on the step, in full while they fit a budget and as a
//! one-line summary after that, plus at most two skills of the shared library
//! that match the step strongly even though the agent was not given them.

use std::collections::HashSet;
use std::sync::OnceLock;

use serde::Serialize;
use utoipa::ToSchema;

use super::catalog::SKILL_SOURCES;
use super::validation::FieldErrors;

/// Most skills an agent may be given.
pub const SKILLS_PER_AGENT_MAX: usize = 12;
/// Most characters of skills added to a system prompt (about 3,500 tokens).
pub const PROMPT_BUDGET_CHARS: usize = 14_000;
/// Most skills of the shared library added to a step the agent was not given.
pub const LIBRARY_EXTRAS_MAX: usize = 2;
/// How strongly a library skill must match a step to be added (see
/// `Focus::score`: 3 points for a keyword at most two skills list, 2 for one
/// up to five list, 1 for a common one, 1 per word of its name and description
/// the step uses); it must also match at least one of those rare keywords.
const LIBRARY_EXTRA_MIN_SCORE: u32 = 6;
/// Most characters of a system prompt with its skills: the agent runtime
/// refuses more than 32,000, and adds sections of its own.
pub const SYSTEM_PROMPT_TOTAL_MAX: usize = 30_000;
/// Characters of the step considered when ranking skills.
const FOCUS_CHARS: usize = 6_000;

/// A public source a skill was written from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct SkillSource {
    pub title: String,
    pub url: String,
}

/// A skill with its working knowledge.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Skill {
    /// Its id in agents' skill lists (`fin-equity-valuation`).
    pub id: String,
    pub name: String,
    /// The field it belongs to (`finance`, `shipping`, ...).
    pub category: String,
    /// What it does and when it applies, in a sentence.
    pub description: String,
    /// Terms a step that needs it tends to use (matched to rank skills).
    pub keywords: Vec<String>,
    /// The public pages it was written from.
    pub sources: Vec<SkillSource>,
    /// The working knowledge, in Markdown.
    pub body: String,
}

/// A skill as listed by `GET /skills`, without its body.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SkillSummary {
    pub id: String,
    pub name: String,
    pub category: String,
    pub description: String,
    pub keywords: Vec<String>,
    pub sources: Vec<SkillSource>,
    /// Characters of its body: what it costs in a prompt.
    pub chars: usize,
}

impl Skill {
    /// The list view of this skill.
    pub fn summary(&self) -> SkillSummary {
        SkillSummary {
            id: self.id.clone(),
            name: self.name.clone(),
            category: self.category.clone(),
            description: self.description.clone(),
            keywords: self.keywords.clone(),
            sources: self.sources.clone(),
            chars: self.body.chars().count(),
        }
    }
}

/// Whether `id` is a valid skill id: lowercase ASCII words joined by `-`.
pub fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.split('-').all(|w| {
            !w.is_empty()
                && w.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

/// Parses a skill file named `file`.
pub fn parse(file: &str, src: &str) -> Result<Skill, String> {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    let mut lines = src.lines();
    if lines.next().map(str::trim) != Some("---") {
        return Err("the first line must be ---".into());
    }
    let (mut id, mut name, mut category, mut description, mut keywords) =
        (None, None, None, None, None);
    let mut sources = Vec::new();
    let mut closed = false;
    for line in lines.by_ref() {
        if line.trim() == "---" {
            closed = true;
            break;
        }
        if line.trim().is_empty() {
            continue;
        }
        let (key, value) = line
            .split_once(':')
            .ok_or_else(|| format!("frontmatter line without a key: {line:?}"))?;
        let value = value.trim().to_owned();
        let slot = match key.trim() {
            "id" => &mut id,
            "name" => &mut name,
            "category" => &mut category,
            "description" => &mut description,
            "keywords" => &mut keywords,
            "source" => {
                let (title, url) = value
                    .rsplit_once(" | ")
                    .ok_or_else(|| format!("a source is `title | url`: {value:?}"))?;
                let (title, url) = (title.trim(), url.trim());
                if title.is_empty() || !(url.starts_with("https://") || url.starts_with("http://"))
                {
                    return Err(format!(
                        "a source needs a title and an http(s) URL: {value:?}"
                    ));
                }
                sources.push(SkillSource {
                    title: title.into(),
                    url: url.into(),
                });
                continue;
            }
            other => return Err(format!("unknown frontmatter key {other:?}")),
        };
        if slot.replace(value).is_some() {
            return Err(format!("duplicate frontmatter key {:?}", key.trim()));
        }
    }
    if !closed {
        return Err("the frontmatter is not closed with ---".into());
    }
    let required = |v: Option<String>, key: &str| {
        v.filter(|v| !v.is_empty())
            .ok_or_else(|| format!("missing {key}"))
    };
    let id = required(id, "id")?;
    let expected = file.strip_suffix(".md").unwrap_or(file);
    if id != expected {
        return Err(format!("id {id:?} must be the file name {expected:?}"));
    }
    if !is_valid_id(&id) {
        return Err(format!("id {id:?} is not lowercase kebab-case"));
    }
    let keywords: Vec<String> = required(keywords, "keywords")?
        .split(',')
        .map(|k| k.trim().to_lowercase())
        .filter(|k| !k.is_empty())
        .collect();
    let body: String = lines.collect::<Vec<_>>().join("\n").trim().to_owned();
    if body.is_empty() {
        return Err("the body is empty".into());
    }
    if sources.is_empty() {
        return Err("a skill names at least one source".into());
    }
    Ok(Skill {
        id,
        name: required(name, "name")?,
        category: required(category, "category")?,
        description: required(description, "description")?,
        keywords,
        sources,
        body,
    })
}

/// Every skill of the library, in id order (parsed once; the files are
/// validated by tests).
pub fn library() -> &'static [Skill] {
    static LIBRARY: OnceLock<Vec<Skill>> = OnceLock::new();
    LIBRARY.get_or_init(|| {
        let mut skills: Vec<Skill> = SKILL_SOURCES
            .iter()
            .map(|(file, src)| {
                parse(file, src)
                    .unwrap_or_else(|e| panic!("skill {file}: {e} (validated by tests)"))
            })
            .collect();
        skills.sort_by(|a, b| a.id.cmp(&b.id));
        skills
    })
}

/// A skill by id.
pub fn find(id: &str) -> Option<&'static Skill> {
    let id = id.trim();
    library()
        .binary_search_by(|s| s.id.as_str().cmp(id))
        .ok()
        .map(|i| &library()[i])
}

/// `ids` trimmed and lowercased, without duplicates, in their order.
pub fn normalize(ids: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    ids.iter()
        .map(|id| id.trim().to_lowercase())
        .filter(|id| !id.is_empty() && seen.insert(id.clone()))
        .collect()
}

/// Checks the skills an agent is given: known ids, at most
/// [`SKILLS_PER_AGENT_MAX`].
pub fn check(errors: &mut FieldErrors, field: &str, ids: &[String]) {
    let ids = normalize(ids);
    if ids.len() > SKILLS_PER_AGENT_MAX {
        errors.add(field, format!("at most {SKILLS_PER_AGENT_MAX} skills"));
    }
    let unknown: Vec<&str> = ids
        .iter()
        .filter(|id| find(id).is_none())
        .map(String::as_str)
        .collect();
    if !unknown.is_empty() {
        errors.add(field, format!("unknown skills: {}", unknown.join(", ")));
    }
}

/// The audit detail of a change from `before` to `after`: what was added and
/// removed, or that only the order changed (the order ranks equal matches).
pub fn describe_change(before: &[String], after: &[String]) -> String {
    let added: Vec<&str> = after
        .iter()
        .filter(|s| !before.contains(s))
        .map(String::as_str)
        .collect();
    let removed: Vec<&str> = before
        .iter()
        .filter(|s| !after.contains(s))
        .map(String::as_str)
        .collect();
    let mut parts = Vec::new();
    if !added.is_empty() {
        parts.push(format!("added {}", added.join(", ")));
    }
    if !removed.is_empty() {
        parts.push(format!("removed {}", removed.join(", ")));
    }
    if parts.is_empty() {
        "reordered".into()
    } else {
        parts.join("; ")
    }
}

/// What composing a prompt added.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Applied {
    /// The agent's skills given in full, most relevant first.
    pub full: Vec<String>,
    /// The agent's skills given as a one-line summary (the budget was spent).
    pub summarised: Vec<String>,
    /// Skills of the shared library added because the step matched them.
    pub library: Vec<String>,
}

impl Applied {
    /// True when nothing was added.
    pub fn is_empty(&self) -> bool {
        self.full.is_empty() && self.summarised.is_empty() && self.library.is_empty()
    }

    /// One line for the node's log.
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if !self.full.is_empty() {
            parts.push(format!("skills applied: {}", self.full.join(", ")));
        }
        if !self.library.is_empty() {
            parts.push(format!("from the library: {}", self.library.join(", ")));
        }
        if !self.summarised.is_empty() {
            parts.push(format!("summarised: {}", self.summarised.join(", ")));
        }
        parts.join("; ")
    }
}

/// `base` with the skills that bear on `focus` (the step): the agent's
/// `assigned` skills and up to [`LIBRARY_EXTRAS_MAX`] library skills the step
/// matches strongly, most relevant first, in full within
/// [`PROMPT_BUDGET_CHARS`]; assigned skills that match nothing come after the
/// relevant ones, and those that do not fit are summarised in a line. The
/// whole prompt stays within [`SYSTEM_PROMPT_TOTAL_MAX`]. Unknown ids are
/// skipped. Without skills, `base` as it is.
pub fn compose(base: &str, assigned: &[String], focus: &str) -> (String, Applied) {
    let assigned: Vec<&Skill> = normalize(assigned)
        .iter()
        .filter_map(|id| find(id))
        .collect();
    compose_with(base, &assigned, library(), focus, PROMPT_BUDGET_CHARS)
}

/// Opens the skills section of a prompt.
const SKILLS_HEADING: &str = "\n\n## Your skills\n\nThe skills below are your working knowledge. \
    Apply the ones that fit this step; the step, its upstream results and the instructions above \
    decide what to do.";
/// Opens the one-line summaries of the skills that did not fit.
const SUMMARY_HEADING: &str = "\n\n### Other skills you have\n";
/// Follows the name of a library skill the agent was not given.
const LIBRARY_SUFFIX: &str = " (shared skill library)";

/// A skill considered for a prompt.
struct Pick<'a> {
    skill: &'a Skill,
    score: u32,
    /// Its place in the agent's list; library skills come after.
    order: usize,
    given: bool,
}

impl Pick<'_> {
    fn block(&self) -> String {
        let suffix = if self.given { "" } else { LIBRARY_SUFFIX };
        format!(
            "\n\n### {}{suffix}\n\n{}",
            self.skill.name,
            self.skill.body.trim()
        )
    }

    fn line(&self) -> String {
        format!("\n- **{}**: {}", self.skill.name, self.skill.description)
    }
}

fn compose_with(
    base: &str,
    assigned: &[&Skill],
    library: &[Skill],
    focus: &str,
    budget: usize,
) -> (String, Applied) {
    let base = base.trim_end();
    let focus: String = focus.chars().take(FOCUS_CHARS).collect();
    let focus = Focus::new(&focus, library);
    let picks: Vec<Pick> = assigned
        .iter()
        .enumerate()
        .map(|(order, skill)| Pick {
            skill,
            score: focus.score(skill),
            order,
            given: true,
        })
        .collect();
    let mut extras: Vec<Pick> = Vec::new();
    if !focus.is_empty() {
        let given: HashSet<&str> = assigned.iter().map(|s| s.id.as_str()).collect();
        let mut matches: Vec<Pick> = library
            .iter()
            .filter(|s| !given.contains(s.id.as_str()) && focus.has_rare_keyword(s))
            .map(|skill| Pick {
                skill,
                score: focus.score(skill),
                order: usize::MAX,
                given: false,
            })
            .filter(|p| p.score >= LIBRARY_EXTRA_MIN_SCORE)
            .collect();
        matches.sort_by(|a, b| b.score.cmp(&a.score).then(a.skill.id.cmp(&b.skill.id)));
        matches.truncate(LIBRARY_EXTRAS_MAX);
        extras = matches;
    }
    // In full first: the relevant skills, the agent's own before the library's on a tie; then
    // the agent's skills that match nothing (all of them, in its order, when there is no step).
    let relevant = |p: &&Pick| focus.is_empty() || p.score > 0;
    let mut queue: Vec<&Pick> = picks.iter().filter(relevant).chain(extras.iter()).collect();
    queue.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then(b.given.cmp(&a.given))
            .then(a.order.cmp(&b.order))
    });
    queue.extend(picks.iter().filter(|p| !relevant(p)));

    let room = budget.min(SYSTEM_PROMPT_TOTAL_MAX.saturating_sub(base.chars().count()));
    let mut used = SKILLS_HEADING.chars().count();
    let (mut full, mut left): (Vec<&Pick>, Vec<&Pick>) = (Vec::new(), Vec::new());
    if used < room {
        for pick in queue {
            let cost = pick.block().chars().count();
            if used + cost <= room {
                used += cost;
                full.push(pick);
            } else if pick.given {
                left.push(pick);
            }
        }
    } else {
        left = queue.into_iter().filter(|p| p.given).collect();
    }
    let mut summarised = Vec::new();
    if !full.is_empty() && !left.is_empty() {
        used += SUMMARY_HEADING.chars().count();
        for pick in left {
            let cost = pick.line().chars().count();
            if used + cost <= room {
                used += cost;
                summarised.push(pick);
            }
        }
    }
    let applied = Applied {
        full: full
            .iter()
            .filter(|p| p.given)
            .map(|p| p.skill.id.clone())
            .collect(),
        summarised: summarised.iter().map(|p| p.skill.id.clone()).collect(),
        library: full
            .iter()
            .filter(|p| !p.given)
            .map(|p| p.skill.id.clone())
            .collect(),
    };
    if full.is_empty() {
        return (base.to_owned(), Applied::default());
    }
    let mut prompt = String::with_capacity(base.len() + used * 2);
    prompt.push_str(base);
    prompt.push_str(SKILLS_HEADING);
    for pick in &full {
        prompt.push_str(&pick.block());
    }
    if !summarised.is_empty() {
        prompt.push_str(SUMMARY_HEADING);
        for pick in &summarised {
            prompt.push_str(&pick.line());
        }
    }
    (prompt, applied)
}

/// Words that say nothing about which skill a step needs.
const STOPWORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "for", "from", "has", "have", "how", "in",
    "into", "is", "it", "its", "of", "on", "or", "that", "the", "their", "this", "to", "use",
    "uses", "used", "using", "was", "what", "when", "which", "with", "work", "step", "steps",
];

/// Lowercase words of `text` (letters and digits), without stopwords.
fn words(text: &str) -> HashSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 2)
        .map(str::to_lowercase)
        .filter(|w| !STOPWORDS.contains(&w.as_str()))
        .collect()
}

/// `text` lowercased with every run of non-alphanumeric characters as one
/// space, padded with spaces so that phrases match on word boundaries.
fn padded(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push(' ');
    let mut space = true;
    for c in text.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            out.push(c);
            space = false;
        } else if !space {
            out.push(' ');
            space = true;
        }
    }
    if !space {
        out.push(' ');
    }
    out
}

/// The step, prepared for ranking skills against a library.
struct Focus {
    text: String,
    words: HashSet<String>,
    /// How many skills of the library list each keyword.
    spread: std::collections::HashMap<String, usize>,
}

impl Focus {
    fn new(text: &str, library: &[Skill]) -> Self {
        let mut spread = std::collections::HashMap::new();
        for skill in library {
            let mine: HashSet<String> = skill.keywords.iter().map(|k| padded(k)).collect();
            for keyword in mine {
                *spread.entry(keyword).or_insert(0) += 1;
            }
        }
        Focus {
            text: padded(text),
            words: words(text),
            spread,
        }
    }

    fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// The keywords of `skill` the step uses, each with its weight: 3 when at
    /// most two skills of the library list it, 2 when at most five do, 1 when
    /// more do.
    fn keyword_hits<'a>(&'a self, skill: &'a Skill) -> impl Iterator<Item = u32> + 'a {
        skill
            .keywords
            .iter()
            .map(|k| padded(k))
            .filter(|k| k.trim().chars().count() >= 2 && self.text.contains(k.as_str()))
            .map(|k| match self.spread.get(&k).copied().unwrap_or(1) {
                0..=2 => 3,
                3..=5 => 2,
                _ => 1,
            })
    }

    /// Whether the step uses a keyword few skills share (weight 3).
    fn has_rare_keyword(&self, skill: &Skill) -> bool {
        !self.is_empty() && self.keyword_hits(skill).any(|w| w == 3)
    }

    /// The weights of the keywords the step uses, plus one per word of the
    /// skill's name and description it uses.
    fn score(&self, skill: &Skill) -> u32 {
        if self.is_empty() {
            return 0;
        }
        let keywords: u32 = self.keyword_hits(skill).sum();
        let mut described = words(&skill.name);
        described.extend(words(&skill.description));
        let shared = described.iter().filter(|w| self.words.contains(*w)).count() as u32;
        keywords + shared
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(id: &str, keywords: &str, body: &str) -> Skill {
        Skill {
            id: id.into(),
            name: id.replace('-', " "),
            category: "test".into(),
            description: format!("Does {id} work."),
            keywords: keywords.split(',').map(|k| k.trim().to_owned()).collect(),
            sources: vec![],
            body: body.into(),
        }
    }

    #[test]
    fn parses_a_skill_file() {
        let src = "---\nid: fin-x\nname: X\ncategory: finance\ndescription: Values things.\n\
                   keywords: dcf, WACC , terminal value\nsource: CFA Institute | https://www.cfainstitute.org\n\
                   source: Damodaran | https://pages.stern.nyu.edu/~adamodar/\n---\n## Method\n\nDo it.\n";
        let s = parse("fin-x.md", src).unwrap();
        assert_eq!(s.id, "fin-x");
        assert_eq!(s.keywords, vec!["dcf", "wacc", "terminal value"]);
        assert_eq!(s.sources.len(), 2);
        assert_eq!(s.sources[1].url, "https://pages.stern.nyu.edu/~adamodar/");
        assert_eq!(s.body, "## Method\n\nDo it.");
    }

    #[test]
    fn rejects_malformed_files() {
        let ok = "---\nid: a-b\nname: N\ncategory: c\ndescription: d\nkeywords: k\nsource: T | https://x.org\n---\nbody";
        assert!(parse("a-b.md", ok).is_ok());
        assert!(parse("other.md", ok).unwrap_err().contains("file name"));
        assert!(parse("a-b.md", &ok.replace("---\nbody", "body")).is_err());
        assert!(
            parse(
                "a-b.md",
                &ok.replace("source: T | https://x.org", "source: T")
            )
            .is_err()
        );
        assert!(parse("a-b.md", &ok.replace("name: N", "name: N\nname: M")).is_err());
        assert!(parse("a-b.md", &ok.replace("name: N", "colour: red")).is_err());
        assert!(parse("a-b.md", &ok.replace("---\nbody", "---\n")).is_err());
        assert!(!is_valid_id("A-b") && !is_valid_id("a--b") && is_valid_id("iso-9001"));
    }

    #[test]
    fn the_library_is_well_formed() {
        let lib = library();
        assert!(!lib.is_empty(), "backend/skills holds the skill library");
        let mut ids = HashSet::new();
        for s in lib {
            assert!(ids.insert(s.id.as_str()), "duplicate skill {}", s.id);
            assert!(s.name.chars().count() <= 60, "{}: name", s.id);
            let desc = s.description.chars().count();
            assert!(
                (40..=240).contains(&desc),
                "{}: description of {desc} chars",
                s.id
            );
            assert!((5..=16).contains(&s.keywords.len()), "{}: keywords", s.id);
            assert!((1..=5).contains(&s.sources.len()), "{}: sources", s.id);
            let body = s.body.chars().count();
            assert!(
                (1_500..=3_800).contains(&body),
                "{}: body of {body} chars",
                s.id
            );
            assert!(
                s.body.matches("\n## ").count() + usize::from(s.body.starts_with("## ")) >= 3,
                "{}: sections",
                s.id
            );
            assert_eq!(find(&s.id).map(|f| &f.id), Some(&s.id));
        }
        assert!(find("no-such-skill").is_none());
    }

    #[test]
    fn checks_agent_skill_lists() {
        let known = library()[0].id.clone();
        let mut errors = FieldErrors::default();
        check(
            &mut errors,
            "skills",
            &[known.clone(), format!(" {} ", known.to_uppercase())],
        );
        assert!(errors.into_result().is_ok());
        let mut errors = FieldErrors::default();
        check(&mut errors, "skills", &["no-such-skill".into()]);
        assert!(errors.into_result().is_err());
        let many: Vec<String> = library()
            .iter()
            .take(SKILLS_PER_AGENT_MAX + 1)
            .map(|s| s.id.clone())
            .collect();
        if many.len() > SKILLS_PER_AGENT_MAX {
            let mut errors = FieldErrors::default();
            check(&mut errors, "skills", &many);
            assert!(errors.into_result().is_err());
        }
    }

    #[test]
    fn ranks_by_relevance_and_keeps_to_the_budget() {
        let dcf = skill(
            "equity-valuation",
            "dcf, wacc, terminal value",
            &"v".repeat(500),
        );
        let bonds = skill(
            "fixed-income",
            "duration, convexity, yield",
            &"b".repeat(500),
        );
        let ships = skill(
            "chartering",
            "laytime, demurrage, charter party",
            &"s".repeat(500),
        );
        let lib = vec![dcf.clone(), bonds.clone(), ships.clone()];
        let focus =
            "Build a DCF with a WACC of 8% and check the terminal value; note duration risk.";

        let (prompt, applied) = compose_with("Base.", &[&bonds, &dcf], &lib, focus, 10_000);
        assert_eq!(applied.full, vec!["equity-valuation", "fixed-income"]);
        assert!(applied.library.is_empty(), "chartering does not match");
        assert!(prompt.starts_with("Base.\n\n## Your skills"));
        assert!(
            prompt.find("### equity valuation").unwrap() < prompt.find("### fixed income").unwrap()
        );

        // The heading and the summary line count against the budget too.
        let budget = SKILLS_HEADING.chars().count() + 540 + SUMMARY_HEADING.chars().count() + 60;
        let (prompt, applied) = compose_with("Base.", &[&bonds, &dcf], &lib, focus, budget);
        assert_eq!(applied.full, vec!["equity-valuation"]);
        assert_eq!(applied.summarised, vec!["fixed-income"]);
        assert!(
            prompt.contains(
                "### Other skills you have\n\n- **fixed income**: Does fixed-income work."
            )
        );
        assert!(prompt.chars().count() <= "Base.".len() + budget);

        let (prompt, applied) = compose_with("Base.", &[], &lib, "", 10_000);
        assert!(applied.is_empty());
        assert_eq!(prompt, "Base.");
        // Without a step, the agent's skills in its order.
        let (_, applied) = compose_with("Base.", &[&bonds, &dcf], &lib, "", 10_000);
        assert_eq!(applied.full, vec!["fixed-income", "equity-valuation"]);
    }

    #[test]
    fn a_strong_library_skill_comes_before_given_skills_that_match_nothing() {
        // Four of the agent's skills fill the budget but none bears on the step.
        let given: Vec<Skill> = (0..4)
            .map(|i| skill(&format!("finance-{i}"), "dcf, wacc", &"f".repeat(3_600)))
            .collect();
        let ships = skill(
            "chartering",
            "laytime, demurrage, charter party",
            &"s".repeat(3_600),
        );
        let mut lib = given.clone();
        lib.push(ships.clone());
        let refs: Vec<&Skill> = given.iter().collect();
        let (prompt, applied) = compose_with(
            "Base.",
            &refs,
            &lib,
            "Calculate laytime and demurrage under the voyage charter party.",
            PROMPT_BUDGET_CHARS,
        );
        assert_eq!(applied.library, vec!["chartering"]);
        assert_eq!(
            applied.full.len(),
            2,
            "what is left goes to the agent's own skills"
        );
        assert_eq!(applied.summarised.len(), 2);
        assert!(
            prompt
                .find("### chartering (shared skill library)")
                .unwrap()
                < prompt.find("### finance 0").unwrap()
        );
    }

    #[test]
    fn the_whole_prompt_stays_under_the_runtimes_limit() {
        let given: Vec<Skill> = (0..12)
            .map(|i| skill(&format!("s-{i}"), "dcf", &"x".repeat(3_700)))
            .collect();
        let refs: Vec<&Skill> = given.iter().collect();
        let base = "p".repeat(16 * 1024);
        let (prompt, applied) =
            compose_with(&base, &refs, &given, "A DCF please.", PROMPT_BUDGET_CHARS);
        assert!(
            prompt.chars().count() <= SYSTEM_PROMPT_TOTAL_MAX,
            "{}",
            prompt.chars().count()
        );
        assert!(!applied.full.is_empty());
        // A prompt that leaves no room keeps its own text alone.
        let huge = "p".repeat(SYSTEM_PROMPT_TOTAL_MAX);
        let (prompt, applied) =
            compose_with(&huge, &refs, &given, "A DCF please.", PROMPT_BUDGET_CHARS);
        assert!(applied.is_empty());
        assert_eq!(prompt, huge);
    }

    #[test]
    fn adds_strongly_matching_library_skills() {
        let dcf = skill(
            "equity-valuation",
            "dcf, wacc, terminal value",
            &"v".repeat(300),
        );
        let ships = skill(
            "chartering",
            "laytime, demurrage, charter party",
            &"s".repeat(300),
        );
        let lib = vec![dcf.clone(), ships.clone()];
        let (prompt, applied) = compose_with(
            "Base.",
            &[&dcf],
            &lib,
            "Calculate laytime and demurrage under the charter party for the voyage.",
            10_000,
        );
        assert_eq!(applied.library, vec!["chartering"]);
        assert!(prompt.contains("### chartering (shared skill library)"));
        let (_, weak) = compose_with("Base.", &[&dcf], &lib, "Mention demurrage once.", 10_000);
        assert!(weak.library.is_empty(), "one keyword is not a strong match");
        assert_eq!(
            Applied {
                full: vec!["a".into()],
                summarised: vec!["c".into()],
                library: vec!["b".into()]
            }
            .describe(),
            "skills applied: a; from the library: b; summarised: c"
        );
    }

    #[test]
    fn common_keywords_weigh_less_and_never_pull_in_a_library_skill_alone() {
        // `validation`, `test plan` and `reliability` are listed by many skills.
        let mut lib: Vec<Skill> = (0..6)
            .map(|i| {
                skill(
                    &format!("generic-{i}"),
                    "validation, test plan, reliability",
                    "x",
                )
            })
            .collect();
        lib.push(skill(
            "hardware-bringup",
            "validation, test plan, reliability, halt",
            "x",
        ));
        let focus = Focus::new(
            "Write a test plan: input validation and the reliability of sign-up.",
            &lib,
        );
        let hw = lib.last().unwrap();
        assert!(focus.score(hw) < 3 * 3, "common keywords score 1 each");
        assert!(!focus.has_rare_keyword(hw));
        let (_, applied) = compose_with(
            "Base.",
            &[],
            &lib,
            "Write a test plan: input validation and the reliability of sign-up.",
            10_000,
        );
        assert!(applied.library.is_empty());
    }

    #[test]
    fn describes_changes_for_the_audit() {
        let s = |v: &[&str]| v.iter().map(|x| (*x).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            describe_change(&s(&["a", "b"]), &s(&["b", "c"])),
            "added c; removed a"
        );
        assert_eq!(
            describe_change(&s(&["a", "b"]), &s(&["b", "a"])),
            "reordered"
        );
        assert_eq!(describe_change(&s(&[]), &s(&["x"])), "added x");
    }

    #[test]
    fn keywords_match_on_word_boundaries() {
        let can = skill("buses", "can fd, canopen", "x");
        let scan = skill("scanning", "scan, scanner", "x");
        let focus = Focus::new(
            "Review the CAN FD bus timing.",
            &[can.clone(), scan.clone()],
        );
        assert!(focus.score(&can) >= 3);
        assert_eq!(focus.score(&scan), 0);
    }
}
