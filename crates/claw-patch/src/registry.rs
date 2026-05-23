use std::collections::HashMap;
use std::sync::Arc;

use crate::codec::Codec;
use crate::PatchError;
use serde::Serialize;

/// Public inventory row for a registered patch codec.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CodecInfo {
    /// Stable codec identifier stored on patch objects.
    pub id: String,
    /// File extensions routed directly to this codec.
    pub extensions: Vec<String>,
    /// Path heuristics that can route files to this codec ahead of extension matching.
    pub path_matchers: Vec<String>,
    /// Broad codec family for CLI and SDK discovery.
    pub family: &'static str,
    /// Scriptable operation model used by the codec.
    pub operation_model: &'static str,
    /// Whether applying operations emits canonicalized content instead of preserving original formatting.
    pub canonical_output: bool,
    /// Whether this codec is used when no extension or path matcher applies.
    pub fallback: bool,
}

/// Registry for resolving codecs by stable id, file extension, or fallback.
pub struct CodecRegistry {
    codecs: HashMap<String, Arc<dyn Codec>>,
    extension_map: HashMap<String, String>,
    fallback: Option<Arc<dyn Codec>>,
}

impl CodecRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            codecs: HashMap::new(),
            extension_map: HashMap::new(),
            fallback: None,
        }
    }

    /// Register a codec and map the provided extensions to its id.
    pub fn register(&mut self, codec: Arc<dyn Codec>, extensions: &[&str]) {
        let id = codec.id().to_string();
        for ext in extensions {
            self.extension_map.insert(ext.to_string(), id.clone());
        }
        self.codecs.insert(id, codec);
    }

    /// Register the fallback codec used when no extension-specific codec matches.
    pub fn set_fallback(&mut self, codec: Arc<dyn Codec>) {
        let id = codec.id().to_string();
        self.codecs.insert(id, codec.clone());
        self.fallback = Some(codec);
    }

    /// Get a codec by stable id.
    pub fn get(&self, codec_id: &str) -> Result<&Arc<dyn Codec>, PatchError> {
        self.codecs
            .get(codec_id)
            .ok_or_else(|| PatchError::CodecNotFound(codec_id.to_string()))
    }

    /// Get a codec by file extension without the leading dot.
    pub fn get_by_extension(&self, ext: &str) -> Option<&Arc<dyn Codec>> {
        let codec_id = self.extension_map.get(ext)?;
        self.codecs.get(codec_id)
    }

    /// Get the best codec for a file path, falling back when configured.
    pub fn get_for_path(&self, path: &str) -> Option<&Arc<dyn Codec>> {
        let lowered = path.to_ascii_lowercase();
        if is_openapi_path(&lowered) {
            if let Some(codec) = self.codecs.get("openapi/tree") {
                return Some(codec);
            }
        }
        if is_kubernetes_manifest_path(&lowered) {
            if let Some(codec) = self.codecs.get("kubernetes/tree") {
                return Some(codec);
            }
        }
        let ext = path.rsplit('.').next().unwrap_or("");
        self.get_by_extension(ext).or(self.fallback.as_ref())
    }

    /// Return a stable inventory of registered codecs and routing metadata.
    pub fn inventory(&self) -> Vec<CodecInfo> {
        let mut rows = self
            .codecs
            .keys()
            .map(|id| {
                let mut extensions = self
                    .extension_map
                    .iter()
                    .filter_map(|(ext, codec_id)| (codec_id == id).then(|| ext.clone()))
                    .collect::<Vec<_>>();
                extensions.sort();

                CodecInfo {
                    id: id.clone(),
                    extensions,
                    path_matchers: path_matchers_for_codec(id),
                    family: codec_family(id),
                    operation_model: codec_operation_model(id),
                    canonical_output: codec_canonical_output(id),
                    fallback: self
                        .fallback
                        .as_ref()
                        .is_some_and(|codec| codec.id() == id.as_str()),
                }
            })
            .collect::<Vec<_>>();
        rows.sort_by(|left, right| left.id.cmp(&right.id));
        rows
    }

    /// Build the default registry with text, structured, and binary codecs.
    pub fn default_registry() -> Self {
        use crate::binary::BinaryCodec;
        use crate::json_tree::JsonTreeCodec;
        use crate::structured::{
            KubernetesTreeCodec, NotebookTreeCodec, OpenApiTreeCodec, ProtobufAstCodec,
            PythonAstCodec, RustAstCodec, SqlMigrationCodec, TerraformTreeCodec, TomlTreeCodec,
            TypeScriptAstCodec, YamlTreeCodec,
        };
        use crate::text_line::TextLineCodec;

        let mut reg = Self::new();
        reg.register(
            Arc::new(TextLineCodec),
            &["txt", "md", "c", "h", "cpp", "go", "rb", "sh"],
        );
        reg.register(Arc::new(RustAstCodec), &["rs"]);
        reg.register(Arc::new(TypeScriptAstCodec), &["ts", "tsx", "js", "jsx"]);
        reg.register(Arc::new(PythonAstCodec), &["py"]);
        reg.register(Arc::new(SqlMigrationCodec), &["sql"]);
        reg.register(Arc::new(ProtobufAstCodec), &["proto"]);
        reg.register(Arc::new(TerraformTreeCodec), &["tf", "tfvars"]);
        reg.register(Arc::new(TomlTreeCodec), &["toml"]);
        reg.register(Arc::new(YamlTreeCodec), &["yaml", "yml"]);
        reg.register(Arc::new(JsonTreeCodec), &["json"]);
        reg.register(Arc::new(NotebookTreeCodec), &["ipynb"]);
        reg.register(Arc::new(OpenApiTreeCodec), &[]);
        reg.register(Arc::new(KubernetesTreeCodec), &[]);
        reg.set_fallback(Arc::new(BinaryCodec));
        reg
    }
}

fn is_openapi_path(path: &str) -> bool {
    (path.ends_with(".json") || path.ends_with(".yaml") || path.ends_with(".yml"))
        && (path.contains("openapi") || path.contains("swagger"))
}

fn is_kubernetes_manifest_path(path: &str) -> bool {
    let is_structured =
        path.ends_with(".json") || path.ends_with(".yaml") || path.ends_with(".yml");
    is_structured
        && (path.contains("/k8s/")
            || path.contains("/kubernetes/")
            || path.contains("/manifests/")
            || path.contains("deployment.yaml")
            || path.contains("deployment.yml")
            || path.contains("service.yaml")
            || path.contains("service.yml"))
}

fn path_matchers_for_codec(codec_id: &str) -> Vec<String> {
    match codec_id {
        "openapi/tree" => vec![
            "*openapi*.json".to_string(),
            "*openapi*.yaml".to_string(),
            "*openapi*.yml".to_string(),
            "*swagger*.json".to_string(),
            "*swagger*.yaml".to_string(),
            "*swagger*.yml".to_string(),
        ],
        "kubernetes/tree" => vec![
            "*/k8s/*.{json,yaml,yml}".to_string(),
            "*/kubernetes/*.{json,yaml,yml}".to_string(),
            "*/manifests/*.{json,yaml,yml}".to_string(),
            "*deployment.{yaml,yml}".to_string(),
            "*service.{yaml,yml}".to_string(),
        ],
        _ => Vec::new(),
    }
}

fn codec_family(codec_id: &str) -> &'static str {
    match codec_id {
        "binary" => "binary",
        "text/line" => "text",
        "json/tree" | "toml/tree" | "yaml/tree" | "notebook/tree" | "openapi/tree"
        | "kubernetes/tree" | "terraform/tree" => "structured",
        "rust/ast" | "typescript/ast" | "python/ast" | "sql/migration" | "protobuf/ast" => {
            "semantic"
        }
        _ => "custom",
    }
}

fn codec_operation_model(codec_id: &str) -> &'static str {
    match codec_id {
        "binary" => "whole_file",
        "text/line" => "lines",
        "json/tree" | "toml/tree" | "yaml/tree" => "document_tree",
        "notebook/tree" => "notebook_json_tree",
        "openapi/tree" => "openapi_spec_tree",
        "kubernetes/tree" => "kubernetes_manifest_tree",
        "rust/ast" => "rust_top_level_ast_items",
        "typescript/ast" => "typescript_top_level_declarations",
        "python/ast" => "python_top_level_definitions",
        "sql/migration" => "sql_statements",
        "protobuf/ast" => "protobuf_declarations",
        "terraform/tree" => "terraform_hcl_blocks",
        _ => "custom",
    }
}

fn codec_canonical_output(codec_id: &str) -> bool {
    !matches!(codec_id, "binary" | "text/line")
}

impl Default for CodecRegistry {
    fn default() -> Self {
        Self::default_registry()
    }
}

#[cfg(test)]
mod tests {
    use super::CodecRegistry;

    #[test]
    fn registry_routes_structured_extensions() {
        let registry = CodecRegistry::default_registry();
        assert_eq!(
            registry.get_for_path("Claw.toml").unwrap().id(),
            "toml/tree"
        );
        assert_eq!(
            registry.get_for_path("config/settings.yaml").unwrap().id(),
            "yaml/tree"
        );
        assert_eq!(
            registry.get_for_path("analysis/run.ipynb").unwrap().id(),
            "notebook/tree"
        );
        assert_eq!(
            registry.get_for_path("src/lib.rs").unwrap().id(),
            "rust/ast"
        );
        assert_eq!(
            registry.get_for_path("src/app.ts").unwrap().id(),
            "typescript/ast"
        );
        assert_eq!(
            registry.get_for_path("tools/task.py").unwrap().id(),
            "python/ast"
        );
        assert_eq!(
            registry.get_for_path("db/migration.sql").unwrap().id(),
            "sql/migration"
        );
        assert_eq!(
            registry.get_for_path("proto/claw.proto").unwrap().id(),
            "protobuf/ast"
        );
        assert_eq!(
            registry.get_for_path("infra/main.tf").unwrap().id(),
            "terraform/tree"
        );
    }

    #[test]
    fn registry_routes_spec_paths_before_extension_defaults() {
        let registry = CodecRegistry::default_registry();
        assert_eq!(
            registry.get_for_path("api/openapi.yaml").unwrap().id(),
            "openapi/tree"
        );
        assert_eq!(
            registry
                .get_for_path("deploy/k8s/service.yaml")
                .unwrap()
                .id(),
            "kubernetes/tree"
        );
    }

    #[test]
    fn inventory_lists_semantic_codecs_and_path_matchers() {
        let registry = CodecRegistry::default_registry();
        let inventory = registry.inventory();

        let rust = inventory
            .iter()
            .find(|codec| codec.id == "rust/ast")
            .expect("rust codec");
        assert_eq!(rust.family, "semantic");
        assert_eq!(rust.extensions, vec!["rs"]);
        assert_eq!(rust.operation_model, "rust_top_level_ast_items");
        assert!(rust.canonical_output);
        assert!(!rust.fallback);

        let sql = inventory
            .iter()
            .find(|codec| codec.id == "sql/migration")
            .expect("sql codec");
        assert_eq!(sql.operation_model, "sql_statements");

        let terraform = inventory
            .iter()
            .find(|codec| codec.id == "terraform/tree")
            .expect("terraform codec");
        assert_eq!(terraform.operation_model, "terraform_hcl_blocks");

        let openapi = inventory
            .iter()
            .find(|codec| codec.id == "openapi/tree")
            .expect("openapi codec");
        assert_eq!(openapi.family, "structured");
        assert_eq!(openapi.operation_model, "openapi_spec_tree");
        assert!(openapi.extensions.is_empty());
        assert!(openapi
            .path_matchers
            .iter()
            .any(|matcher| matcher.contains("openapi")));

        let binary = inventory
            .iter()
            .find(|codec| codec.id == "binary")
            .expect("binary codec");
        assert_eq!(binary.operation_model, "whole_file");
        assert!(!binary.canonical_output);
        assert!(binary.fallback);
    }
}
