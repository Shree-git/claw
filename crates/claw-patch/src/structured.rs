//! Structured document codecs built on JSON-pointer-style tree operations.
//!
//! These codecs intentionally produce canonical output instead of preserving
//! original formatting or comments. They are useful for semantic conflict
//! detection in configuration-like files while the richer format-preserving
//! codecs are still experimental.

use claw_core::types::PatchOp;

use crate::codec::Codec;
use crate::json_tree::JsonTreeCodec;
use crate::PatchError;

#[derive(Debug, Clone, Copy)]
enum StructuredFormat {
    Toml,
    Yaml,
    Json,
}

#[derive(Debug, Clone, Copy)]
enum SemanticFormat {
    Rust,
    TypeScript,
    Python,
    Sql,
    Protobuf,
    Terraform,
}

/// TOML codec using JSON-pointer-style operations over parsed document values.
pub struct TomlTreeCodec;

/// YAML codec using JSON-pointer-style operations over parsed document values.
pub struct YamlTreeCodec;

/// Jupyter notebook codec using JSON-pointer-style operations over notebook JSON.
pub struct NotebookTreeCodec;

/// OpenAPI/Swagger codec using JSON-pointer-style operations over spec documents.
pub struct OpenApiTreeCodec;

/// Kubernetes manifest codec using JSON-pointer-style operations over manifests.
pub struct KubernetesTreeCodec;

/// Rust source codec using top-level item AST-style operations.
pub struct RustAstCodec;

/// TypeScript/JavaScript source codec using declaration AST-style operations.
pub struct TypeScriptAstCodec;

/// Python source codec using top-level definition AST-style operations.
pub struct PythonAstCodec;

/// SQL migration codec using statement-level operations.
pub struct SqlMigrationCodec;

/// Protobuf codec using message/enum/service/option/import operations.
pub struct ProtobufAstCodec;

/// Terraform codec using top-level HCL block operations.
pub struct TerraformTreeCodec;

impl Codec for TomlTreeCodec {
    fn id(&self) -> &str {
        "toml/tree"
    }

    fn diff(&self, old: &[u8], new: &[u8]) -> Result<Vec<PatchOp>, PatchError> {
        diff_structured(StructuredFormat::Toml, old, new)
    }

    fn apply(&self, base: &[u8], ops: &[PatchOp]) -> Result<Vec<u8>, PatchError> {
        apply_structured(StructuredFormat::Toml, base, ops)
    }

    fn invert(&self, ops: &[PatchOp]) -> Result<Vec<PatchOp>, PatchError> {
        JsonTreeCodec.invert(ops)
    }

    fn commute(
        &self,
        left: &[PatchOp],
        right: &[PatchOp],
    ) -> Result<(Vec<PatchOp>, Vec<PatchOp>), PatchError> {
        JsonTreeCodec.commute(left, right)
    }

    fn merge3(&self, base: &[u8], left: &[u8], right: &[u8]) -> Result<Vec<u8>, PatchError> {
        merge3_structured(StructuredFormat::Toml, base, left, right)
    }
}

impl Codec for YamlTreeCodec {
    fn id(&self) -> &str {
        "yaml/tree"
    }

    fn diff(&self, old: &[u8], new: &[u8]) -> Result<Vec<PatchOp>, PatchError> {
        diff_structured(StructuredFormat::Yaml, old, new)
    }

    fn apply(&self, base: &[u8], ops: &[PatchOp]) -> Result<Vec<u8>, PatchError> {
        apply_structured(StructuredFormat::Yaml, base, ops)
    }

    fn invert(&self, ops: &[PatchOp]) -> Result<Vec<PatchOp>, PatchError> {
        JsonTreeCodec.invert(ops)
    }

    fn commute(
        &self,
        left: &[PatchOp],
        right: &[PatchOp],
    ) -> Result<(Vec<PatchOp>, Vec<PatchOp>), PatchError> {
        JsonTreeCodec.commute(left, right)
    }

    fn merge3(&self, base: &[u8], left: &[u8], right: &[u8]) -> Result<Vec<u8>, PatchError> {
        merge3_structured(StructuredFormat::Yaml, base, left, right)
    }
}

impl Codec for NotebookTreeCodec {
    fn id(&self) -> &str {
        "notebook/tree"
    }

    fn diff(&self, old: &[u8], new: &[u8]) -> Result<Vec<PatchOp>, PatchError> {
        diff_structured(StructuredFormat::Json, old, new)
    }

    fn apply(&self, base: &[u8], ops: &[PatchOp]) -> Result<Vec<u8>, PatchError> {
        apply_structured(StructuredFormat::Json, base, ops)
    }

    fn invert(&self, ops: &[PatchOp]) -> Result<Vec<PatchOp>, PatchError> {
        JsonTreeCodec.invert(ops)
    }

    fn commute(
        &self,
        left: &[PatchOp],
        right: &[PatchOp],
    ) -> Result<(Vec<PatchOp>, Vec<PatchOp>), PatchError> {
        JsonTreeCodec.commute(left, right)
    }

    fn merge3(&self, base: &[u8], left: &[u8], right: &[u8]) -> Result<Vec<u8>, PatchError> {
        merge3_structured(StructuredFormat::Json, base, left, right)
    }
}

impl Codec for OpenApiTreeCodec {
    fn id(&self) -> &str {
        "openapi/tree"
    }

    fn diff(&self, old: &[u8], new: &[u8]) -> Result<Vec<PatchOp>, PatchError> {
        diff_detecting_json_or_yaml(old, new)
    }

    fn apply(&self, base: &[u8], ops: &[PatchOp]) -> Result<Vec<u8>, PatchError> {
        apply_detecting_json_or_yaml(base, ops)
    }

    fn invert(&self, ops: &[PatchOp]) -> Result<Vec<PatchOp>, PatchError> {
        JsonTreeCodec.invert(ops)
    }

    fn commute(
        &self,
        left: &[PatchOp],
        right: &[PatchOp],
    ) -> Result<(Vec<PatchOp>, Vec<PatchOp>), PatchError> {
        JsonTreeCodec.commute(left, right)
    }

    fn merge3(&self, base: &[u8], left: &[u8], right: &[u8]) -> Result<Vec<u8>, PatchError> {
        merge3_detecting_json_or_yaml(base, left, right)
    }
}

impl Codec for KubernetesTreeCodec {
    fn id(&self) -> &str {
        "kubernetes/tree"
    }

    fn diff(&self, old: &[u8], new: &[u8]) -> Result<Vec<PatchOp>, PatchError> {
        diff_detecting_json_or_yaml(old, new)
    }

    fn apply(&self, base: &[u8], ops: &[PatchOp]) -> Result<Vec<u8>, PatchError> {
        apply_detecting_json_or_yaml(base, ops)
    }

    fn invert(&self, ops: &[PatchOp]) -> Result<Vec<PatchOp>, PatchError> {
        JsonTreeCodec.invert(ops)
    }

    fn commute(
        &self,
        left: &[PatchOp],
        right: &[PatchOp],
    ) -> Result<(Vec<PatchOp>, Vec<PatchOp>), PatchError> {
        JsonTreeCodec.commute(left, right)
    }

    fn merge3(&self, base: &[u8], left: &[u8], right: &[u8]) -> Result<Vec<u8>, PatchError> {
        merge3_detecting_json_or_yaml(base, left, right)
    }
}

impl Codec for RustAstCodec {
    fn id(&self) -> &str {
        "rust/ast"
    }

    fn diff(&self, old: &[u8], new: &[u8]) -> Result<Vec<PatchOp>, PatchError> {
        diff_semantic(SemanticFormat::Rust, old, new)
    }

    fn apply(&self, base: &[u8], ops: &[PatchOp]) -> Result<Vec<u8>, PatchError> {
        apply_semantic(SemanticFormat::Rust, base, ops)
    }

    fn invert(&self, ops: &[PatchOp]) -> Result<Vec<PatchOp>, PatchError> {
        JsonTreeCodec.invert(ops)
    }

    fn commute(
        &self,
        left: &[PatchOp],
        right: &[PatchOp],
    ) -> Result<(Vec<PatchOp>, Vec<PatchOp>), PatchError> {
        JsonTreeCodec.commute(left, right)
    }

    fn merge3(&self, base: &[u8], left: &[u8], right: &[u8]) -> Result<Vec<u8>, PatchError> {
        merge3_semantic(SemanticFormat::Rust, base, left, right)
    }
}

impl Codec for TypeScriptAstCodec {
    fn id(&self) -> &str {
        "typescript/ast"
    }

    fn diff(&self, old: &[u8], new: &[u8]) -> Result<Vec<PatchOp>, PatchError> {
        diff_semantic(SemanticFormat::TypeScript, old, new)
    }

    fn apply(&self, base: &[u8], ops: &[PatchOp]) -> Result<Vec<u8>, PatchError> {
        apply_semantic(SemanticFormat::TypeScript, base, ops)
    }

    fn invert(&self, ops: &[PatchOp]) -> Result<Vec<PatchOp>, PatchError> {
        JsonTreeCodec.invert(ops)
    }

    fn commute(
        &self,
        left: &[PatchOp],
        right: &[PatchOp],
    ) -> Result<(Vec<PatchOp>, Vec<PatchOp>), PatchError> {
        JsonTreeCodec.commute(left, right)
    }

    fn merge3(&self, base: &[u8], left: &[u8], right: &[u8]) -> Result<Vec<u8>, PatchError> {
        merge3_semantic(SemanticFormat::TypeScript, base, left, right)
    }
}

impl Codec for PythonAstCodec {
    fn id(&self) -> &str {
        "python/ast"
    }

    fn diff(&self, old: &[u8], new: &[u8]) -> Result<Vec<PatchOp>, PatchError> {
        diff_semantic(SemanticFormat::Python, old, new)
    }

    fn apply(&self, base: &[u8], ops: &[PatchOp]) -> Result<Vec<u8>, PatchError> {
        apply_semantic(SemanticFormat::Python, base, ops)
    }

    fn invert(&self, ops: &[PatchOp]) -> Result<Vec<PatchOp>, PatchError> {
        JsonTreeCodec.invert(ops)
    }

    fn commute(
        &self,
        left: &[PatchOp],
        right: &[PatchOp],
    ) -> Result<(Vec<PatchOp>, Vec<PatchOp>), PatchError> {
        JsonTreeCodec.commute(left, right)
    }

    fn merge3(&self, base: &[u8], left: &[u8], right: &[u8]) -> Result<Vec<u8>, PatchError> {
        merge3_semantic(SemanticFormat::Python, base, left, right)
    }
}

impl Codec for SqlMigrationCodec {
    fn id(&self) -> &str {
        "sql/migration"
    }

    fn diff(&self, old: &[u8], new: &[u8]) -> Result<Vec<PatchOp>, PatchError> {
        diff_semantic(SemanticFormat::Sql, old, new)
    }

    fn apply(&self, base: &[u8], ops: &[PatchOp]) -> Result<Vec<u8>, PatchError> {
        apply_semantic(SemanticFormat::Sql, base, ops)
    }

    fn invert(&self, ops: &[PatchOp]) -> Result<Vec<PatchOp>, PatchError> {
        JsonTreeCodec.invert(ops)
    }

    fn commute(
        &self,
        left: &[PatchOp],
        right: &[PatchOp],
    ) -> Result<(Vec<PatchOp>, Vec<PatchOp>), PatchError> {
        JsonTreeCodec.commute(left, right)
    }

    fn merge3(&self, base: &[u8], left: &[u8], right: &[u8]) -> Result<Vec<u8>, PatchError> {
        merge3_semantic(SemanticFormat::Sql, base, left, right)
    }
}

impl Codec for ProtobufAstCodec {
    fn id(&self) -> &str {
        "protobuf/ast"
    }

    fn diff(&self, old: &[u8], new: &[u8]) -> Result<Vec<PatchOp>, PatchError> {
        diff_semantic(SemanticFormat::Protobuf, old, new)
    }

    fn apply(&self, base: &[u8], ops: &[PatchOp]) -> Result<Vec<u8>, PatchError> {
        apply_semantic(SemanticFormat::Protobuf, base, ops)
    }

    fn invert(&self, ops: &[PatchOp]) -> Result<Vec<PatchOp>, PatchError> {
        JsonTreeCodec.invert(ops)
    }

    fn commute(
        &self,
        left: &[PatchOp],
        right: &[PatchOp],
    ) -> Result<(Vec<PatchOp>, Vec<PatchOp>), PatchError> {
        JsonTreeCodec.commute(left, right)
    }

    fn merge3(&self, base: &[u8], left: &[u8], right: &[u8]) -> Result<Vec<u8>, PatchError> {
        merge3_semantic(SemanticFormat::Protobuf, base, left, right)
    }
}

impl Codec for TerraformTreeCodec {
    fn id(&self) -> &str {
        "terraform/tree"
    }

    fn diff(&self, old: &[u8], new: &[u8]) -> Result<Vec<PatchOp>, PatchError> {
        diff_semantic(SemanticFormat::Terraform, old, new)
    }

    fn apply(&self, base: &[u8], ops: &[PatchOp]) -> Result<Vec<u8>, PatchError> {
        apply_semantic(SemanticFormat::Terraform, base, ops)
    }

    fn invert(&self, ops: &[PatchOp]) -> Result<Vec<PatchOp>, PatchError> {
        JsonTreeCodec.invert(ops)
    }

    fn commute(
        &self,
        left: &[PatchOp],
        right: &[PatchOp],
    ) -> Result<(Vec<PatchOp>, Vec<PatchOp>), PatchError> {
        JsonTreeCodec.commute(left, right)
    }

    fn merge3(&self, base: &[u8], left: &[u8], right: &[u8]) -> Result<Vec<u8>, PatchError> {
        merge3_semantic(SemanticFormat::Terraform, base, left, right)
    }
}

fn diff_structured(
    format: StructuredFormat,
    old: &[u8],
    new: &[u8],
) -> Result<Vec<PatchOp>, PatchError> {
    let old_json = parse_to_json_bytes(format, old)?;
    let new_json = parse_to_json_bytes(format, new)?;
    JsonTreeCodec.diff(&old_json, &new_json)
}

fn apply_structured(
    format: StructuredFormat,
    base: &[u8],
    ops: &[PatchOp],
) -> Result<Vec<u8>, PatchError> {
    let base_json = parse_to_json_bytes(format, base)?;
    let applied = JsonTreeCodec.apply(&base_json, ops)?;
    render_from_json_bytes(format, &applied)
}

fn merge3_structured(
    format: StructuredFormat,
    base: &[u8],
    left: &[u8],
    right: &[u8],
) -> Result<Vec<u8>, PatchError> {
    let base_json = parse_to_json_bytes(format, base)?;
    let left_json = parse_to_json_bytes(format, left)?;
    let right_json = parse_to_json_bytes(format, right)?;
    let merged = JsonTreeCodec.merge3(&base_json, &left_json, &right_json)?;
    render_from_json_bytes(format, &merged)
}

fn parse_to_json_bytes(format: StructuredFormat, input: &[u8]) -> Result<Vec<u8>, PatchError> {
    let value = match format {
        StructuredFormat::Toml => {
            let text = std::str::from_utf8(input)
                .map_err(|err| PatchError::ApplyFailed(format!("invalid toml utf-8: {err}")))?;
            toml::from_str::<serde_json::Value>(text)
                .map_err(|err| PatchError::ApplyFailed(format!("invalid toml: {err}")))?
        }
        StructuredFormat::Yaml => serde_yaml::from_slice::<serde_json::Value>(input)
            .map_err(|err| PatchError::ApplyFailed(format!("invalid yaml: {err}")))?,
        StructuredFormat::Json => serde_json::from_slice::<serde_json::Value>(input)
            .map_err(|err| PatchError::InvalidJson(err.to_string()))?,
    };
    serde_json::to_vec(&value).map_err(|err| PatchError::ApplyFailed(err.to_string()))
}

fn render_from_json_bytes(format: StructuredFormat, input: &[u8]) -> Result<Vec<u8>, PatchError> {
    let value: serde_json::Value =
        serde_json::from_slice(input).map_err(|err| PatchError::InvalidJson(err.to_string()))?;
    let rendered = match format {
        StructuredFormat::Toml => toml::to_string_pretty(&value)
            .map_err(|err| PatchError::ApplyFailed(format!("cannot render toml: {err}")))?,
        StructuredFormat::Yaml => serde_yaml::to_string(&value)
            .map_err(|err| PatchError::ApplyFailed(format!("cannot render yaml: {err}")))?,
        StructuredFormat::Json => serde_json::to_string_pretty(&value)
            .map_err(|err| PatchError::ApplyFailed(err.to_string()))?,
    };
    Ok(rendered.into_bytes())
}

fn detect_json_or_yaml(input: &[u8]) -> StructuredFormat {
    let trimmed = input
        .iter()
        .copied()
        .skip_while(|byte| byte.is_ascii_whitespace())
        .next();
    match trimmed {
        Some(b'{') | Some(b'[') => StructuredFormat::Json,
        _ => StructuredFormat::Yaml,
    }
}

fn diff_detecting_json_or_yaml(old: &[u8], new: &[u8]) -> Result<Vec<PatchOp>, PatchError> {
    let old_format = detect_json_or_yaml(old);
    let new_format = detect_json_or_yaml(new);
    let old_json = parse_to_json_bytes(old_format, old)?;
    let new_json = parse_to_json_bytes(new_format, new)?;
    JsonTreeCodec.diff(&old_json, &new_json)
}

fn apply_detecting_json_or_yaml(base: &[u8], ops: &[PatchOp]) -> Result<Vec<u8>, PatchError> {
    let format = detect_json_or_yaml(base);
    apply_structured(format, base, ops)
}

fn merge3_detecting_json_or_yaml(
    base: &[u8],
    left: &[u8],
    right: &[u8],
) -> Result<Vec<u8>, PatchError> {
    let format = detect_json_or_yaml(base);
    let base_json = parse_to_json_bytes(format, base)?;
    let left_json = parse_to_json_bytes(detect_json_or_yaml(left), left)?;
    let right_json = parse_to_json_bytes(detect_json_or_yaml(right), right)?;
    let merged = JsonTreeCodec.merge3(&base_json, &left_json, &right_json)?;
    render_from_json_bytes(format, &merged)
}

fn diff_semantic(
    format: SemanticFormat,
    old: &[u8],
    new: &[u8],
) -> Result<Vec<PatchOp>, PatchError> {
    let old_tree = parse_semantic_to_json_bytes(format, old)?;
    let new_tree = parse_semantic_to_json_bytes(format, new)?;
    JsonTreeCodec.diff(&old_tree, &new_tree)
}

fn apply_semantic(
    format: SemanticFormat,
    base: &[u8],
    ops: &[PatchOp],
) -> Result<Vec<u8>, PatchError> {
    let base_tree = parse_semantic_to_json_bytes(format, base)?;
    let applied = JsonTreeCodec.apply(&base_tree, ops)?;
    render_semantic_from_json_bytes(format, &applied)
}

fn merge3_semantic(
    format: SemanticFormat,
    base: &[u8],
    left: &[u8],
    right: &[u8],
) -> Result<Vec<u8>, PatchError> {
    let base_tree = parse_semantic_to_json_bytes(format, base)?;
    let left_tree = parse_semantic_to_json_bytes(format, left)?;
    let right_tree = parse_semantic_to_json_bytes(format, right)?;
    let merged = JsonTreeCodec.merge3(&base_tree, &left_tree, &right_tree)?;
    render_semantic_from_json_bytes(format, &merged)
}

fn parse_semantic_to_json_bytes(
    format: SemanticFormat,
    input: &[u8],
) -> Result<Vec<u8>, PatchError> {
    let text = std::str::from_utf8(input)
        .map_err(|err| PatchError::ApplyFailed(format!("invalid source utf-8: {err}")))?;
    let tree = match format {
        SemanticFormat::Rust => {
            source_items_tree("rust/ast", parse_brace_language_items(format, text))
        }
        SemanticFormat::TypeScript => {
            source_items_tree("typescript/ast", parse_brace_language_items(format, text))
        }
        SemanticFormat::Python => source_items_tree("python/ast", parse_python_items(text)),
        SemanticFormat::Sql => source_items_tree("sql/migration", parse_sql_statements(text)),
        SemanticFormat::Protobuf => {
            source_items_tree("protobuf/ast", parse_brace_language_items(format, text))
        }
        SemanticFormat::Terraform => {
            source_items_tree("terraform/tree", parse_brace_language_items(format, text))
        }
    };
    serde_json::to_vec(&tree).map_err(|err| PatchError::ApplyFailed(err.to_string()))
}

fn render_semantic_from_json_bytes(
    format: SemanticFormat,
    input: &[u8],
) -> Result<Vec<u8>, PatchError> {
    let value: serde_json::Value =
        serde_json::from_slice(input).map_err(|err| PatchError::InvalidJson(err.to_string()))?;
    let mut rendered = String::new();
    let codec = semantic_codec_id(format);
    rendered.push_str(&format!("// claw-canonical-codec: {codec}\n\n"));
    let Some(order) = value.get("order").and_then(serde_json::Value::as_array) else {
        return Ok(rendered.into_bytes());
    };
    let items = value
        .get("items")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| PatchError::ApplyFailed("semantic tree missing items".to_string()))?;

    for key in order {
        let Some(key) = key.as_str() else {
            continue;
        };
        let Some(item) = items.get(key) else {
            continue;
        };
        if let Some(body) = item.get("body").and_then(serde_json::Value::as_str) {
            rendered.push_str(body.trim_end());
            rendered.push_str("\n\n");
        }
    }

    Ok(rendered.into_bytes())
}

fn semantic_codec_id(format: SemanticFormat) -> &'static str {
    match format {
        SemanticFormat::Rust => "rust/ast",
        SemanticFormat::TypeScript => "typescript/ast",
        SemanticFormat::Python => "python/ast",
        SemanticFormat::Sql => "sql/migration",
        SemanticFormat::Protobuf => "protobuf/ast",
        SemanticFormat::Terraform => "terraform/tree",
    }
}

#[derive(Debug, Clone)]
struct SemanticItem {
    key: String,
    kind: String,
    name: String,
    signature: String,
    body: String,
}

fn source_items_tree(codec: &str, items: Vec<SemanticItem>) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    let mut order = Vec::new();
    for (ordinal, item) in items.into_iter().enumerate() {
        order.push(serde_json::Value::String(item.key.clone()));
        map.insert(
            item.key,
            serde_json::json!({
                "kind": item.kind,
                "name": item.name,
                "signature": item.signature,
                "body": item.body,
                "ordinal": ordinal,
            }),
        );
    }
    serde_json::json!({
        "codec": codec,
        "order": order,
        "items": map,
    })
}

fn parse_brace_language_items(format: SemanticFormat, text: &str) -> Vec<SemanticItem> {
    let lines = text.lines().collect::<Vec<_>>();
    let mut starts = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        if leading_spaces(line) == 0 {
            if let Some((kind, name)) = classify_brace_item(format, line.trim()) {
                starts.push((idx, kind, name));
            }
        }
    }
    if starts.is_empty() && !text.trim().is_empty() {
        return vec![raw_item("document", "root", text)];
    }

    let mut items = Vec::new();
    for pos in 0..starts.len() {
        let (start, kind, name) = &starts[pos];
        let end = starts
            .get(pos + 1)
            .map(|(next, _, _)| *next)
            .unwrap_or(lines.len());
        let body = lines[*start..end].join("\n");
        items.push(SemanticItem {
            key: unique_key(&items, kind, name),
            kind: kind.clone(),
            name: name.clone(),
            signature: lines[*start].trim().to_string(),
            body,
        });
    }
    items
}

fn parse_python_items(text: &str) -> Vec<SemanticItem> {
    let lines = text.lines().collect::<Vec<_>>();
    let mut starts = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        if leading_spaces(line) == 0 {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("def ") {
                starts.push((idx, "function".to_string(), take_ident(rest)));
            } else if let Some(rest) = trimmed.strip_prefix("async def ") {
                starts.push((idx, "function".to_string(), take_ident(rest)));
            } else if let Some(rest) = trimmed.strip_prefix("class ") {
                starts.push((idx, "class".to_string(), take_ident(rest)));
            }
        }
    }
    if starts.is_empty() && !text.trim().is_empty() {
        return vec![raw_item("module", "root", text)];
    }

    let mut items = Vec::new();
    for pos in 0..starts.len() {
        let (start, kind, name) = &starts[pos];
        let end = starts
            .get(pos + 1)
            .map(|(next, _, _)| *next)
            .unwrap_or(lines.len());
        let body = lines[*start..end].join("\n");
        items.push(SemanticItem {
            key: unique_key(&items, kind, name),
            kind: kind.clone(),
            name: name.clone(),
            signature: lines[*start].trim().to_string(),
            body,
        });
    }
    items
}

fn parse_sql_statements(text: &str) -> Vec<SemanticItem> {
    let mut items = Vec::new();
    for statement in text.split(';') {
        let body = statement.trim();
        if body.is_empty() {
            continue;
        }
        let normalized = body
            .split_whitespace()
            .map(|part| part.trim_matches('"').to_ascii_lowercase())
            .collect::<Vec<_>>();
        let (kind, name) = classify_sql_statement(&normalized);
        items.push(SemanticItem {
            key: unique_key(&items, &kind, &name),
            kind,
            name,
            signature: normalized
                .iter()
                .take(6)
                .cloned()
                .collect::<Vec<_>>()
                .join(" "),
            body: format!("{body};"),
        });
    }
    if items.is_empty() && !text.trim().is_empty() {
        return vec![raw_item("migration", "root", text)];
    }
    items
}

fn classify_brace_item(format: SemanticFormat, line: &str) -> Option<(String, String)> {
    match format {
        SemanticFormat::Rust => classify_keyword_item(
            line,
            &[
                ("fn", "function"),
                ("struct", "struct"),
                ("enum", "enum"),
                ("trait", "trait"),
                ("impl", "impl"),
                ("mod", "module"),
                ("type", "type"),
                ("const", "const"),
                ("static", "static"),
            ],
        ),
        SemanticFormat::TypeScript => classify_keyword_item(
            line,
            &[
                ("function", "function"),
                ("class", "class"),
                ("interface", "interface"),
                ("type", "type"),
                ("const", "const"),
                ("let", "binding"),
                ("var", "binding"),
                ("enum", "enum"),
            ],
        ),
        SemanticFormat::Protobuf => classify_keyword_item(
            line,
            &[
                ("message", "message"),
                ("enum", "enum"),
                ("service", "service"),
                ("rpc", "rpc"),
                ("option", "option"),
                ("import", "import"),
                ("syntax", "syntax"),
            ],
        ),
        SemanticFormat::Terraform => classify_terraform_item(line),
        _ => None,
    }
}

fn classify_keyword_item(line: &str, keywords: &[(&str, &str)]) -> Option<(String, String)> {
    let cleaned = line
        .trim_start_matches("pub ")
        .trim_start_matches("export ")
        .trim_start_matches("default ")
        .trim_start_matches("async ");
    for (keyword, kind) in keywords {
        if let Some(rest) = cleaned.strip_prefix(&format!("{keyword} ")) {
            return Some(((*kind).to_string(), take_ident(rest)));
        }
    }
    None
}

fn classify_terraform_item(line: &str) -> Option<(String, String)> {
    let mut parts = line.split_whitespace().collect::<Vec<_>>();
    if parts.is_empty() {
        return None;
    }
    let kind = parts.remove(0).trim_matches('"');
    match kind {
        "resource" | "data" => {
            let labels = parts
                .iter()
                .take(2)
                .map(|part| part.trim_matches('"').trim_end_matches('{'))
                .collect::<Vec<_>>();
            Some((kind.to_string(), labels.join(".")))
        }
        "module" | "variable" | "output" | "provider" => parts.first().map(|label| {
            (
                kind.to_string(),
                label.trim_matches('"').trim_end_matches('{').to_string(),
            )
        }),
        "terraform" | "locals" => Some((kind.to_string(), kind.to_string())),
        _ => None,
    }
}

fn classify_sql_statement(parts: &[String]) -> (String, String) {
    match parts {
        [a, b, c, rest @ ..] if a == "create" && b == "table" => {
            ("create_table".to_string(), table_name(c, rest))
        }
        [a, b, c, rest @ ..] if a == "alter" && b == "table" => {
            ("alter_table".to_string(), table_name(c, rest))
        }
        [a, b, c, rest @ ..] if a == "create" && b == "index" => {
            ("create_index".to_string(), table_name(c, rest))
        }
        [a, b, c, ..] if a == "insert" && b == "into" => ("insert".to_string(), c.clone()),
        [a, b, ..] => (format!("{a}_{b}"), b.clone()),
        [a] => (a.clone(), "statement".to_string()),
        [] => ("statement".to_string(), "empty".to_string()),
    }
}

fn table_name(first: &str, rest: &[String]) -> String {
    if first == "if" && rest.len() >= 3 && rest[0] == "not" && rest[1] == "exists" {
        rest[2].trim_matches('(').to_string()
    } else {
        first.trim_matches('(').to_string()
    }
}

fn raw_item(kind: &str, name: &str, body: &str) -> SemanticItem {
    SemanticItem {
        key: format!("{kind}:{name}"),
        kind: kind.to_string(),
        name: name.to_string(),
        signature: body.lines().next().unwrap_or("").trim().to_string(),
        body: body.trim_end().to_string(),
    }
}

fn unique_key(existing: &[SemanticItem], kind: &str, name: &str) -> String {
    let base = format!("{kind}:{name}");
    if !existing.iter().any(|item| item.key == base) {
        return base;
    }
    let mut suffix = 2;
    loop {
        let candidate = format!("{base}#{suffix}");
        if !existing.iter().any(|item| item.key == candidate) {
            return candidate;
        }
        suffix += 1;
    }
}

fn take_ident(input: &str) -> String {
    input
        .trim_start_matches("r#")
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '-')
        .collect::<String>()
        .trim_end_matches(':')
        .to_string()
}

fn leading_spaces(line: &str) -> usize {
    line.chars()
        .take_while(|ch| ch.is_ascii_whitespace())
        .count()
}

#[cfg(test)]
mod tests {
    use super::{
        ProtobufAstCodec, RustAstCodec, SqlMigrationCodec, TerraformTreeCodec, TomlTreeCodec,
        TypeScriptAstCodec, YamlTreeCodec,
    };
    use crate::Codec;

    #[test]
    fn toml_codec_roundtrips_structural_updates() {
        let codec = TomlTreeCodec;
        let old = b"name = \"claw\"\ncount = 1\n";
        let new = b"name = \"claw\"\ncount = 2\n";

        let ops = codec.diff(old, new).expect("diff toml");
        assert!(ops.iter().any(|op| op.address == "/count"));
        let applied = codec.apply(old, &ops).expect("apply toml");
        let applied_value: toml::Value =
            toml::from_str(std::str::from_utf8(&applied).expect("utf8")).expect("toml");
        assert_eq!(applied_value["count"].as_integer(), Some(2));
    }

    #[test]
    fn yaml_codec_merges_independent_paths() {
        let codec = YamlTreeCodec;
        let base = b"name: claw\ncount: 1\n";
        let left = b"name: claw-vcs\ncount: 1\n";
        let right = b"name: claw\ncount: 2\n";

        let merged = codec.merge3(base, left, right).expect("merge yaml");
        let value: serde_yaml::Value = serde_yaml::from_slice(&merged).expect("yaml");
        assert_eq!(value["name"].as_str(), Some("claw-vcs"));
        assert_eq!(value["count"].as_i64(), Some(2));
    }

    #[test]
    fn rust_ast_codec_diffs_by_top_level_function() {
        let codec = RustAstCodec;
        let old = b"fn alpha() {\n    one();\n}\n\nfn beta() {\n    two();\n}\n";
        let new = b"fn alpha() {\n    changed();\n}\n\nfn beta() {\n    two();\n}\n";

        let ops = codec.diff(old, new).expect("diff rust");
        assert!(ops
            .iter()
            .any(|op| op.address == "/items/function:alpha/body"));
        assert!(!ops
            .iter()
            .any(|op| op.address.contains("function:beta/body")));
    }

    #[test]
    fn typescript_ast_codec_commutes_independent_declarations() {
        let codec = TypeScriptAstCodec;
        let base = b"export function alpha() {\n  return 1;\n}\n\nexport function beta() {\n  return 2;\n}\n";
        let left = b"export function alpha() {\n  return 10;\n}\n\nexport function beta() {\n  return 2;\n}\n";
        let right = b"export function alpha() {\n  return 1;\n}\n\nexport function beta() {\n  return 20;\n}\n";
        codec.merge3(base, left, right).expect("merge typescript");
    }

    #[test]
    fn sql_codec_routes_statement_changes_by_table() {
        let codec = SqlMigrationCodec;
        let old = b"CREATE TABLE users (id INT);\nCREATE TABLE teams (id INT);\n";
        let new = b"CREATE TABLE users (id INT, email TEXT);\nCREATE TABLE teams (id INT);\n";
        let ops = codec.diff(old, new).expect("diff sql");
        assert!(ops
            .iter()
            .any(|op| op.address == "/items/create_table:users/body"));
    }

    #[test]
    fn protobuf_and_terraform_codecs_identify_named_blocks() {
        let proto = ProtobufAstCodec;
        let proto_ops = proto
            .diff(
                b"message User {\n  string id = 1;\n}\n",
                b"message User {\n  string id = 1;\n  string email = 2;\n}\n",
            )
            .expect("diff proto");
        assert!(proto_ops
            .iter()
            .any(|op| op.address == "/items/message:User/body"));

        let tf = TerraformTreeCodec;
        let tf_ops = tf
            .diff(
                b"resource \"aws_s3_bucket\" \"logs\" {\n  bucket = \"a\"\n}\n",
                b"resource \"aws_s3_bucket\" \"logs\" {\n  bucket = \"b\"\n}\n",
            )
            .expect("diff terraform");
        assert!(tf_ops
            .iter()
            .any(|op| op.address == "/items/resource:aws_s3_bucket.logs/body"));
    }
}
