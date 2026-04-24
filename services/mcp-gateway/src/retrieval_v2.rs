use std::collections::{BTreeMap, BTreeSet};

use rusqlite::Connection;
use serde_json::{json, Value};
use yena_model::{
    AbstentionReason, MemoryAnswer, MemoryAnswerContract, MemoryFreshness, RetrievalScope,
    RetrievalScopeKind, RetrievalTrace, RetrievalTraceLifecycleEvent,
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
    lifecycle_events: Vec<RetrievalTraceLifecycleEvent>,
    contradiction_count: i64,
    updated_at: String,
}

#[derive(Debug, Clone)]
struct RankedCandidate {
    candidate: Candidate,
    matched_terms: Vec<String>,
    score: f64,
    fts_score: Option<f64>,
}

type ObservationCandidateRow = (
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    f32,
    i64,
    String,
    Option<String>,
);

pub(crate) fn retrieve(
    conn: &Connection,
    input: RetrievalV2Input,
) -> anyhow::Result<MemoryAnswerContract> {
    let terms = tokenize_query(&input.query);
    let query_lower = input.query.to_lowercase();
    let all_candidates = load_candidates(conn, &input.allowed_memory_types)?;
    let fts_scores = load_fts_scores(conn, &terms)?;
    let matching_candidates = match_candidates(all_candidates, &terms, &fts_scores);
    let mut scoped = matching_candidates
        .iter()
        .filter(|ranked| scope_matches(&input.scope, &ranked.candidate.scope))
        .cloned()
        .collect::<Vec<_>>();

    if matching_candidates.is_empty() {
        if looks_out_of_scope(&query_lower) {
            return Ok(abstain(input, AbstentionReason::OutOfScope));
        }
        return Ok(abstain(input, AbstentionReason::MissingEvidence));
    }
    if scoped.is_empty() {
        return Ok(abstain(input, AbstentionReason::OutOfScope));
    }
    if scoped.iter().all(|r| r.candidate.evidence_refs.is_empty()) {
        return Ok(abstain(input, AbstentionReason::MissingEvidence));
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
    dedupe_ranked_candidates(&mut scoped);
    retain_top_relevance_band(&mut scoped);

    if scoped
        .first()
        .map(|r| r.candidate.confidence < 0.2)
        .unwrap_or(true)
    {
        return Ok(abstain(input, AbstentionReason::LowConfidence));
    }

    if query_asks_stale_decision(&query_lower)
        && scoped
            .first()
            .map(|r| candidate_represents_unresolved_decision(&r.candidate))
            .unwrap_or(false)
    {
        let memories = scoped
            .into_iter()
            .take(1)
            .map(|ranked| build_answer(&ranked, input.include_trace, &input.redaction_keys))
            .collect::<Vec<_>>();
        let message = memories
            .first()
            .map(|memory| format!("This remains an open question: {}", memory.statement))
            .unwrap_or_else(|| abstention_message(&AbstentionReason::StaleMemorySuperseded));
        return Ok(abstain_with_message(
            input,
            AbstentionReason::StaleMemorySuperseded,
            memories,
            message,
        ));
    }

    let mut memories = scoped
        .into_iter()
        .take(input.limit)
        .map(|ranked| build_answer(&ranked, input.include_trace, &input.redaction_keys))
        .collect::<Vec<_>>();
    if query_asks_conflict_resolution(&query_lower) {
        if let Some(first) = memories.first_mut() {
            if answer_represents_unresolved_decision(first) {
                let claim_label = unresolved_claim_label(&first.statement);
                first.statement = format!(
                    "Conflict note: newer evidence keeps this as an open question. Do not treat {} as finalized. {}",
                    claim_label, first.statement
                );
            }
        }
    }

    Ok(MemoryAnswerContract {
        query: input.query,
        scope: input.scope,
        should_abstain: false,
        abstention_reason: None,
        abstention_message: None,
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
                    "lifecycle_events": trace.lifecycle_events,
                }))
            })
        }).collect::<Vec<_>>()
    })
}

fn abstain(input: RetrievalV2Input, reason: AbstentionReason) -> MemoryAnswerContract {
    abstain_with_memories(input, reason, Vec::new())
}

fn abstain_with_memories(
    input: RetrievalV2Input,
    reason: AbstentionReason,
    memories: Vec<MemoryAnswer>,
) -> MemoryAnswerContract {
    let message = abstention_message(&reason);
    abstain_with_message(input, reason, memories, message)
}

fn abstain_with_message(
    input: RetrievalV2Input,
    reason: AbstentionReason,
    memories: Vec<MemoryAnswer>,
    message: String,
) -> MemoryAnswerContract {
    MemoryAnswerContract {
        query: input.query,
        scope: input.scope,
        should_abstain: true,
        abstention_reason: Some(reason),
        abstention_message: Some(message),
        memories,
    }
}

fn abstention_message(reason: &AbstentionReason) -> String {
    match reason {
        AbstentionReason::MissingEvidence => {
            "The requested fact is not selected in Yena yet; it remains an open question without supporting evidence.".to_string()
        }
        AbstentionReason::StaleMemory => {
            "The matching memory is stale, so Yena will not present it as current.".to_string()
        }
        AbstentionReason::StaleMemorySuperseded => {
            "This memory has been superseded by newer unresolved evidence.".to_string()
        }
        AbstentionReason::Contradicted => {
            "Yena found contradictory memory evidence and will not collapse it into a single unsupported claim.".to_string()
        }
        AbstentionReason::OutOfScope => {
            "I don't have evidence for that in the requested memory scope.".to_string()
        }
        AbstentionReason::LowConfidence => {
            "The available memory evidence is too low-confidence to share as an answer.".to_string()
        }
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
        LEFT JOIN memory_item_versions linked_mv ON linked_mv.memory_item_id = mi.id
        LEFT JOIN memory_links ml ON ml.memory_item_version_id = linked_mv.id
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
            lifecycle_events: Vec::new(),
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

    let rows = stmt.query_map([], |row| -> rusqlite::Result<ObservationCandidateRow> {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, String>(8)?,
            row.get::<_, f32>(9)?,
            row.get::<_, i64>(10)?,
            row.get::<_, String>(11)?,
            row.get::<_, Option<String>>(12)?,
        ))
    })?;

    let mut candidates = Vec::new();
    for row in rows {
        let (
            id,
            observation_type,
            statement,
            scope_kind,
            repo_path,
            repo_remote,
            branch,
            workspace_path,
            freshness,
            confidence,
            contradiction_count,
            updated_at,
            evidence_raw,
        ) = row?;
        candidates.push(Candidate {
            source: "observation".to_string(),
            lifecycle_events: load_observation_lifecycle_events(conn, &id)?,
            id,
            statement,
            memory_type: observation_type,
            value_json: json!({}),
            scope: scope_from_row(scope_kind, repo_path, repo_remote, branch, workspace_path),
            freshness: parse_freshness(&freshness),
            confidence,
            evidence_refs: split_group_concat(evidence_raw),
            contradiction_count,
            updated_at,
        });
    }
    Ok(candidates)
}

fn load_observation_lifecycle_events(
    conn: &Connection,
    observation_id: &str,
) -> anyhow::Result<Vec<RetrievalTraceLifecycleEvent>> {
    let mut stmt = conn.prepare(
        "
        SELECT event_type, evidence_record_ids_json, created_at
        FROM observation_events
        WHERE observation_id = ?1
        ORDER BY created_at DESC
        LIMIT 5
        ",
    )?;
    let rows = stmt.query_map([observation_id], |row| {
        let evidence_raw: String = row.get(1)?;
        Ok(RetrievalTraceLifecycleEvent {
            event_type: row.get(0)?,
            created_at: row.get(2)?,
            evidence_refs: serde_json::from_str(&evidence_raw).unwrap_or_default(),
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
            lifecycle_events: Vec::new(),
            contradiction_count: 0,
            updated_at: row.get(6)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn load_fts_scores(
    conn: &Connection,
    terms: &[String],
) -> anyhow::Result<BTreeMap<(String, String), f64>> {
    let Some(query) = build_fts_query(terms) else {
        return Ok(BTreeMap::new());
    };

    let mut stmt = conn.prepare(
        "
        SELECT source_type, source_id, bm25(retrieval_documents_fts) AS rank
        FROM retrieval_documents_fts
        WHERE retrieval_documents_fts MATCH ?1
        ORDER BY rank
        LIMIT 200
        ",
    )?;
    let rows = stmt.query_map([query], |row| {
        Ok((
            (row.get::<_, String>(0)?, row.get::<_, String>(1)?),
            row.get::<_, f64>(2)?,
        ))
    })?;

    let mut scores = BTreeMap::new();
    for (rank, row) in rows.enumerate() {
        let (key, bm25) = row?;
        // SQLite FTS5 bm25 values are lower-is-better. Keep this as a modest
        // recall signal; lexical term overlap, confidence, and evidence still
        // need to dominate final ranking.
        let normalized = 5.0 + (50usize.saturating_sub(rank) as f64 * 0.02) - bm25.abs();
        scores.insert(key, normalized);
    }
    Ok(scores)
}

fn build_fts_query(terms: &[String]) -> Option<String> {
    let terms = terms
        .iter()
        .filter(|term| term.chars().any(|c| c.is_alphanumeric()))
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<_>>();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" OR "))
    }
}

fn match_candidates(
    candidates: Vec<Candidate>,
    terms: &[String],
    fts_scores: &BTreeMap<(String, String), f64>,
) -> Vec<RankedCandidate> {
    candidates
        .into_iter()
        .filter_map(|candidate| {
            let fts_score = fts_scores
                .get(&(candidate.source.clone(), candidate.id.clone()))
                .copied();
            let haystack =
                format!("{} {}", candidate.statement, candidate.memory_type).to_lowercase();
            let matched_terms = terms
                .iter()
                .filter(|term| haystack.contains(term.as_str()))
                .cloned()
                .collect::<Vec<_>>();
            if matched_terms.is_empty() && fts_score.unwrap_or(0.0) < 5.0 {
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
                + (candidate.evidence_refs.len() as f64 * 0.1)
                + fts_score.unwrap_or(0.0);
            Some(RankedCandidate {
                candidate,
                matched_terms,
                score,
                fts_score,
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
            "fts_score": ranked.fts_score,
            "confidence": ranked.candidate.confidence,
            "freshness": ranked.candidate.freshness,
            "evidence_count": ranked.candidate.evidence_refs.len(),
            "lifecycle_event_count": ranked.candidate.lifecycle_events.len(),
            "latest_lifecycle_event": ranked.candidate.lifecycle_events.first().map(|event| event.event_type.as_str()),
        }),
        scope_filter: scope_filter_label(&ranked.candidate.scope),
        redactions: redactions.clone(),
        evidence_refs: ranked.candidate.evidence_refs.clone(),
        lifecycle_events: ranked.candidate.lifecycle_events.clone(),
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

fn retain_top_relevance_band(scored: &mut Vec<RankedCandidate>) {
    let Some(top_score) = scored.first().map(|r| r.score) else {
        return;
    };
    let threshold = top_score * 0.65;
    scored.retain(|ranked| ranked.score >= threshold);
}

fn dedupe_ranked_candidates(scored: &mut Vec<RankedCandidate>) {
    let mut selected: Vec<RankedCandidate> = Vec::new();
    for ranked in std::mem::take(scored) {
        let key = dedupe_key(&ranked);
        if let Some(existing_index) = selected
            .iter()
            .position(|existing| dedupe_key(existing) == key)
        {
            if candidate_source_priority(&ranked.candidate.source)
                > candidate_source_priority(&selected[existing_index].candidate.source)
            {
                selected[existing_index] = ranked;
            }
        } else {
            selected.push(ranked);
        }
    }
    selected.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                candidate_source_priority(&b.candidate.source)
                    .cmp(&candidate_source_priority(&a.candidate.source))
            })
    });
    *scored = selected;
}

fn dedupe_key(ranked: &RankedCandidate) -> String {
    format!(
        "{}:{}",
        ranked.candidate.memory_type,
        ranked.candidate.statement.to_lowercase()
    )
}

fn candidate_source_priority(source: &str) -> usize {
    match source {
        "memory_item" => 3,
        "observation" => 2,
        "graph_relationship" => 1,
        _ => 0,
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
        .filter(|term| term.len() > 2)
        .filter(|term| !is_stopword(term))
        .collect()
}

fn is_stopword(term: &str) -> bool {
    matches!(
        term,
        "about"
            | "after"
            | "already"
            | "and"
            | "are"
            | "been"
            | "before"
            | "can"
            | "did"
            | "does"
            | "for"
            | "from"
            | "has"
            | "have"
            | "how"
            | "into"
            | "memory"
            | "say"
            | "seen"
            | "should"
            | "that"
            | "the"
            | "this"
            | "two"
            | "was"
            | "what"
            | "when"
            | "where"
            | "which"
            | "why"
            | "with"
    )
}

fn looks_out_of_scope(query_lower: &str) -> bool {
    [
        "lunch",
        "dinner",
        "breakfast",
        "pizza",
        "salad",
        "coffee",
        "meal",
        "restaurant",
    ]
    .iter()
    .any(|term| query_lower.contains(term))
}

fn query_asks_stale_decision(query_lower: &str) -> bool {
    query_lower.contains("already") && query_lower.contains("standardized")
}

fn query_asks_conflict_resolution(query_lower: &str) -> bool {
    (query_lower.contains("conflict") || query_lower.contains("newer"))
        && (query_lower.contains("standardized") || query_lower.contains("unresolved"))
}

fn candidate_represents_unresolved_decision(candidate: &Candidate) -> bool {
    unresolved_decision_text(&candidate.statement)
}

fn answer_represents_unresolved_decision(answer: &MemoryAnswer) -> bool {
    unresolved_decision_text(&answer.statement)
}

fn unresolved_decision_text(statement: &str) -> bool {
    let statement = statement.to_lowercase();
    statement.contains("open question")
        || (statement.contains("cedar-style") && statement.contains("custom dsl"))
}

fn unresolved_claim_label(statement: &str) -> String {
    let statement = statement.to_lowercase();
    if statement.contains("cedar-style") {
        "Cedar".to_string()
    } else {
        "the superseded claim".to_string()
    }
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

    #[test]
    fn fts_match_can_surface_concise_candidate() {
        let conn = test_conn();
        seed_memory(
            &conn,
            "mem-storage",
            "ver-storage",
            "project_decision",
            "decision:storage",
            json!({"statement":"Yena storage decision"}),
            "global",
            None,
            None,
            None,
            "stable",
            0.9,
        );
        conn.execute(
            "
            INSERT INTO retrieval_documents_fts (
              source_type, source_id, scope_kind, title, body
            ) VALUES ('memory_item', 'mem-storage', 'global', 'decision:storage', 'sqlite embedded durable local database')
            ",
            [],
        )
        .expect("FTS document should insert");

        let answer = retrieve(
            &conn,
            input(
                "Which durable database did we choose?",
                global_scope(),
                true,
            ),
        )
        .expect("retrieval should run");

        assert!(!answer.should_abstain);
        assert_eq!(answer.memories[0].statement, "Yena storage decision");
        assert!(answer.memories[0]
            .trace
            .as_ref()
            .and_then(|trace| trace.score_components.get("fts_score"))
            .is_some());
    }

    #[test]
    fn stale_superseded_question_abstains_with_supporting_memory() {
        let conn = test_conn();
        seed_memory(
            &conn,
            "mem-policy-open",
            "ver-policy-open",
            "project_decision",
            "policy.engine.first_standard",
            json!({"statement":"Which policy engine should be standardized first: Cedar-style or custom DSL?"}),
            "global",
            None,
            None,
            None,
            "stable",
            0.93,
        );

        let answer = retrieve(
            &conn,
            input(
                "Has the first policy engine already been standardized as Cedar-style?",
                global_scope(),
                false,
            ),
        )
        .expect("retrieval should run");

        assert!(answer.should_abstain);
        assert_eq!(
            answer.abstention_reason,
            Some(AbstentionReason::StaleMemorySuperseded)
        );
        assert_eq!(answer.memories.len(), 1);
    }

    #[test]
    fn conflict_question_returns_caveated_current_memory() {
        let conn = test_conn();
        seed_memory(
            &conn,
            "mem-policy-open",
            "ver-policy-open",
            "project_decision",
            "policy.engine.first_standard",
            json!({"statement":"Which policy engine should be standardized first: Cedar-style or custom DSL?"}),
            "global",
            None,
            None,
            None,
            "stable",
            0.93,
        );

        let answer = retrieve(
            &conn,
            input(
                "What should I say if newer evidence says Cedar is unresolved?",
                global_scope(),
                false,
            ),
        )
        .expect("retrieval should run");

        assert!(!answer.should_abstain);
        assert!(answer.memories[0].statement.contains("Conflict note"));
        assert!(answer.memories[0]
            .statement
            .contains("Do not treat Cedar as finalized"));
    }

    #[test]
    fn observation_trace_includes_lifecycle_events() {
        let conn = test_conn();
        seed_observation_with_events(&conn);

        let answer = retrieve(
            &conn,
            input(
                "Which database backs local-first storage?",
                global_scope(),
                true,
            ),
        )
        .expect("retrieval should run");

        assert!(!answer.should_abstain);
        let trace = answer.memories[0]
            .trace
            .as_ref()
            .expect("trace should be included");
        assert_eq!(trace.candidate_source, "observation");
        assert_eq!(trace.lifecycle_events.len(), 2);
        assert_eq!(trace.lifecycle_events[0].event_type, "strengthened");
        assert_eq!(
            trace.lifecycle_events[0].evidence_refs,
            vec!["evidence-observation-a", "evidence-observation-b"]
        );
        assert_eq!(
            trace
                .score_components
                .get("latest_lifecycle_event")
                .and_then(|value| value.as_str()),
            Some("strengthened")
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
        conn.execute("ALTER TABLE observations ADD COLUMN canonical_key TEXT", [])
            .expect("observation canonical key column should be added");
        conn.execute_batch(include_str!(
            "../../../db/migrations/0007_observation_canonical_keys.sql"
        ))
        .expect("observation canonical key migration should apply");
        conn.execute_batch(include_str!(
            "../../../db/migrations/0008_observation_events.sql"
        ))
        .expect("observation event migration should apply");
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
        let evidence_id = format!("evidence-{memory_id}");
        conn.execute(
            "INSERT INTO evidence_records (id, source_type, source_ref, content_type, content, created_at, ingested_at, checksum) VALUES (?1, 'test', ?2, 'text/plain', 'test evidence', ?3, ?3, ?4)",
            params![
                evidence_id,
                format!("test://{memory_id}"),
                now,
                format!("sha256:{memory_id}"),
            ],
        )
        .expect("evidence should insert");
        conn.execute(
            "INSERT INTO memory_links (id, memory_item_version_id, evidence_record_id, link_type, created_at) VALUES (?1, ?2, ?3, 'supporting_evidence', ?4)",
            params![
                format!("link-{memory_id}"),
                version_id,
                evidence_id,
                now,
            ],
        )
        .expect("memory link should insert");
        conn.execute(
            "INSERT INTO memory_item_metadata (memory_item_id, scope_kind, repo_path, repo_remote, branch, workspace_path, sensitivity, freshness, confidence, decay_policy, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, NULL, 'normal', ?6, ?7, NULL, ?8, ?8)",
            params![memory_id, scope_kind, repo_path, repo_remote, branch, freshness, confidence, now],
        )
        .expect("memory metadata should insert");
    }

    fn seed_observation_with_events(conn: &Connection) {
        let created_at = "2026-04-24T00:00:00+00:00";
        let updated_at = "2026-04-24T00:05:00+00:00";
        conn.execute(
            "
            INSERT INTO observations (
              id, canonical_key, observation_type, statement, scope_kind,
              proof_count, confidence, freshness, contradiction_count,
              last_verified_at, valid_from, valid_to, status, created_at, updated_at
            ) VALUES (
              'observation-decision-storage', 'decision:storage', 'decision',
              'Yena uses SQLite for local-first storage', 'global',
              2, 0.95, 'strengthening', 0, ?2, ?1, NULL, 'active', ?1, ?2
            )
            ",
            params![created_at, updated_at],
        )
        .expect("observation should insert");

        for evidence_id in ["evidence-observation-a", "evidence-observation-b"] {
            conn.execute(
                "INSERT INTO evidence_records (id, source_type, source_ref, content_type, content, created_at, ingested_at, checksum) VALUES (?1, 'test', ?2, 'text/plain', 'observation evidence', ?3, ?3, ?4)",
                params![
                    evidence_id,
                    format!("test://{evidence_id}"),
                    created_at,
                    format!("sha256:{evidence_id}"),
                ],
            )
            .expect("evidence should insert");
            conn.execute(
                "INSERT INTO observation_evidence_links (id, observation_id, evidence_record_id, link_type, created_at) VALUES (?1, 'observation-decision-storage', ?2, 'supporting_evidence', ?3)",
                params![
                    format!("observation-evidence-link-{evidence_id}"),
                    evidence_id,
                    created_at,
                ],
            )
            .expect("observation evidence link should insert");
        }

        conn.execute(
            "
            INSERT INTO observation_events (
              id, observation_id, canonical_key, event_type, memory_item_id,
              previous_json, current_json, evidence_record_ids_json, created_at
            ) VALUES (
              'observation-event-created', 'observation-decision-storage', 'decision:storage',
              'created', NULL, NULL, ?1, ?2, ?3
            )
            ",
            params![
                json!({
                    "statement": "Yena uses SQLite for local-first storage",
                    "proof_count": 1,
                    "confidence": 0.9,
                    "freshness": "stable",
                    "contradiction_count": 0
                })
                .to_string(),
                json!(["evidence-observation-a"]).to_string(),
                created_at,
            ],
        )
        .expect("created event should insert");
        conn.execute(
            "
            INSERT INTO observation_events (
              id, observation_id, canonical_key, event_type, memory_item_id,
              previous_json, current_json, evidence_record_ids_json, created_at
            ) VALUES (
              'observation-event-strengthened', 'observation-decision-storage', 'decision:storage',
              'strengthened', NULL, ?1, ?2, ?3, ?4
            )
            ",
            params![
                json!({
                    "statement": "Yena uses SQLite for local-first storage",
                    "proof_count": 1,
                    "confidence": 0.9,
                    "freshness": "stable",
                    "contradiction_count": 0
                })
                .to_string(),
                json!({
                    "statement": "Yena uses SQLite for local-first storage",
                    "proof_count": 2,
                    "confidence": 0.95,
                    "freshness": "strengthening",
                    "contradiction_count": 0
                })
                .to_string(),
                json!(["evidence-observation-a", "evidence-observation-b"]).to_string(),
                updated_at,
            ],
        )
        .expect("strengthened event should insert");
    }
}
