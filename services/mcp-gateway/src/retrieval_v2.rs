use std::collections::BTreeSet;

use rusqlite::Connection;
use serde_json::{json, Value};
use yena_model::{
    AbstentionReason, MemoryAnswer, MemoryAnswerContract, MemoryFreshness, RetrievalScope,
    RetrievalScopeKind, RetrievalTrace,
};

#[derive(Debug, Clone)]
pub(crate) struct RetrievalV2Input {
    pub query: String,
    pub limit: usize,
    pub include_trace: bool,
    pub scope: RetrievalScope,
    pub allowed_memory_types: BTreeSet<String>,
    pub redaction_keys: BTreeSet<String>,
}

#[derive(Debug, Clone)]
struct Candidate {
    source: String,
    id: String,
    statement: String,
    memory_type: String,
    value_json: Value,
    scope: RetrievalScope,
    freshness: MemoryFreshness,
    confidence: f32,
    evidence_refs: Vec<String>,
    contradiction_count: i64,
    updated_at: String,
}

#[derive(Debug, Clone)]
struct RankedCandidate {
    candidate: Candidate,
    matched_terms: Vec<String>,
    score: f64,
}

pub(crate) fn retrieve(
    conn: &Connection,
    input: RetrievalV2Input,
) -> anyhow::Result<MemoryAnswerContract> {
    let terms = tokenize_query(&input.query);
    let all_candidates = load_candidates(conn, &input.allowed_memory_types)?;
    let matching_candidates = match_candidates(all_candidates, &terms);
    let mut scoped = matching_candidates
        .iter()
        .filter(|ranked| scope_matches(&input.scope, &ranked.candidate.scope))
        .cloned()
        .collect::<Vec<_>>();

    if matching_candidates.is_empty() {
        return Ok(abstain(input, AbstentionReason::MissingEvidence));
    }
    if scoped.is_empty() {
        return Ok(abstain(input, AbstentionReason::OutOfScope));
    }
    if scoped
        .iter()
        .all(|r| r.candidate.freshness == MemoryFreshness::Stale)
    {
        return Ok(abstain(input, AbstentionReason::StaleMemory));
    }
    if scoped.iter().any(|r| r.candidate.contradiction_count > 0) {
        return Ok(abstain(input, AbstentionReason::Contradicted));
    }

    scoped.retain(|r| r.candidate.freshness != MemoryFreshness::Stale);
    scoped.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.candidate.updated_at.cmp(&a.candidate.updated_at))
    });

    if scoped
        .first()
        .map(|r| r.candidate.confidence < 0.2)
        .unwrap_or(true)
    {
        return Ok(abstain(input, AbstentionReason::LowConfidence));
    }

    let memories = scoped
        .into_iter()
        .take(input.limit)
        .map(|ranked| build_answer(&ranked, input.include_trace, &input.redaction_keys))
        .collect::<Vec<_>>();

    Ok(MemoryAnswerContract {
        query: input.query,
        scope: input.scope,
        should_abstain: false,
        abstention_reason: None,
        memories,
    })
}

pub(crate) fn safe_trace_summary(answer: &MemoryAnswerContract) -> Value {
    json!({
        "should_abstain": answer.should_abstain,
        "abstention_reason": answer.abstention_reason,
        "memories": answer.memories.iter().map(|memory| {
            json!({
                "memory_type": memory.memory_type,
                "confidence": memory.confidence,
                "freshness": memory.freshness,
                "evidence_refs": memory.evidence_refs,
                "redactions": memory.redactions,
                "trace": memory.trace.as_ref().map(|trace| json!({
                    "candidate_source": trace.candidate_source,
                    "candidate_id": trace.candidate_id,
                    "matched_terms": trace.matched_terms,
                    "score_components": trace.score_components,
                    "scope_filter": trace.scope_filter,
                    "redactions": trace.redactions,
                    "evidence_refs": trace.evidence_refs,
                }))
            })
        }).collect::<Vec<_>>()
    })
}

fn abstain(input: RetrievalV2Input, reason: AbstentionReason) -> MemoryAnswerContract {
    MemoryAnswerContract {
        query: input.query,
        scope: input.scope,
        should_abstain: true,
        abstention_reason: Some(reason),
        memories: Vec::new(),
    }
}

fn load_candidates(
    conn: &Connection,
    allowed_memory_types: &BTreeSet<String>,
) -> anyhow::Result<Vec<Candidate>> {
    let mut candidates = Vec::new();
    candidates.extend(load_memory_candidates(conn, allowed_memory_types)?);
    candidates.extend(load_observation_candidates(conn)?);
    candidates.extend(load_graph_candidates(conn, allowed_memory_types)?);
    Ok(candidates)
}

fn load_memory_candidates(
    conn: &Connection,
    allowed_memory_types: &BTreeSet<String>,
) -> anyhow::Result<Vec<Candidate>> {
    let mut stmt = conn.prepare(
        "
        SELECT
          mi.id,
          mi.canonical_key,
          mi.memory_type,
          mv.value_json,
          COALESCE(mm.scope_kind, 'global'),
          mm.repo_path,
          mm.repo_remote,
          mm.branch,
          mm.workspace_path,
          COALESCE(mm.freshness, 'stable'),
          COALESCE(mm.confidence, 1.0),
          mi.updated_at,
          GROUP_CONCAT(ml.evidence_record_id)
        FROM memory_items mi
        JOIN memory_item_versions mv ON mv.id = mi.active_version_id
        LEFT JOIN memory_item_metadata mm ON mm.memory_item_id = mi.id
        LEFT JOIN memory_links ml ON ml.memory_item_version_id = mv.id
        WHERE mi.status = 'active'
        GROUP BY mi.id, mi.canonical_key, mi.memory_type, mv.value_json,
          mm.scope_kind, mm.repo_path, mm.repo_remote, mm.branch, mm.workspace_path,
          mm.freshness, mm.confidence, mi.updated_at
        ORDER BY mi.updated_at DESC
        ",
    )?;

    let rows = stmt.query_map([], |row| {
        let memory_type: String = row.get(2)?;
        let value_json_raw: String = row.get(3)?;
        let value_json = parse_json_or_raw(&value_json_raw);
        let statement = statement_from_memory(&row.get::<_, String>(1)?, &value_json);
        Ok(Candidate {
            source: "memory_item".to_string(),
            id: row.get(0)?,
            statement,
            memory_type,
            value_json,
            scope: scope_from_row(
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
            ),
            freshness: parse_freshness(&row.get::<_, String>(9)?),
            confidence: row.get::<_, f32>(10)?,
            evidence_refs: split_group_concat(row.get(12)?),
            contradiction_count: 0,
            updated_at: row.get(11)?,
        })
    })?;

    let mut out = Vec::new();
    for row in rows {
        let candidate = row?;
        if allowed_memory_types.is_empty() || allowed_memory_types.contains(&candidate.memory_type)
        {
            out.push(candidate);
        }
    }
    Ok(out)
}

fn load_observation_candidates(conn: &Connection) -> anyhow::Result<Vec<Candidate>> {
    let mut stmt = conn.prepare(
        "
        SELECT
          o.id,
          o.observation_type,
          o.statement,
          o.scope_kind,
          o.repo_path,
          o.repo_remote,
          o.branch,
          o.workspace_path,
          o.freshness,
          o.confidence,
          o.contradiction_count,
          o.updated_at,
          GROUP_CONCAT(oel.evidence_record_id)
        FROM observations o
        LEFT JOIN observation_evidence_links oel ON oel.observation_id = o.id
        WHERE o.status = 'active'
        GROUP BY o.id, o.observation_type, o.statement, o.scope_kind, o.repo_path,
          o.repo_remote, o.branch, o.workspace_path, o.freshness, o.confidence,
          o.contradiction_count, o.updated_at
        ORDER BY o.updated_at DESC
        ",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(Candidate {
            source: "observation".to_string(),
            id: row.get(0)?,
            statement: row.get(2)?,
            memory_type: row.get(1)?,
            value_json: json!({}),
            scope: scope_from_row(
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
            ),
            freshness: parse_freshness(&row.get::<_, String>(8)?),
            confidence: row.get::<_, f32>(9)?,
            evidence_refs: split_group_concat(row.get(12)?),
            contradiction_count: row.get(10)?,
            updated_at: row.get(11)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn load_graph_candidates(
    conn: &Connection,
    allowed_memory_types: &BTreeSet<String>,
) -> anyhow::Result<Vec<Candidate>> {
    if !allowed_memory_types.is_empty()
        && !allowed_memory_types.contains("graph")
        && !allowed_memory_types.contains("relationship")
    {
        return Ok(Vec::new());
    }

    let mut stmt = conn.prepare(
        "
        SELECT
          gr.id,
          se.canonical_name,
          gr.predicate,
          oe.canonical_name,
          grv.confidence,
          grv.attributes_json,
          gr.updated_at,
          GROUP_CONCAT(grel.evidence_record_id)
        FROM graph_relationships gr
        JOIN graph_relationship_versions grv ON grv.id = gr.active_version_id
        JOIN graph_entities se ON se.id = gr.subject_entity_id
        JOIN graph_entities oe ON oe.id = gr.object_entity_id
        LEFT JOIN graph_relationship_evidence_links grel ON grel.relationship_version_id = grv.id
        WHERE gr.status = 'active'
        GROUP BY gr.id, se.canonical_name, gr.predicate, oe.canonical_name,
          grv.confidence, grv.attributes_json, gr.updated_at
        ORDER BY gr.updated_at DESC
        ",
    )?;

    let rows = stmt.query_map([], |row| {
        let subject: String = row.get(1)?;
        let predicate: String = row.get(2)?;
        let object: String = row.get(3)?;
        let statement = format!("{} {} {}", subject, predicate, object);
        let attributes_raw: String = row.get(5)?;
        Ok(Candidate {
            source: "graph_relationship".to_string(),
            id: row.get(0)?,
            statement,
            memory_type: "relationship".to_string(),
            value_json: parse_json_or_raw(&attributes_raw),
            scope: global_scope(),
            freshness: MemoryFreshness::Stable,
            confidence: row.get::<_, f32>(4)?,
            evidence_refs: split_group_concat(row.get(7)?),
            contradiction_count: 0,
            updated_at: row.get(6)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn match_candidates(candidates: Vec<Candidate>, terms: &[String]) -> Vec<RankedCandidate> {
    candidates
        .into_iter()
        .filter_map(|candidate| {
            let haystack = format!(
                "{} {} {}",
                candidate.statement, candidate.memory_type, candidate.value_json
            )
            .to_lowercase();
            let matched_terms = terms
                .iter()
                .filter(|term| haystack.contains(term.as_str()))
                .cloned()
                .collect::<Vec<_>>();
            if matched_terms.is_empty() {
                return None;
            }
            let freshness_boost = match candidate.freshness {
                MemoryFreshness::New => 0.2,
                MemoryFreshness::Stable => 0.5,
                MemoryFreshness::Strengthening => 0.7,
                MemoryFreshness::Weakening => -0.2,
                MemoryFreshness::Stale => -1.0,
            };
            let score = (matched_terms.len() as f64 * 10.0)
                + candidate.confidence as f64
                + freshness_boost
                + (candidate.evidence_refs.len() as f64 * 0.1);
            Some(RankedCandidate {
                candidate,
                matched_terms,
                score,
            })
        })
        .collect()
}

fn build_answer(
    ranked: &RankedCandidate,
    include_trace: bool,
    redaction_keys: &BTreeSet<String>,
) -> MemoryAnswer {
    let (_redacted_value, redactions) =
        apply_redaction(ranked.candidate.value_json.clone(), redaction_keys);
    let trace = include_trace.then(|| RetrievalTrace {
        candidate_source: ranked.candidate.source.clone(),
        candidate_id: ranked.candidate.id.clone(),
        matched_terms: ranked.matched_terms.clone(),
        score_components: json!({
            "rank_score": ranked.score,
            "confidence": ranked.candidate.confidence,
            "freshness": ranked.candidate.freshness,
            "evidence_count": ranked.candidate.evidence_refs.len(),
        }),
        scope_filter: scope_filter_label(&ranked.candidate.scope),
        redactions: redactions.clone(),
        evidence_refs: ranked.candidate.evidence_refs.clone(),
    });

    MemoryAnswer {
        statement: ranked.candidate.statement.clone(),
        memory_type: ranked.candidate.memory_type.clone(),
        freshness: ranked.candidate.freshness.clone(),
        confidence: ranked.candidate.confidence,
        evidence_refs: ranked.candidate.evidence_refs.clone(),
        trace,
        redactions,
    }
}

fn scope_matches(requested: &RetrievalScope, candidate: &RetrievalScope) -> bool {
    if candidate.kind == RetrievalScopeKind::Global {
        return true;
    }
    if requested.kind != candidate.kind {
        return false;
    }
    match candidate.kind {
        RetrievalScopeKind::Global => true,
        RetrievalScopeKind::Repo => {
            optional_eq(&requested.repo_path, &candidate.repo_path)
                && optional_eq(&requested.repo_remote, &candidate.repo_remote)
                && optional_eq(&requested.branch, &candidate.branch)
        }
        RetrievalScopeKind::Workspace => {
            optional_eq(&requested.workspace_path, &candidate.workspace_path)
        }
        RetrievalScopeKind::Agent | RetrievalScopeKind::Source => true,
    }
}

fn optional_eq(requested: &Option<String>, candidate: &Option<String>) -> bool {
    match candidate {
        Some(candidate_value) => requested.as_deref() == Some(candidate_value.as_str()),
        None => true,
    }
}

fn scope_filter_label(scope: &RetrievalScope) -> String {
    match scope.kind {
        RetrievalScopeKind::Global => "global".to_string(),
        RetrievalScopeKind::Repo => format!(
            "repo:{}:{}:{}",
            scope.repo_path.as_deref().unwrap_or("*"),
            scope.repo_remote.as_deref().unwrap_or("*"),
            scope.branch.as_deref().unwrap_or("*")
        ),
        RetrievalScopeKind::Workspace => {
            format!(
                "workspace:{}",
                scope.workspace_path.as_deref().unwrap_or("*")
            )
        }
        RetrievalScopeKind::Agent => "agent".to_string(),
        RetrievalScopeKind::Source => "source".to_string(),
    }
}

fn scope_from_row(
    kind: String,
    repo_path: Option<String>,
    repo_remote: Option<String>,
    branch: Option<String>,
    workspace_path: Option<String>,
) -> RetrievalScope {
    RetrievalScope {
        kind: parse_scope_kind(&kind),
        repo_path,
        repo_remote,
        branch,
        workspace_path,
    }
}

pub(crate) fn parse_scope_kind(value: &str) -> RetrievalScopeKind {
    match value.trim().to_lowercase().as_str() {
        "repo" => RetrievalScopeKind::Repo,
        "workspace" => RetrievalScopeKind::Workspace,
        "agent" => RetrievalScopeKind::Agent,
        "source" => RetrievalScopeKind::Source,
        _ => RetrievalScopeKind::Global,
    }
}

pub(crate) fn global_scope() -> RetrievalScope {
    RetrievalScope {
        kind: RetrievalScopeKind::Global,
        repo_path: None,
        repo_remote: None,
        branch: None,
        workspace_path: None,
    }
}

fn parse_freshness(value: &str) -> MemoryFreshness {
    match value.trim().to_lowercase().as_str() {
        "new" => MemoryFreshness::New,
        "strengthening" => MemoryFreshness::Strengthening,
        "weakening" => MemoryFreshness::Weakening,
        "stale" => MemoryFreshness::Stale,
        _ => MemoryFreshness::Stable,
    }
}

fn tokenize_query(query: &str) -> Vec<String> {
    query
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .map(|term| term.trim().to_lowercase())
        .filter(|term| term.len() > 1)
        .collect()
}

fn statement_from_memory(canonical_key: &str, value_json: &Value) -> String {
    value_json
        .get("statement")
        .and_then(Value::as_str)
        .or_else(|| value_json.get("value").and_then(Value::as_str))
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("{} {}", canonical_key, value_json))
}

fn parse_json_or_raw(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| json!({ "raw": raw }))
}

fn split_group_concat(raw: Option<String>) -> Vec<String> {
    raw.unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn apply_redaction(value: Value, keys: &BTreeSet<String>) -> (Value, Vec<String>) {
    if keys.is_empty() {
        return (value, Vec::new());
    }

    let mut redacted = Vec::new();
    let updated = match value {
        Value::Object(mut map) => {
            for key in keys {
                if map.remove(key).is_some() {
                    redacted.push(key.clone());
                }
            }
            Value::Object(map)
        }
        other => other,
    };
    (updated, redacted)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use rusqlite::{params, Connection};
    use serde_json::json;
    use yena_model::{AbstentionReason, RetrievalScope, RetrievalScopeKind};

    use super::{global_scope, retrieve, RetrievalV2Input};

    #[test]
    fn repo_scoped_memory_does_not_leak_across_repos() {
        let conn = test_conn();
        seed_memory(
            &conn,
            "mem-yena-db",
            "ver-yena-db",
            "project_decision",
            "decision:db",
            json!({"statement":"Yena uses SQLite for local-first storage"}),
            "repo",
            Some("/repo/yena"),
            Some("https://github.com/eyasu-12/Yena.git"),
            Some("main"),
            "stable",
            0.91,
        );

        let answer = retrieve(
            &conn,
            input(
                "SQLite local-first storage",
                repo_scope(
                    "/repo/other",
                    "https://github.com/example/Other.git",
                    "main",
                ),
                false,
            ),
        )
        .expect("retrieval should run");

        assert!(answer.should_abstain);
        assert_eq!(answer.abstention_reason, Some(AbstentionReason::OutOfScope));
    }

    #[test]
    fn include_trace_controls_trace_presence_and_redaction() {
        let conn = test_conn();
        seed_memory(
            &conn,
            "mem-token",
            "ver-token",
            "project_decision",
            "decision:token",
            json!({"statement":"Yena stores tokens only in local config","secret":"do-not-leak"}),
            "global",
            None,
            None,
            None,
            "stable",
            0.88,
        );

        let mut request = input("How are tokens stored?", global_scope(), true);
        request.redaction_keys = BTreeSet::from(["secret".to_string()]);
        let answer = retrieve(&conn, request).expect("retrieval should run");

        assert!(!answer.should_abstain);
        let memory = &answer.memories[0];
        assert!(memory.trace.is_some());
        assert_eq!(memory.redactions, vec!["secret".to_string()]);
        let trace_text = serde_json::to_string(&memory.trace).expect("trace should encode");
        assert!(!trace_text.contains("do-not-leak"));
    }

    #[test]
    fn missing_evidence_abstains() {
        let conn = test_conn();
        let answer = retrieve(
            &conn,
            input("Which auth provider did we choose?", global_scope(), false),
        )
        .expect("retrieval should run");

        assert!(answer.should_abstain);
        assert_eq!(
            answer.abstention_reason,
            Some(AbstentionReason::MissingEvidence)
        );
    }

    fn input(query: &str, scope: RetrievalScope, include_trace: bool) -> RetrievalV2Input {
        RetrievalV2Input {
            query: query.to_string(),
            limit: 5,
            include_trace,
            scope,
            allowed_memory_types: BTreeSet::from(["project_decision".to_string()]),
            redaction_keys: BTreeSet::new(),
        }
    }

    fn repo_scope(repo_path: &str, repo_remote: &str, branch: &str) -> RetrievalScope {
        RetrievalScope {
            kind: RetrievalScopeKind::Repo,
            repo_path: Some(repo_path.to_string()),
            repo_remote: Some(repo_remote.to_string()),
            branch: Some(branch.to_string()),
            workspace_path: None,
        }
    }

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db should open");
        conn.execute_batch(include_str!("../../../db/migrations/0001_init.sql"))
            .expect("base migration should apply");
        conn.execute_batch(include_str!("../../../db/migrations/0002_indexes.sql"))
            .expect("index migration should apply");
        conn.execute_batch(include_str!(
            "../../../db/migrations/0003_knowledge_graph.sql"
        ))
        .expect("graph migration should apply");
        conn.execute(
            "ALTER TABLE graph_relationship_versions ADD COLUMN confidence REAL NOT NULL DEFAULT 1.0",
            [],
        )
        .expect("confidence column should be added");
        conn.execute_batch(include_str!(
            "../../../db/migrations/0004_graph_confidence.sql"
        ))
        .expect("confidence index migration should apply");
        conn.execute_batch(include_str!(
            "../../../db/migrations/0005_graph_canonicalization.sql"
        ))
        .expect("graph canonicalization migration should apply");
        conn.execute_batch(include_str!(
            "../../../db/migrations/0006_retrieval_v2_foundation.sql"
        ))
        .expect("retrieval v2 migration should apply");
        conn
    }

    #[allow(clippy::too_many_arguments)]
    fn seed_memory(
        conn: &Connection,
        memory_id: &str,
        version_id: &str,
        memory_type: &str,
        canonical_key: &str,
        value_json: serde_json::Value,
        scope_kind: &str,
        repo_path: Option<&str>,
        repo_remote: Option<&str>,
        branch: Option<&str>,
        freshness: &str,
        confidence: f32,
    ) {
        let now = "2026-04-24T00:00:00+00:00";
        conn.execute(
            "INSERT INTO memory_items (id, memory_type, canonical_key, active_version_id, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, 'active', ?5, ?5)",
            params![memory_id, memory_type, canonical_key, version_id, now],
        )
        .expect("memory item should insert");
        conn.execute(
            "INSERT INTO memory_item_versions (id, memory_item_id, version_number, state, value_json, supersedes_version_id, valid_from, valid_to, created_at) VALUES (?1, ?2, 1, 'active', ?3, NULL, ?4, NULL, ?4)",
            params![version_id, memory_id, serde_json::to_string(&value_json).expect("value should encode"), now],
        )
        .expect("memory version should insert");
        conn.execute(
            "INSERT INTO memory_item_metadata (memory_item_id, scope_kind, repo_path, repo_remote, branch, workspace_path, sensitivity, freshness, confidence, decay_policy, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, NULL, 'normal', ?6, ?7, NULL, ?8, ?8)",
            params![memory_id, scope_kind, repo_path, repo_remote, branch, freshness, confidence, now],
        )
        .expect("memory metadata should insert");
    }
}
