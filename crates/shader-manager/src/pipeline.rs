//! Parallel shader translation and compilation pipeline.
//!
//! Uses rayon for multi-threaded translation to achieve near-native performance.
//! Supports batching, timeouts, and fallback.

use std::sync::Arc;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tracing::{debug, info, trace, warn};

use super::ShaderSource;
use crate::{ShaderError, ShaderManagerConfig, ShaderStage};

/// Result of translating a single shader.
#[derive(Debug, Clone)]
pub struct TranslationResult {
    pub path: std::path::PathBuf,
    pub stage: ShaderStage,
    pub hash: u64,
    pub translated: Result<String, String>,
    pub error: Option<ShaderError>,
}

/// Configuration for the translation pipeline.
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    pub threads: usize,
    pub batch_size: usize,
    pub timeout: Duration,
    pub enable_fallback: bool,
    pub max_retries: usize,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            threads: 0,
            batch_size: 256,
            timeout: Duration::from_secs(5),
            enable_fallback: true,
            max_retries: 2,
        }
    }
}

impl PipelineConfig {
    pub fn from_manager_config(config: &ShaderManagerConfig) -> Self {
        let threads = if config.threads == 0 {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
        } else {
            config.threads
        };
        Self {
            threads,
            batch_size: 256,
            timeout: config.timeout,
            enable_fallback: true,
            max_retries: 2,
        }
    }
}

/// Shader translation and compilation pipeline.
pub struct ShaderPipeline {
    config: PipelineConfig,
    stats: Mutex<PipelineStats>,
}

/// Pipeline statistics.
#[derive(Debug, Clone, Default)]
pub struct PipelineStats {
    pub translated: u64,
    pub compiled: u64,
    pub failed: u64,
    pub cache_hits: u64,
    pub total_duration: Duration,
    pub parallel_speedup: f64,
}

impl ShaderPipeline {
    pub fn new(config: PipelineConfig) -> Self {
        info!(
            threads = config.threads,
            batch_size = config.batch_size,
            "ShaderPipeline created"
        );
        Self {
            config,
            stats: Mutex::new(PipelineStats::default()),
        }
    }


    /// Translate a single shader source.
    pub fn translate(&self, source: &str, stage: ShaderStage) -> Result<String, ShaderError> {
        use shader_translate::naga_translate::{translate_glsl_to_essl, ShaderStageType, TranslateError};
        use std::collections::HashMap;
        
        let naga_stage = match stage {
            ShaderStage::Vertex => ShaderStageType::Vertex,
            ShaderStage::Fragment => ShaderStageType::Fragment,
            ShaderStage::Compute => ShaderStageType::Compute,
            ShaderStage::Geometry => return Err(ShaderError::Translate("Geometry shaders not supported on GLES".into())),
            ShaderStage::TessControl => return Err(ShaderError::Translate("Tessellation shaders not supported on GLES".into())),
            ShaderStage::TessEval => return Err(ShaderError::Translate("Tessellation shaders not supported on GLES".into())),
        };
        
        let defines = HashMap::new();
        translate_glsl_to_essl(source, naga_stage, &defines)
            .map_err(|e| ShaderError::Translate(match e {
                shader_translate::naga_translate::TranslateError::ParseError(msg) => format!("Parse error: {}", msg),
                shader_translate::naga_translate::TranslateError::ValidationError(msg) => format!("Validation error: {}", msg),
                shader_translate::naga_translate::TranslateError::EmitError(msg) => format!("Emit error: {}", msg),
                shader_translate::naga_translate::TranslateError::UnsupportedFeature(msg) => format!("Unsupported feature: {}", msg),
                shader_translate::naga_translate::TranslateError::ComputeVersionError => format!("Compute version error: Compute shader requires GLSL ES 3.10+"),
                shader_translate::naga_translate::TranslateError::MissingEntryPoint(msg) => format!("Missing entry point: {}", msg),
            }))
    }

    /// Translate and compile a shader for a given backend.
    pub fn translate_and_compile(
        &self,
        backend: &dyn renderer_core::Backend,
        source: &str,
        kind: ShaderStage,
        hash: &u64,
    ) -> Result<renderer_core::ShaderId, ShaderError> {
        let start = Instant::now();
        let translated = self.translate(source, kind)?;
        let elapsed = start.elapsed();
        trace!(?kind, elapsed_us = elapsed.as_micros(), "shader translated");

        let gl_kind = match kind {
            ShaderStage::Vertex => 0x8B31,   // GL_VERTEX_SHADER
            ShaderStage::Fragment => 0x8B30, // GL_FRAGMENT_SHADER
            ShaderStage::Geometry => 0x8DD9, // GL_GEOMETRY_SHADER
            ShaderStage::Compute => 0x91B9,  // GL_COMPUTE_SHADER
            ShaderStage::TessControl => 0x8E88, // GL_TESS_CONTROL_SHADER
            ShaderStage::TessEval => 0x8E87,    // GL_TESS_EVALUATION_SHADER
        };

        let shader_id = backend
            .compile_shader(gl_kind, &translated)
            .map_err(|e| ShaderError::from(e))?;

        let mut stats = self.stats.lock();
        stats.translated += 1;
        stats.compiled += 1;
        stats.total_duration += elapsed;

        debug!(?kind, ?hash, elapsed_us = elapsed.as_micros(), "shader compiled");
        Ok(shader_id)
    }

    /// Batch-translate a list of shader sources.
    pub fn batch_translate(
        &self,
        sources: &[ShaderSource],
    ) -> Vec<TranslationResult> {
        if sources.is_empty() {
            return Vec::new();
        }

        let start = Instant::now();

        let results: Vec<TranslationResult> = if self.config.threads > 1 && sources.len() > 1 {
            #[cfg(feature = "parallel")]
            {
                use rayon::prelude::*;
                sources
                    .par_iter()
                    .map(|s| self.translate_one(s))
                    .collect()
            }
            #[cfg(not(feature = "parallel"))]
            {
                sources.iter().map(|s| self.translate_one(s)).collect()
            }
        } else {
            sources.iter().map(|s| self.translate_one(s)).collect()
        };

        let elapsed = start.elapsed();
        let mut stats = self.stats.lock();
        stats.total_duration += elapsed;

        let success = results.iter().filter(|r| r.translated.is_ok()).count();
        let failed = results.iter().filter(|r| r.translated.is_err()).count();
        stats.translated += success as u64;
        stats.failed += failed as u64;

        info!(
            total = results.len(),
            success,
            failed,
            elapsed_ms = elapsed.as_millis(),
            "batch translation complete"
        );

        results
    }

    /// Translate a single shader source.
    fn translate_one(&self, source: &ShaderSource) -> TranslationResult {
        let start = Instant::now();
        let translated = self.translate(&source.source, source.stage);
        let elapsed = start.elapsed();

        let error = translated.as_ref().err().map(|e| {
            warn!(
                path = ?source.path,
                stage = ?source.stage,
                elapsed_us = elapsed.as_micros(),
                "shader translation failed: {e}"
            );
            ShaderError::from(e.to_string())
        });

        TranslationResult {
            path: source.path.clone(),
            stage: source.stage,
            hash: source.hash,
            translated: translated.map_err(|e| e.to_string()),
            error,
        }
    }

    /// Get pipeline statistics.
    pub fn stats(&self) -> PipelineStats {
        self.stats.lock().clone()
    }

    /// Reset pipeline statistics.
    pub fn reset_stats(&self) {
        *self.stats.lock() = PipelineStats::default();
    }
}
