//! Modern shader manager for Minecraft 1.21.4-26.4 with parallel translation,
//! shader pack support (Complementary, Derivative, Bliss, complex packs),
//! and multi-threaded near-native performance.
//!
//! Modules:
//! - `pack`    — ShaderPack discovery, parsing, and management
//! - `cache`   — Shader source/bytecode cache with content-addressed keys
//! - `pipeline`— Parallel shader translation and compilation pipeline
//! - `modern`  — 1.21.4-26.4 modern shader feature detection and rewrites
//! - `error`   — Shader compile error classification and recovery

use std::collections::HashMap;
use std::hash::Hash;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use parking_lot::RwLock;
use tracing::{debug, info, trace, warn};
use xxhash_rust::xxh3::xxh3_64;

use renderer_core::{BackendError, BackendKind, ShaderId};

mod cache;
mod error;
mod modern;
mod pack;
mod pipeline;

pub use cache::{ShaderCache, ShaderEntry, ShaderKind};
pub use error::ShaderError;
pub use modern::ModernShaderExt;
pub use pack::{ShaderPack, ShaderPackManager, ShaderStage, ShaderVariant};
pub use pipeline::{PipelineConfig, ShaderPipeline, TranslationResult};

// ---- shader pack type detection ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum PackType {
    /// Complementary Reimagined / Complementary Shaders
    Complementary = 1,
    /// Derivative (Chocapic/Sildurs derivative family)
    Derivative = 2,
    /// Bliss Shaders
    Bliss = 3,
    /// BSL Shaders
    Bsl = 4,
    /// SEUS (Sonic Ether's Unbelievable Shaders)
    Seus = 5,
    /// OptiFine / Iris native shader pack
    OptiFine = 6,
    /// Unknown / generic
    Generic = 7,
}

impl PackType {
    pub fn from_name(name: &str) -> Self {
        let n = name.to_ascii_lowercase();
        if n.contains("complementar") { Self::Complementary }
        else if n.contains("derivative") || n.contains("chocapic") { Self::Derivative }
        else if n.contains("bliss") { Self::Bliss }
        else if n.contains("bsl") { Self::Bsl }
        else if n.contains("seus") { Self::Seus }
        else if n.contains("optifine") || n.contains("iris") { Self::OptiFine }
        else { Self::Generic }
    }

    pub fn min_supported_version(&self) -> (u32, u32) {
        match self {
            Self::Complementary => (1, 17),
            Self::Derivative => (1, 14),
            Self::Bliss => (1, 17),
            Self::Bsl => (1, 16),
            Self::Seus => (1, 13),
            Self::OptiFine => (1, 8),
            Self::Generic => (1, 8),
        }
    }
}

// ---- shader source metadata ----

#[derive(Debug, Clone)]
pub struct ShaderSource {
    pub path: PathBuf,
    pub source: String,
    pub stage: ShaderStage,
    pub pack_type: PackType,
    pub hash: u64,
    pub variants: Vec<ShaderVariant>,
}

impl ShaderSource {
    pub fn new(path: PathBuf, source: String, stage: ShaderStage) -> Self {
        let pack_type = PackType::from_name(
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(""),
        );
        let hash = xxh3_64(source.as_bytes());
        Self {
            path,
            source,
            stage,
            pack_type,
            hash,
            variants: Vec::new(),
        }
    }
}

// ---- shader manager config ----

#[derive(Debug, Clone)]
pub struct ShaderManagerConfig {
    /// Enable parallel translation (default: true)
    pub parallel: bool,
    /// Translation thread count (0 = num_cpus::get())
    pub threads: usize,
    /// Cache directory for translated shaders
    pub cache_dir: Option<PathBuf>,
    /// Maximum cache size in bytes (0 = unlimited)
    pub max_cache_bytes: u64,
    /// Translation timeout per shader
    pub timeout: Duration,
    /// Enable modern 1.21.4+ shader rewrites
    pub modern_rewrites: bool,
    /// Shader pack root directories
    pub pack_roots: Vec<PathBuf>,
}

impl Default for ShaderManagerConfig {
    fn default() -> Self {
        Self {
            parallel: true,
            threads: 0,
            cache_dir: None,
            max_cache_bytes: 0,
            timeout: Duration::from_secs(5),
            modern_rewrites: true,
            pack_roots: vec![],
        }
    }
}

// ---- main shader manager ----

pub struct ShaderManager {
    config: ShaderManagerConfig,
    pack_manager: Arc<pack::ShaderPackManager>,
    cache: Arc<RwLock<ShaderCache>>,
    backends: HashMap<BackendKind, Arc<pipeline::ShaderPipeline>>,
}

impl ShaderManager {
    pub fn new(config: ShaderManagerConfig) -> Self {
        let max_cache_bytes = config.max_cache_bytes;
        let pack_manager = Arc::new(pack::ShaderPackManager::new(config.pack_roots.clone()));
        let num_threads = if config.threads == 0 {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
        } else {
            config.threads
        };

        info!(
            threads = num_threads,
            parallel = config.parallel,
            "ShaderManager initialized"
        );

        Self {
            config,
            pack_manager,
            cache: Arc::new(RwLock::new(ShaderCache::new(max_cache_bytes))),
            backends: HashMap::new(),
        }
    }

    pub fn with_pipeline(mut self, backend: BackendKind, pipeline: pipeline::ShaderPipeline) -> Self {
        self.backends.insert(backend, Arc::new(pipeline));
        self
    }

    pub fn pipeline(&self, backend: BackendKind) -> Option<&Arc<pipeline::ShaderPipeline>> {
        self.backends.get(&backend)
    }

    pub fn pack_manager(&self) -> &Arc<pack::ShaderPackManager> {
        &self.pack_manager
    }

    pub fn cache(&self) -> &Arc<RwLock<ShaderCache>> {
        &self.cache
    }

    /// Translate and compile a shader source for the given backend.
    pub fn translate_and_compile(
        &self,
        backend: &dyn renderer_core::Backend,
        source: &str,
        kind: ShaderStage,
    ) -> Result<ShaderId, ShaderError> {
        let hash = xxh3_64(source.as_bytes());

        if let Some(cached) = self.cache.read().get(&hash) {
            trace!("shader cache hit hash={hash:016x}");
        } else {
            trace!("shader cache miss hash={hash:016x}");
        }

        let pipeline = self
            .backends
            .get(&backend.kind())
            .ok_or_else(|| ShaderError::Backend(format!("no pipeline for {:?}", backend.kind())))?;

        pipeline.translate_and_compile(backend, source, kind, &hash)
    }

    /// Batch-translate a list of shaders in parallel.
    pub fn batch_translate(
        &self,
        sources: &[ShaderSource],
    ) -> Vec<TranslationResult> {
        if !self.config.parallel || sources.len() == 1 {
            return sources
                .iter()
                .map(|s| self.translate_single(s))
                .collect();
        }

        #[cfg(feature = "parallel")]
        {
            use rayon::prelude::*;
            sources
                .par_iter()
                .map(|s| self.translate_single(s))
                .collect()
        }
        #[cfg(not(feature = "parallel"))]
        {
            sources.iter().map(|s| self.translate_single(s)).collect()
        }
    }

    fn translate_single(&self, source: &ShaderSource) -> TranslationResult {
        let result = if self.config.modern_rewrites {
            modern::apply_modern_rewrites(&source.source, source.pack_type, source.stage)
        } else {
            Ok(source.source.clone())
        };

        TranslationResult {
            path: source.path.clone(),
            stage: source.stage,
            hash: source.hash,
            translated: result,
            error: None,
        }
    }

    /// Discover all shader packs from configured roots.
    pub fn discover_packs(&self) -> Vec<Arc<pack::ShaderPack>> {
        self.pack_manager.discover_all()
    }

    /// Pre-compile all shaders in a pack for the given backend.
    pub fn precompile_pack(
        &self,
        backend: &dyn renderer_core::Backend,
        pack: &pack::ShaderPack,
    ) -> Vec<Result<ShaderId, ShaderError>> {
        let sources = pack.all_sources();
        let results = self.batch_translate(&sources);
        results
            .into_iter()
            .map(|r| match r.translated {
                Ok(src) => self.translate_and_compile(backend, &src, r.stage),
                Err(e) => Err(ShaderError::Translate(e)),
            })
            .collect()
    }
}

// ---- content-addressed shader key ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShaderKey {
    pub hash: u64,
    pub backend: BackendKind,
    pub stage: ShaderStage,
}

impl ShaderKey {
    pub fn new(hash: u64, backend: BackendKind, stage: ShaderStage) -> Self {
        Self { hash, backend, stage }
    }
}

// ---- cache statistics ----

#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub entries: usize,
    pub total_bytes: u64,
}
