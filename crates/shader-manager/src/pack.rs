//! Shader pack discovery, parsing, and management.
//!
//! Supports modern shader pack formats:
//! - Complementary Reimagined/Complementary (1.17+)
//! - Derivative (Chocapic/Sildurs, 1.14+)
//! - Bliss (1.17+)
//! - BSL (1.16+)
//! - SEUS (1.13+)
//! - OptiFine / Iris native (1.8+)
//! - Generic packs (1.21.4-26.4)

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dashmap::DashMap;

use super::{PackType, ShaderSource};

/// A single shader file within a pack.
#[derive(Debug, Clone)]
pub struct ShaderFile {
    pub path: PathBuf,
    pub stage: ShaderStage,
    pub pack_type: PackType,
}

impl ShaderFile {
    pub fn new(path: PathBuf, stage: ShaderStage, pack_type: PackType) -> Self {
        Self { path, stage, pack_type }
    }
}

/// A shader pack containing vertex, fragment, geometry, compute, and tessellation shaders.
#[derive(Debug, Clone)]
pub struct ShaderPack {
    pub name: String,
    pub root: PathBuf,
    pub pack_type: PackType,
    pub description: String,
    pub version: String,
    pub min_mc_version: (u32, u32),
    pub max_mc_version: (u32, u32),
    pub files: DashMap<PathBuf, ShaderFile>,
    pub enabled: bool,
    pub supports_modern: bool,
}

impl ShaderPack {
    pub fn new(name: impl Into<String>, root: PathBuf, pack_type: PackType) -> Self {
        let name = name.into();
        let min_ver = pack_type.min_supported_version();
        Self {
            name,
            root,
            pack_type,
            description: String::new(),
            version: String::new(),
            min_mc_version: min_ver,
            max_mc_version: (26, 4),
            files: DashMap::new(),
            enabled: true,
            supports_modern: true,
        }
    }

    /// Load shader metadata from pack files.
    pub fn load_metadata(&mut self) {
        let pack_txt = self.root.join("pack.txt");
        if let Ok(data) = std::fs::read_to_string(pack_txt) {
            self.description = data.lines().next().unwrap_or("").to_string();
        }
        let pack_mcmeta = self.root.join("pack.mcmeta");
        if let Ok(data) = std::fs::read_to_string(pack_mcmeta) {
            if let Ok(meta) = serde_json::from_str::<serde_json::Value>(&data) {
                self.description = meta
                    .get("pack")
                    .and_then(|p| p.get("description"))
                    .and_then(|d| d.as_str())
                    .unwrap_or(&self.description)
                    .to_string();
                self.version = meta
                    .get("pack")
                    .and_then(|p| p.get("pack_format"))
                    .and_then(|f| f.as_u64())
                    .map(|f| f.to_string())
                    .unwrap_or_default();
            }
        }
    }

    /// Scan the pack directory for shader files.
    pub fn scan(&self) {
        let stages = [
            ("shaders/vertex", ShaderStage::Vertex),
            ("shaders/geometry", ShaderStage::Geometry),
            ("shaders/fragment", ShaderStage::Fragment),
            ("shaders/compute", ShaderStage::Compute),
            ("shaders/tesscontrol", ShaderStage::TessControl),
            ("shaders/tesseval", ShaderStage::TessEval),
        ];

        for (subdir, stage) in &stages {
            let dir = self.root.join(subdir);
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if let Some(ext) = path.extension() {
                        if ext == "vsh" || ext == "fsh" || ext == "gsh" || ext == "csh"
                            || ext == "vsh.glsl" || ext == "fsh.glsl"
                            || ext == "glsl"
                        {
                            self.files.insert(
                                path.clone(),
                                ShaderFile::new(path, *stage, self.pack_type),
                            );
                        }
                    }
                }
            }
        }
    }

    /// Load all shader sources in this pack.
    pub fn all_sources(&self) -> Vec<super::ShaderSource> {
        let mut out = Vec::with_capacity(self.files.len());
        for entry in self.files.iter() {
            if let Ok(data) = std::fs::read_to_string(&entry.path) {
                let src = super::ShaderSource::new(
                    entry.path.clone(),
                    data,
                    entry.stage,
                );
                out.push(src);
            }
        }
        out
    }

    /// Get a specific shader by path.
    pub fn get_shader(&self, path: &Path) -> Option<ShaderFile> {
        self.files.get(path).map(|e| e.clone())
    }

    /// Find all shaders matching a variant.
    pub fn find_by_variant(&self, variant: ShaderVariant) -> Vec<ShaderFile> {
        self.files
            .iter()
            .filter(|e| {
                let stem = e.path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                match variant {
                    ShaderVariant::Gbuffers => stem.contains("gbuffers"),
                    ShaderVariant::Shadow => stem.contains("shadow"),
                    ShaderVariant::Composite => stem.contains("composite"),
                    ShaderVariant::Prepare => stem.contains("prepare"),
                    ShaderVariant::Deferred => stem.contains("deferred"),
                    ShaderVariant::World => stem.contains("world"),
                    ShaderVariant::Entity => stem.contains("entity"),
                    ShaderVariant::Hand => stem.contains("hand"),
                    ShaderVariant::Sky => stem.contains("sky"),
                    ShaderVariant::Clouds => stem.contains("cloud"),
                    ShaderVariant::Weather => stem.contains("weather"),
                    ShaderVariant::Atmosphere => stem.contains("atmosphere"),
                    ShaderVariant::Unknown => true,
                }
            })
            .map(|e| e.clone())
            .collect()
    }

    pub fn shader_count(&self) -> usize {
        self.files.len()
    }
}

/// Shader stage (vertex, fragment, geometry, etc.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum ShaderStage {
    Vertex = 0,
    Fragment = 1,
    Geometry = 2,
    Compute = 3,
    TessControl = 4,
    TessEval = 5,
}

impl std::fmt::Display for ShaderStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Vertex => write!(f, "vertex"),
            Self::Fragment => write!(f, "fragment"),
            Self::Geometry => write!(f, "geometry"),
            Self::Compute => write!(f, "compute"),
            Self::TessControl => write!(f, "tesscontrol"),
            Self::TessEval => write!(f, "tesseval"),
        }
    }
}

/// Shader variant for modern shader packs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum ShaderVariant {
    Gbuffers = 0,
    Shadow = 1,
    Composite = 2,
    Prepare = 3,
    Deferred = 4,
    World = 5,
    Entity = 6,
    Hand = 7,
    Sky = 8,
    Clouds = 9,
    Weather = 10,
    Atmosphere = 11,
    Unknown = 255,
}

impl std::fmt::Display for ShaderVariant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Gbuffers => write!(f, "gbuffers"),
            Self::Shadow => write!(f, "shadow"),
            Self::Composite => write!(f, "composite"),
            Self::Prepare => write!(f, "prepare"),
            Self::Deferred => write!(f, "deferred"),
            Self::World => write!(f, "world"),
            Self::Entity => write!(f, "entity"),
            Self::Hand => write!(f, "hand"),
            Self::Sky => write!(f, "sky"),
            Self::Clouds => write!(f, "clouds"),
            Self::Weather => write!(f, "weather"),
            Self::Atmosphere => write!(f, "atmosphere"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

/// Manages all shader packs across configured directories.
pub struct ShaderPackManager {
    roots: Vec<PathBuf>,
    packs: DashMap<String, Arc<ShaderPack>>,
    discovered: std::sync::atomic::AtomicBool,
}

impl ShaderPackManager {
    pub fn new(roots: Vec<PathBuf>) -> Self {
        Self {
            roots,
            packs: DashMap::new(),
            discovered: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Discover all shader packs from all configured roots.
    pub fn discover_all(&self) -> Vec<Arc<ShaderPack>> {
        if self.discovered.load(std::sync::atomic::Ordering::Relaxed) {
            return self.packs.iter().map(|e| e.value().clone()).collect();
        }

        for root in &self.roots {
            self.discover_root(root);
        }
        self.discovered.store(true, std::sync::atomic::Ordering::Relaxed);
        self.packs.iter().map(|e| e.value().clone()).collect()
    }

    fn discover_root(&self, root: &Path) {
        if !root.exists() {
            return;
        }
        let entries = match std::fs::read_dir(root) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                let pack_type = PackType::from_name(&name);
                let mut pack = ShaderPack::new(name, path.clone(), pack_type);
                pack.load_metadata();
                pack.scan();
                self.packs.insert(pack.name.clone(), Arc::new(pack));
            }
        }
    }

    /// Get a pack by name.
    pub fn get_pack(&self, name: &str) -> Option<Arc<ShaderPack>> {
        self.packs.get(name).map(|e| e.value().clone())
    }

    /// Register a shader pack manually.
    pub fn register_pack(&self, pack: Arc<ShaderPack>) {
        self.packs.insert(pack.name.clone(), pack);
    }

    /// List all registered pack names.
    pub fn pack_names(&self) -> Vec<String> {
        self.packs.iter().map(|e| e.key().clone()).collect()
    }
}
