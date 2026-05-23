use clap::{Args, Subcommand};
use serde::Serialize;

use claw_core::id::ObjectId;
use claw_core::object::Object;
use claw_core::types::{Capsule, Evidence};
use claw_store::ClawStore;

use crate::config::find_repo_root;

use super::object_refs::{derive_capsule_trust_score, load_capsule, resolve_object_ref_or_id};

#[derive(Args)]
pub struct EvidenceArgs {
    /// Output command results as JSON
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: EvidenceCommand,
}

#[derive(Subcommand)]
enum EvidenceCommand {
    /// Query capsule evidence, for example: `test=pass AND signer.trust>0.8`
    Query {
        /// Boolean query expression over evidence/capsule fields.
        query: String,
        /// Restrict results to one revision ref or object id.
        #[arg(long)]
        revision: Option<String>,
        /// Restrict results to one capsule ref or object id.
        #[arg(long)]
        capsule: Option<String>,
        /// Maximum number of matching evidence rows to print.
        #[arg(long)]
        limit: Option<usize>,
    },
}

#[derive(Debug, Clone, PartialEq)]
enum Operator {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
}

impl Operator {
    fn as_str(&self) -> &'static str {
        match self {
            Operator::Eq => "=",
            Operator::Ne => "!=",
            Operator::Gt => ">",
            Operator::Ge => ">=",
            Operator::Lt => "<",
            Operator::Le => "<=",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Clause {
    field: String,
    op: Operator,
    value: String,
}

#[derive(Debug, Clone, PartialEq)]
enum QueryExpr {
    Clause(Clause),
    And(Vec<QueryExpr>),
    Or(Vec<QueryExpr>),
}

#[derive(Debug, Clone, PartialEq)]
enum QueryToken {
    Ident(String),
    Op(Operator),
    And,
    Or,
    LParen,
    RParen,
}

#[derive(Debug, Serialize)]
struct QueryPlan {
    expression: String,
    fields: Vec<String>,
    operators: Vec<String>,
    boolean_operators: Vec<String>,
    uses_boolean_logic: bool,
    uses_evidence_fields: bool,
    uses_capsule_fields: bool,
    uses_revision_fields: bool,
    uses_signer_trust: bool,
    revision_filter: Option<String>,
    capsule_filter: Option<String>,
    limit: Option<usize>,
    clauses: Vec<QueryPlanClause>,
}

#[derive(Debug, Clone, Serialize)]
struct QueryPlanClause {
    field: String,
    normalized_field: String,
    scope: String,
    operator: String,
    value: String,
    shorthand: bool,
}

#[derive(Debug, Serialize)]
struct MatchedClause {
    field: String,
    normalized_field: String,
    scope: String,
    operator: String,
    expected: String,
    actual: Vec<String>,
    shorthand: bool,
}

#[derive(Debug, Serialize)]
struct EvidenceMatch {
    capsule_id: String,
    capsule_hex: String,
    revision_id: String,
    revision_hex: String,
    agent_id: String,
    signer_ids: Vec<String>,
    trust_score: Option<f32>,
    matched_clauses: Vec<MatchedClause>,
    evidence: Evidence,
}

pub fn run(args: EvidenceArgs) -> anyhow::Result<()> {
    match args.command {
        EvidenceCommand::Query {
            query,
            revision,
            capsule,
            limit,
        } => run_query(
            &query,
            revision.as_deref(),
            capsule.as_deref(),
            limit,
            args.json,
        ),
    }
}

fn run_query(
    query: &str,
    revision: Option<&str>,
    capsule: Option<&str>,
    limit: Option<usize>,
    json: bool,
) -> anyhow::Result<()> {
    let expr = parse_query(query)?;
    let query_plan = build_query_plan(&expr, query, revision, capsule, limit);
    let root = find_repo_root()?;
    let store = ClawStore::open(&root)?;
    let revision_filter = revision
        .map(|value| resolve_object_ref_or_id(&store, value))
        .transpose()?;

    let capsules = if let Some(value) = capsule {
        let (capsule_id, capsule) = load_capsule(&store, value)?;
        vec![(capsule_id, capsule)]
    } else {
        load_all_capsules(&store)?
    };

    let mut matches = Vec::new();
    for (capsule_id, capsule) in capsules {
        if revision_filter.is_some_and(|expected| expected != capsule.revision_id) {
            continue;
        }
        let trust_score = derive_capsule_trust_score(&capsule);
        for evidence in &capsule.public_fields.evidence {
            if evidence_matches(&expr, &capsule, evidence, trust_score) {
                let matched_clauses =
                    collect_matching_clauses(&expr, &capsule, evidence, trust_score);
                matches.push(EvidenceMatch {
                    capsule_id: capsule_id.to_string(),
                    capsule_hex: capsule_id.to_hex(),
                    revision_id: capsule.revision_id.to_string(),
                    revision_hex: capsule.revision_id.to_hex(),
                    agent_id: capsule.public_fields.agent_id.clone(),
                    signer_ids: capsule
                        .signatures
                        .iter()
                        .map(|signature| signature.signer_id.clone())
                        .collect(),
                    trust_score,
                    matched_clauses,
                    evidence: evidence.clone(),
                });
                if limit.is_some_and(|limit| matches.len() >= limit) {
                    break;
                }
            }
        }
        if limit.is_some_and(|limit| matches.len() >= limit) {
            break;
        }
    }

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "action": "evidence.query",
                "query": query,
                "query_plan": query_plan,
                "count": matches.len(),
                "matches": matches,
            }))?
        );
    } else if matches.is_empty() {
        println!("No evidence matched.");
    } else {
        for item in &matches {
            println!(
                "{} {}={} capsule={} revision={} agent={} trust={}",
                item.evidence.name,
                "status",
                item.evidence.status,
                item.capsule_hex,
                item.revision_hex,
                item.agent_id,
                item.trust_score
                    .map(|score| format!("{score:.2}"))
                    .unwrap_or_else(|| "n/a".to_string())
            );
        }
    }

    Ok(())
}

fn load_all_capsules(store: &ClawStore) -> anyhow::Result<Vec<(ObjectId, Capsule)>> {
    let mut capsules = Vec::new();
    for id in store.list_object_ids()? {
        if let Ok(Object::Capsule(capsule)) = store.load_object(&id) {
            capsules.push((id, capsule));
        }
    }
    Ok(capsules)
}

fn build_query_plan(
    expr: &QueryExpr,
    query: &str,
    revision: Option<&str>,
    capsule: Option<&str>,
    limit: Option<usize>,
) -> QueryPlan {
    let mut clauses = Vec::new();
    collect_query_plan_clauses(expr, &mut clauses);

    let mut fields = Vec::new();
    let mut operators = Vec::new();
    for clause in &clauses {
        push_unique(&mut fields, clause.normalized_field.clone());
        push_unique(&mut operators, clause.operator.clone());
    }

    let mut boolean_operators = Vec::new();
    collect_boolean_operators(expr, &mut boolean_operators);

    QueryPlan {
        expression: query.to_string(),
        fields,
        operators,
        uses_boolean_logic: !boolean_operators.is_empty(),
        uses_evidence_fields: clauses.iter().any(|clause| clause.scope == "evidence"),
        uses_capsule_fields: clauses.iter().any(|clause| clause.scope == "capsule"),
        uses_revision_fields: clauses.iter().any(|clause| clause.scope == "revision"),
        uses_signer_trust: clauses
            .iter()
            .any(|clause| clause.normalized_field == "signer.trust"),
        revision_filter: revision.map(str::to_string),
        capsule_filter: capsule.map(str::to_string),
        limit,
        boolean_operators,
        clauses,
    }
}

fn collect_query_plan_clauses(expr: &QueryExpr, clauses: &mut Vec<QueryPlanClause>) {
    match expr {
        QueryExpr::Clause(clause) => clauses.push(query_plan_clause(clause)),
        QueryExpr::And(items) | QueryExpr::Or(items) => {
            for item in items {
                collect_query_plan_clauses(item, clauses);
            }
        }
    }
}

fn collect_boolean_operators(expr: &QueryExpr, operators: &mut Vec<String>) {
    match expr {
        QueryExpr::Clause(_) => {}
        QueryExpr::And(items) => {
            push_unique(operators, "AND");
            for item in items {
                collect_boolean_operators(item, operators);
            }
        }
        QueryExpr::Or(items) => {
            push_unique(operators, "OR");
            for item in items {
                collect_boolean_operators(item, operators);
            }
        }
    }
}

fn query_plan_clause(clause: &Clause) -> QueryPlanClause {
    let normalized_field = normalize_query_field(&clause.field);
    QueryPlanClause {
        field: clause.field.clone(),
        scope: query_field_scope(&normalized_field).to_string(),
        operator: clause.op.as_str().to_string(),
        value: clause.value.clone(),
        shorthand: normalized_field == "evidence.name_status",
        normalized_field,
    }
}

fn normalize_query_field(field: &str) -> String {
    let field = field.to_ascii_lowercase();
    if is_shorthand_evidence_name(&field) {
        return "evidence.name_status".to_string();
    }

    match field.as_str() {
        "name" | "check" | "evidence.name" => "evidence.name",
        "status" | "evidence.status" => "evidence.status",
        "duration" | "duration_ms" | "evidence.duration_ms" => "evidence.duration_ms",
        "runner" | "runner_identity" | "evidence.runner_identity" => "evidence.runner_identity",
        "command" | "evidence.command" => "evidence.command",
        "summary" | "evidence.summary" => "evidence.summary",
        "exit_code" | "evidence.exit_code" => "evidence.exit_code",
        "started_at" | "started_at_ms" | "evidence.started_at_ms" => "evidence.started_at_ms",
        "ended_at" | "ended_at_ms" | "evidence.ended_at_ms" => "evidence.ended_at_ms",
        "expires_at" | "expires_at_ms" | "evidence.expires_at_ms" => "evidence.expires_at_ms",
        "environment"
        | "environment_digest"
        | "env"
        | "env_digest"
        | "evidence.environment_digest" => "evidence.environment_digest",
        "log_digest" | "evidence.log_digest" => "evidence.log_digest",
        "artifact_digest" | "evidence.artifact_digest" => "evidence.artifact_digest",
        "trust_domain" | "evidence.trust_domain" => "evidence.trust_domain",
        "evidence.revision"
        | "evidence.revision_id"
        | "evidence_revision"
        | "evidence_revision_id" => "evidence.revision_id",
        "signature" | "evidence.signature" | "evidence.signed" => "evidence.signature",
        "agent" | "agent_id" | "agent.id" => "capsule.agent_id",
        "signer" | "signer_id" | "signer.id" => "capsule.signer_id",
        "trust" | "signer.trust" | "capsule.trust" => "signer.trust",
        "revision" | "revision_id" | "capsule.revision_id" => "revision.id",
        "artifact" | "artifact_ref" | "evidence.artifact_ref" => "evidence.artifact_ref",
        _ => field.as_str(),
    }
    .to_string()
}

fn query_field_scope(normalized_field: &str) -> &'static str {
    if normalized_field.starts_with("evidence.") {
        "evidence"
    } else if normalized_field.starts_with("revision.") {
        "revision"
    } else {
        "capsule"
    }
}

fn collect_matching_clauses(
    expr: &QueryExpr,
    capsule: &Capsule,
    evidence: &Evidence,
    trust_score: Option<f32>,
) -> Vec<MatchedClause> {
    let mut clauses = Vec::new();
    collect_matching_clauses_inner(expr, capsule, evidence, trust_score, &mut clauses);
    clauses
}

fn collect_matching_clauses_inner(
    expr: &QueryExpr,
    capsule: &Capsule,
    evidence: &Evidence,
    trust_score: Option<f32>,
    clauses: &mut Vec<MatchedClause>,
) {
    match expr {
        QueryExpr::Clause(clause) => {
            if clause_matches(clause, capsule, evidence, trust_score) {
                clauses.push(matched_clause(clause, capsule, evidence, trust_score));
            }
        }
        QueryExpr::And(items) | QueryExpr::Or(items) => {
            for item in items {
                collect_matching_clauses_inner(item, capsule, evidence, trust_score, clauses);
            }
        }
    }
}

fn matched_clause(
    clause: &Clause,
    capsule: &Capsule,
    evidence: &Evidence,
    trust_score: Option<f32>,
) -> MatchedClause {
    let normalized_field = normalize_query_field(&clause.field);
    MatchedClause {
        field: clause.field.clone(),
        scope: query_field_scope(&normalized_field).to_string(),
        operator: clause.op.as_str().to_string(),
        expected: clause.value.clone(),
        actual: actual_values_for_field(&normalized_field, capsule, evidence, trust_score),
        shorthand: normalized_field == "evidence.name_status",
        normalized_field,
    }
}

fn actual_values_for_field(
    normalized_field: &str,
    capsule: &Capsule,
    evidence: &Evidence,
    trust_score: Option<f32>,
) -> Vec<String> {
    match normalized_field {
        "evidence.name_status" => vec![format!("{}={}", evidence.name, evidence.status)],
        "evidence.name" => vec![evidence.name.clone()],
        "evidence.status" => vec![evidence.status.clone()],
        "evidence.duration_ms" => vec![evidence.duration_ms.to_string()],
        "evidence.runner_identity" => option_string(evidence.runner_identity.as_deref()),
        "evidence.command" => option_string(evidence.command.as_deref()),
        "evidence.summary" => option_string(evidence.summary.as_deref()),
        "evidence.exit_code" => evidence
            .exit_code
            .map(|value| vec![value.to_string()])
            .unwrap_or_default(),
        "evidence.started_at_ms" => evidence
            .started_at_ms
            .map(|value| vec![value.to_string()])
            .unwrap_or_default(),
        "evidence.ended_at_ms" => evidence
            .ended_at_ms
            .map(|value| vec![value.to_string()])
            .unwrap_or_default(),
        "evidence.expires_at_ms" => evidence
            .expires_at_ms
            .map(|value| vec![value.to_string()])
            .unwrap_or_default(),
        "evidence.environment_digest" => option_string(evidence.environment_digest.as_deref()),
        "evidence.log_digest" => option_string(evidence.log_digest.as_deref()),
        "evidence.artifact_digest" => option_string(evidence.artifact_digest.as_deref()),
        "evidence.trust_domain" => option_string(evidence.trust_domain.as_deref()),
        "evidence.revision_id" => evidence
            .revision_id
            .map(|id| vec![id.to_hex(), id.to_string()])
            .unwrap_or_default(),
        "evidence.signature" => vec![if evidence
            .signature
            .as_ref()
            .is_some_and(|bytes| !bytes.is_empty())
        {
            "present"
        } else {
            "missing"
        }
        .to_string()],
        "evidence.artifact_ref" => evidence.artifact_refs.clone(),
        "capsule.agent_id" => vec![capsule.public_fields.agent_id.clone()],
        "capsule.signer_id" => capsule
            .signatures
            .iter()
            .map(|signature| signature.signer_id.clone())
            .collect(),
        "signer.trust" => trust_score
            .map(|score| vec![format!("{score:.3}")])
            .unwrap_or_default(),
        "revision.id" => vec![
            capsule.revision_id.to_hex(),
            capsule.revision_id.to_string(),
        ],
        _ => Vec::new(),
    }
}

fn option_string(value: Option<&str>) -> Vec<String> {
    value
        .map(|value| vec![value.to_string()])
        .unwrap_or_default()
}

fn push_unique(values: &mut Vec<String>, value: impl Into<String>) {
    let value = value.into();
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

fn parse_query(query: &str) -> anyhow::Result<QueryExpr> {
    let tokens = tokenize_query(query)?;
    if tokens.is_empty() {
        anyhow::bail!("evidence query cannot be empty");
    }
    let mut parser = QueryParser { tokens, pos: 0 };
    let expr = parser.parse_or()?;
    if parser.peek().is_some() {
        anyhow::bail!("unexpected token in evidence query");
    }
    Ok(expr)
}

fn tokenize_query(query: &str) -> anyhow::Result<Vec<QueryToken>> {
    let chars = query.chars().collect::<Vec<_>>();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            ch if ch.is_whitespace() => index += 1,
            '(' => {
                tokens.push(QueryToken::LParen);
                index += 1;
            }
            ')' => {
                tokens.push(QueryToken::RParen);
                index += 1;
            }
            '\'' | '"' => {
                let quote = chars[index];
                index += 1;
                let start = index;
                while index < chars.len() && chars[index] != quote {
                    index += 1;
                }
                if index >= chars.len() {
                    anyhow::bail!("unterminated quoted value in evidence query");
                }
                tokens.push(QueryToken::Ident(chars[start..index].iter().collect()));
                index += 1;
            }
            '>' | '<' | '!' | '=' => {
                let op = match chars[index] {
                    '>' if chars.get(index + 1) == Some(&'=') => {
                        index += 2;
                        Operator::Ge
                    }
                    '<' if chars.get(index + 1) == Some(&'=') => {
                        index += 2;
                        Operator::Le
                    }
                    '!' if chars.get(index + 1) == Some(&'=') => {
                        index += 2;
                        Operator::Ne
                    }
                    '=' if chars.get(index + 1) == Some(&'=') => {
                        index += 2;
                        Operator::Eq
                    }
                    '>' => {
                        index += 1;
                        Operator::Gt
                    }
                    '<' => {
                        index += 1;
                        Operator::Lt
                    }
                    '=' => {
                        index += 1;
                        Operator::Eq
                    }
                    _ => anyhow::bail!("invalid evidence query operator"),
                };
                tokens.push(QueryToken::Op(op));
            }
            _ => {
                let start = index;
                while index < chars.len()
                    && !chars[index].is_whitespace()
                    && !matches!(chars[index], '(' | ')' | '>' | '<' | '!' | '=')
                {
                    index += 1;
                }
                let word: String = chars[start..index].iter().collect();
                match word.to_ascii_uppercase().as_str() {
                    "AND" => tokens.push(QueryToken::And),
                    "OR" => tokens.push(QueryToken::Or),
                    _ => tokens.push(QueryToken::Ident(word)),
                }
            }
        }
    }
    Ok(tokens)
}

struct QueryParser {
    tokens: Vec<QueryToken>,
    pos: usize,
}

impl QueryParser {
    fn parse_or(&mut self) -> anyhow::Result<QueryExpr> {
        let mut items = vec![self.parse_and()?];
        while matches!(self.peek(), Some(QueryToken::Or)) {
            self.pos += 1;
            items.push(self.parse_and()?);
        }
        Ok(if items.len() == 1 {
            items.remove(0)
        } else {
            QueryExpr::Or(items)
        })
    }

    fn parse_and(&mut self) -> anyhow::Result<QueryExpr> {
        let mut items = vec![self.parse_primary()?];
        while matches!(self.peek(), Some(QueryToken::And)) {
            self.pos += 1;
            items.push(self.parse_primary()?);
        }
        Ok(if items.len() == 1 {
            items.remove(0)
        } else {
            QueryExpr::And(items)
        })
    }

    fn parse_primary(&mut self) -> anyhow::Result<QueryExpr> {
        if matches!(self.peek(), Some(QueryToken::LParen)) {
            self.pos += 1;
            let expr = self.parse_or()?;
            match self.next() {
                Some(QueryToken::RParen) => Ok(expr),
                _ => anyhow::bail!("missing closing ')' in evidence query"),
            }
        } else {
            Ok(QueryExpr::Clause(self.parse_clause()?))
        }
    }

    fn parse_clause(&mut self) -> anyhow::Result<Clause> {
        let field = match self.next() {
            Some(QueryToken::Ident(value)) if !value.is_empty() => value,
            _ => anyhow::bail!("expected evidence query field"),
        };
        let op = match self.next() {
            Some(QueryToken::Op(op)) => op,
            _ => anyhow::bail!("expected evidence query comparison operator after '{field}'"),
        };
        let value = match self.next() {
            Some(QueryToken::Ident(value)) if !value.is_empty() => value,
            _ => anyhow::bail!("expected evidence query value after '{field}'"),
        };
        Ok(Clause { field, op, value })
    }

    fn peek(&self) -> Option<&QueryToken> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<QueryToken> {
        let token = self.tokens.get(self.pos).cloned();
        if token.is_some() {
            self.pos += 1;
        }
        token
    }
}

fn evidence_matches(
    expr: &QueryExpr,
    capsule: &Capsule,
    evidence: &Evidence,
    trust_score: Option<f32>,
) -> bool {
    match expr {
        QueryExpr::Clause(clause) => clause_matches(clause, capsule, evidence, trust_score),
        QueryExpr::And(items) => items
            .iter()
            .all(|item| evidence_matches(item, capsule, evidence, trust_score)),
        QueryExpr::Or(items) => items
            .iter()
            .any(|item| evidence_matches(item, capsule, evidence, trust_score)),
    }
}

fn clause_matches(
    clause: &Clause,
    capsule: &Capsule,
    evidence: &Evidence,
    trust_score: Option<f32>,
) -> bool {
    let field = clause.field.to_ascii_lowercase();
    if is_shorthand_evidence_name(&field) {
        return evidence.name.eq_ignore_ascii_case(&clause.field)
            && compare_string(&evidence.status, &clause.op, &clause.value);
    }

    match field.as_str() {
        "name" | "evidence.name" | "check" => {
            compare_string(&evidence.name, &clause.op, &clause.value)
        }
        "status" | "evidence.status" => compare_string(&evidence.status, &clause.op, &clause.value),
        "duration" | "duration_ms" | "evidence.duration_ms" => {
            compare_number(evidence.duration_ms as f64, &clause.op, &clause.value)
        }
        "runner" | "runner_identity" | "evidence.runner_identity" => compare_optional_string(
            evidence.runner_identity.as_deref(),
            &clause.op,
            &clause.value,
        ),
        "command" | "evidence.command" => {
            compare_optional_string(evidence.command.as_deref(), &clause.op, &clause.value)
        }
        "summary" | "evidence.summary" => {
            compare_optional_string(evidence.summary.as_deref(), &clause.op, &clause.value)
        }
        "exit_code" | "evidence.exit_code" => evidence
            .exit_code
            .is_some_and(|value| compare_number(value as f64, &clause.op, &clause.value)),
        "started_at" | "started_at_ms" | "evidence.started_at_ms" => evidence
            .started_at_ms
            .is_some_and(|value| compare_number(value as f64, &clause.op, &clause.value)),
        "ended_at" | "ended_at_ms" | "evidence.ended_at_ms" => evidence
            .ended_at_ms
            .is_some_and(|value| compare_number(value as f64, &clause.op, &clause.value)),
        "expires_at" | "expires_at_ms" | "evidence.expires_at_ms" => evidence
            .expires_at_ms
            .is_some_and(|value| compare_number(value as f64, &clause.op, &clause.value)),
        "environment"
        | "environment_digest"
        | "env"
        | "env_digest"
        | "evidence.environment_digest" => compare_optional_string(
            evidence.environment_digest.as_deref(),
            &clause.op,
            &clause.value,
        ),
        "log_digest" | "evidence.log_digest" => {
            compare_optional_string(evidence.log_digest.as_deref(), &clause.op, &clause.value)
        }
        "artifact_digest" | "evidence.artifact_digest" => compare_optional_string(
            evidence.artifact_digest.as_deref(),
            &clause.op,
            &clause.value,
        ),
        "trust_domain" | "evidence.trust_domain" => {
            compare_optional_string(evidence.trust_domain.as_deref(), &clause.op, &clause.value)
        }
        "evidence.revision"
        | "evidence.revision_id"
        | "evidence_revision"
        | "evidence_revision_id" => evidence.revision_id.as_ref().is_some_and(|id| {
            compare_string(&id.to_hex(), &clause.op, &clause.value)
                || compare_string(&id.to_string(), &clause.op, &clause.value)
        }),
        "signature" | "evidence.signature" | "evidence.signed" => compare_bool(
            evidence
                .signature
                .as_ref()
                .is_some_and(|bytes| !bytes.is_empty()),
            &clause.op,
            &clause.value,
        ),
        "agent" | "agent_id" | "agent.id" => {
            compare_string(&capsule.public_fields.agent_id, &clause.op, &clause.value)
        }
        "signer" | "signer_id" | "signer.id" => capsule
            .signatures
            .iter()
            .any(|signature| compare_string(&signature.signer_id, &clause.op, &clause.value)),
        "trust" | "signer.trust" | "capsule.trust" => {
            trust_score.is_some_and(|score| compare_number(score as f64, &clause.op, &clause.value))
        }
        "revision" | "revision_id" | "capsule.revision_id" => {
            compare_string(&capsule.revision_id.to_hex(), &clause.op, &clause.value)
                || compare_string(&capsule.revision_id.to_string(), &clause.op, &clause.value)
        }
        "artifact" | "artifact_ref" | "evidence.artifact_ref" => evidence
            .artifact_refs
            .iter()
            .any(|artifact| compare_string(artifact, &clause.op, &clause.value)),
        _ => false,
    }
}

fn is_shorthand_evidence_name(field: &str) -> bool {
    !matches!(
        field,
        "name"
            | "evidence.name"
            | "check"
            | "status"
            | "evidence.status"
            | "duration"
            | "duration_ms"
            | "evidence.duration_ms"
            | "runner"
            | "runner_identity"
            | "evidence.runner_identity"
            | "command"
            | "evidence.command"
            | "summary"
            | "evidence.summary"
            | "exit_code"
            | "evidence.exit_code"
            | "started_at"
            | "started_at_ms"
            | "evidence.started_at_ms"
            | "ended_at"
            | "ended_at_ms"
            | "evidence.ended_at_ms"
            | "expires_at"
            | "expires_at_ms"
            | "evidence.expires_at_ms"
            | "environment"
            | "environment_digest"
            | "env"
            | "env_digest"
            | "evidence.environment_digest"
            | "log_digest"
            | "evidence.log_digest"
            | "artifact_digest"
            | "evidence.artifact_digest"
            | "trust_domain"
            | "evidence.trust_domain"
            | "evidence.revision"
            | "evidence.revision_id"
            | "evidence_revision"
            | "evidence_revision_id"
            | "signature"
            | "evidence.signature"
            | "evidence.signed"
            | "agent"
            | "agent_id"
            | "agent.id"
            | "signer"
            | "signer_id"
            | "signer.id"
            | "trust"
            | "signer.trust"
            | "capsule.trust"
            | "revision"
            | "revision_id"
            | "capsule.revision_id"
            | "artifact"
            | "artifact_ref"
            | "evidence.artifact_ref"
    )
}

fn compare_optional_string(actual: Option<&str>, op: &Operator, expected: &str) -> bool {
    actual.is_some_and(|actual| compare_string(actual, op, expected))
}

fn compare_bool(actual: bool, op: &Operator, expected: &str) -> bool {
    let expected = match expected.to_ascii_lowercase().as_str() {
        "true" | "yes" | "present" | "signed" | "1" => true,
        "false" | "no" | "missing" | "unsigned" | "0" => false,
        _ => return false,
    };
    match op {
        Operator::Eq => actual == expected,
        Operator::Ne => actual != expected,
        Operator::Gt | Operator::Ge | Operator::Lt | Operator::Le => false,
    }
}

fn compare_string(actual: &str, op: &Operator, expected: &str) -> bool {
    match op {
        Operator::Eq => actual.eq_ignore_ascii_case(expected),
        Operator::Ne => !actual.eq_ignore_ascii_case(expected),
        Operator::Gt | Operator::Ge | Operator::Lt | Operator::Le => actual
            .parse::<f64>()
            .ok()
            .is_some_and(|actual| compare_number_value(actual, op, expected)),
    }
}

fn compare_number(actual: f64, op: &Operator, expected: &str) -> bool {
    compare_number_value(actual, op, expected)
}

fn compare_number_value(actual: f64, op: &Operator, expected: &str) -> bool {
    let Ok(expected) = expected.parse::<f64>() else {
        return false;
    };
    match op {
        Operator::Eq => (actual - expected).abs() < f64::EPSILON,
        Operator::Ne => (actual - expected).abs() >= f64::EPSILON,
        Operator::Gt => actual > expected,
        Operator::Ge => actual >= expected,
        Operator::Lt => actual < expected,
        Operator::Le => actual <= expected,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_query_plan, collect_matching_clauses, evidence_matches, parse_query, Clause,
        Operator, QueryExpr,
    };
    use claw_core::hash::content_hash;
    use claw_core::object::TypeTag;
    use claw_core::types::{Capsule, CapsulePublic, CapsuleSignature, Evidence};

    #[test]
    fn parses_shorthand_and_numeric_clauses() {
        let expr = parse_query("test=pass AND signer.trust>0.8").unwrap();
        assert_eq!(
            expr,
            QueryExpr::And(vec![
                QueryExpr::Clause(Clause {
                    field: "test".to_string(),
                    op: Operator::Eq,
                    value: "pass".to_string(),
                }),
                QueryExpr::Clause(Clause {
                    field: "signer.trust".to_string(),
                    op: Operator::Gt,
                    value: "0.8".to_string(),
                }),
            ])
        );
    }

    #[test]
    fn parses_or_parentheses_and_quoted_values() {
        let expr = parse_query("(test=pass OR lint=pass) AND runner='github actions'").unwrap();
        assert_eq!(
            expr,
            QueryExpr::And(vec![
                QueryExpr::Or(vec![
                    QueryExpr::Clause(Clause {
                        field: "test".to_string(),
                        op: Operator::Eq,
                        value: "pass".to_string(),
                    }),
                    QueryExpr::Clause(Clause {
                        field: "lint".to_string(),
                        op: Operator::Eq,
                        value: "pass".to_string(),
                    }),
                ]),
                QueryExpr::Clause(Clause {
                    field: "runner".to_string(),
                    op: Operator::Eq,
                    value: "github actions".to_string(),
                }),
            ])
        );
    }

    #[test]
    fn builds_query_plan_for_signer_trust_queries() {
        let expr = parse_query("(test=pass OR lint=pass) AND signer.trust>=0.8").unwrap();
        let plan = build_query_plan(
            &expr,
            "(test=pass OR lint=pass) AND signer.trust>=0.8",
            Some("heads/main"),
            None,
            Some(10),
        );

        assert_eq!(
            plan.expression,
            "(test=pass OR lint=pass) AND signer.trust>=0.8"
        );
        assert_eq!(
            plan.fields,
            vec![
                "evidence.name_status".to_string(),
                "signer.trust".to_string()
            ]
        );
        assert_eq!(
            plan.boolean_operators,
            vec!["AND".to_string(), "OR".to_string()]
        );
        assert!(plan.uses_boolean_logic);
        assert!(plan.uses_evidence_fields);
        assert!(plan.uses_capsule_fields);
        assert!(!plan.uses_revision_fields);
        assert!(plan.uses_signer_trust);
        assert_eq!(plan.revision_filter.as_deref(), Some("heads/main"));
        assert_eq!(plan.limit, Some(10));
        assert_eq!(plan.clauses.len(), 3);
        assert!(plan.clauses[0].shorthand);
        assert_eq!(plan.clauses[2].normalized_field, "signer.trust");
        assert_eq!(plan.clauses[2].operator, ">=");
    }

    #[test]
    fn matches_evidence_metadata_fields() {
        let capsule_revision_id = content_hash(TypeTag::Revision, b"capsule revision");
        let evidence_revision_id = content_hash(TypeTag::Revision, b"evidence revision");
        let evidence = Evidence {
            name: "test".to_string(),
            status: "pass".to_string(),
            duration_ms: 42,
            artifact_refs: vec!["artifact://junit.xml".to_string()],
            summary: Some("all checks passed".to_string()),
            revision_id: Some(evidence_revision_id),
            command: Some("cargo test".to_string()),
            exit_code: Some(0),
            started_at_ms: Some(100),
            ended_at_ms: Some(250),
            environment_digest: Some("sha256:env".to_string()),
            runner_identity: Some("github-actions/release".to_string()),
            log_digest: Some("sha256:log".to_string()),
            artifact_digest: Some("sha256:artifact".to_string()),
            expires_at_ms: Some(1_000),
            trust_domain: Some("ci".to_string()),
            signature: Some(vec![1, 2, 3]),
        };
        let capsule = Capsule {
            revision_id: capsule_revision_id,
            public_fields: CapsulePublic {
                agent_id: "agent-1".to_string(),
                agent_version: None,
                toolchain_digest: None,
                env_fingerprint: None,
                evidence: vec![evidence.clone()],
            },
            encrypted_private: None,
            encryption: String::new(),
            key_id: None,
            recipients: Vec::new(),
            signatures: vec![CapsuleSignature {
                signer_id: "release-signer".to_string(),
                signature: vec![4, 5, 6],
            }],
        };

        let query = format!(
            "summary='all checks passed' \
             AND started_at_ms>=100 \
             AND ended_at_ms<300 \
             AND expires_at_ms=1000 \
             AND environment_digest='sha256:env' \
             AND log_digest='sha256:log' \
             AND artifact_digest='sha256:artifact' \
             AND evidence.revision_id={} \
             AND signature=present",
            evidence_revision_id.to_hex()
        );
        let expr = parse_query(&query).unwrap();

        assert!(evidence_matches(&expr, &capsule, &evidence, Some(0.9)));
    }

    #[test]
    fn records_matched_clause_actual_values() {
        let revision_id = content_hash(TypeTag::Revision, b"matched clause revision");
        let evidence = Evidence {
            name: "test".to_string(),
            status: "pass".to_string(),
            duration_ms: 42,
            artifact_refs: Vec::new(),
            summary: None,
            revision_id: None,
            command: None,
            exit_code: None,
            started_at_ms: None,
            ended_at_ms: None,
            environment_digest: None,
            runner_identity: None,
            log_digest: None,
            artifact_digest: None,
            expires_at_ms: None,
            trust_domain: None,
            signature: None,
        };
        let capsule = Capsule {
            revision_id,
            public_fields: CapsulePublic {
                agent_id: "agent-1".to_string(),
                agent_version: None,
                toolchain_digest: None,
                env_fingerprint: None,
                evidence: vec![evidence.clone()],
            },
            encrypted_private: None,
            encryption: String::new(),
            key_id: None,
            recipients: Vec::new(),
            signatures: vec![CapsuleSignature {
                signer_id: "release-signer".to_string(),
                signature: vec![4, 5, 6],
            }],
        };
        let expr = parse_query("(test=pass OR lint=pass) AND signer.trust>=0.8").unwrap();

        let matched = collect_matching_clauses(&expr, &capsule, &evidence, Some(1.0));

        assert_eq!(matched.len(), 2);
        assert_eq!(matched[0].normalized_field, "evidence.name_status");
        assert_eq!(matched[0].actual, vec!["test=pass".to_string()]);
        assert_eq!(matched[1].normalized_field, "signer.trust");
        assert_eq!(matched[1].actual, vec!["1.000".to_string()]);
    }

    #[test]
    fn rejects_missing_or_empty_evidence_signatures() {
        let revision_id = content_hash(TypeTag::Revision, b"unsigned revision");
        let capsule = Capsule {
            revision_id,
            public_fields: CapsulePublic {
                agent_id: "agent-1".to_string(),
                agent_version: None,
                toolchain_digest: None,
                env_fingerprint: None,
                evidence: Vec::new(),
            },
            encrypted_private: None,
            encryption: String::new(),
            key_id: None,
            recipients: Vec::new(),
            signatures: Vec::new(),
        };
        let mut evidence = Evidence {
            name: "test".to_string(),
            status: "pass".to_string(),
            duration_ms: 0,
            artifact_refs: Vec::new(),
            summary: None,
            revision_id: None,
            command: None,
            exit_code: None,
            started_at_ms: None,
            ended_at_ms: None,
            environment_digest: None,
            runner_identity: None,
            log_digest: None,
            artifact_digest: None,
            expires_at_ms: None,
            trust_domain: None,
            signature: None,
        };
        let signed = parse_query("signature=present").unwrap();
        let unsigned = parse_query("signature=missing").unwrap();

        assert!(!evidence_matches(&signed, &capsule, &evidence, None));
        assert!(evidence_matches(&unsigned, &capsule, &evidence, None));

        evidence.signature = Some(Vec::new());
        assert!(!evidence_matches(&signed, &capsule, &evidence, None));
        assert!(evidence_matches(&unsigned, &capsule, &evidence, None));
    }
}
