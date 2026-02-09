use std::collections::HashMap;
use std::sync::Arc;

use crate::codec::Codec;
use crate::PatchError;

pub struct CodecRegistry {
    codecs: HashMap<String, Arc<dyn Codec>>,
    extension_map: HashMap<String, String>,
    fallback: Option<Arc<dyn Codec>>,
}

impl CodecRegistry {
    pub fn new() -> Self {
        Self {
            codecs: HashMap::new(),
            extension_map: HashMap::new(),
            fallback: None,
        }
    }

    pub fn register(&mut self, codec: Arc<dyn Codec>, extensions: &[&str]) {
        let id = codec.id().to_string();
        for ext in extensions {
            self.extension_map.insert(ext.to_string(), id.clone());
        }
        self.codecs.insert(id, codec);
    }

    pub fn set_fallback(&mut self, codec: Arc<dyn Codec>) {
        let id = codec.id().to_string();
        self.codecs.insert(id, codec.clone());
        self.fallback = Some(codec);
    }

    pub fn get(&self, codec_id: &str) -> Result<&Arc<dyn Codec>, PatchError> {
        self.codecs
            .get(codec_id)
            .ok_or_else(|| PatchError::CodecNotFound(codec_id.to_string()))
    }

    pub fn get_by_extension(&self, ext: &str) -> Option<&Arc<dyn Codec>> {
        let codec_id = self.extension_map.get(ext)?;
        self.codecs.get(codec_id)
    }

    pub fn get_for_path(&self, path: &str) -> Option<&Arc<dyn Codec>> {
        let ext = path.rsplit('.').next().unwrap_or("");
        self.get_by_extension(ext).or(self.fallback.as_ref())
    }

    pub fn default_registry() -> Self {
        use crate::binary::BinaryCodec;
        use crate::json_tree::JsonTreeCodec;
        use crate::text_line::TextLineCodec;

        let mut reg = Self::new();
        reg.register(
            Arc::new(TextLineCodec),
            &["txt", "md", "rs", "py", "js", "ts", "c", "h", "cpp", "go", "rb", "sh", "toml", "yaml", "yml"],
        );
        reg.register(Arc::new(JsonTreeCodec), &["json"]);
        reg.set_fallback(Arc::new(BinaryCodec));
        reg
    }
}

impl Default for CodecRegistry {
    fn default() -> Self {
        Self::default_registry()
    }
}
