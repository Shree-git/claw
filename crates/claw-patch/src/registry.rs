use std::collections::HashMap;
use std::sync::Arc;

use crate::codec::Codec;
use crate::PatchError;

pub struct CodecRegistry {
    codecs: HashMap<String, Arc<dyn Codec>>,
    extension_map: HashMap<String, String>,
}

impl CodecRegistry {
    pub fn new() -> Self {
        Self {
            codecs: HashMap::new(),
            extension_map: HashMap::new(),
        }
    }

    pub fn register(&mut self, codec: Arc<dyn Codec>, extensions: &[&str]) {
        let id = codec.id().to_string();
        for ext in extensions {
            self.extension_map.insert(ext.to_string(), id.clone());
        }
        self.codecs.insert(id, codec);
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

    pub fn default_registry() -> Self {
        use crate::json_tree::JsonTreeCodec;
        use crate::text_line::TextLineCodec;

        let mut reg = Self::new();
        reg.register(
            Arc::new(TextLineCodec),
            &["txt", "md", "rs", "py", "js", "ts", "c", "h", "cpp", "go", "rb", "sh", "toml", "yaml", "yml"],
        );
        reg.register(Arc::new(JsonTreeCodec), &["json"]);
        reg
    }
}

impl Default for CodecRegistry {
    fn default() -> Self {
        Self::default_registry()
    }
}
