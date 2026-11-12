//! Lazy bge-small-en-v1.5 embeddings. No query prefix.
//!
//! The HackerTube matrix was built with fastembed's quantized ONNX
//! (`Qdrant/bge-small-en-v1.5-onnx-Q`). That file fails on the ort 2 runtime
//! bundled with fastembed-rs (`SkipLayerNormalization` is missing an
//! initializer input). We try it, and if a probe embed fails we use the full
//! `Xenova/bge-small-en-v1.5` export: same 384-d model, CLS pooling, no prefix.
//! The download is cached under `HACKERTUBE_HOME/models`. If neither model can
//! run, search degrades to lexical FTS and says so.

use crate::error::AppError;
use crate::vecstore;
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use std::path::Path;

pub const DIM: usize = 384;

pub struct Embedder {
    model: TextEmbedding,
    id: &'static str,
}

impl Embedder {
    pub fn id(&self) -> &'static str {
        self.id
    }

    pub fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, AppError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let mut vectors = self
            .model
            .embed(texts.to_vec(), None)
            .map_err(|err| AppError::new("embedder", brief(&format!("{err:#}"))))?;
        for vector in &mut vectors {
            if vector.len() != DIM {
                return Err(AppError::new(
                    "embedder",
                    format!("expected {DIM}-d vectors, got {}", vector.len()),
                ));
            }
            vecstore::normalize(vector);
        }
        Ok(vectors)
    }
}

pub enum EmbedStatus {
    Ready(Box<Embedder>),
    Off { notice: String },
}

impl EmbedStatus {
    pub fn load(home: &Path) -> Self {
        if std::env::var("HACKERTUBE_EMBED").ok().as_deref() == Some("off") {
            return EmbedStatus::Off {
                notice: "HACKERTUBE_EMBED=off".to_string(),
            };
        }
        let cache = home.join("models");
        match load_working(&cache) {
            Ok(model) => EmbedStatus::Ready(Box::new(model)),
            Err(err) => EmbedStatus::Off { notice: err.detail },
        }
    }

    /// Model id recorded by the last successful load, without loading ONNX.
    /// `HACKERTUBE_EMBED=off` is `off`. Missing or unknown files are `None`.
    pub fn recorded_model_id(home: &Path) -> Option<String> {
        if std::env::var("HACKERTUBE_EMBED").ok().as_deref() == Some("off") {
            return Some("off".to_string());
        }
        let text = std::fs::read_to_string(home.join("models").join("selected-model")).ok()?;
        let id = text.trim();
        CANDIDATES
            .iter()
            .any(|(_, name)| *name == id)
            .then(|| id.to_string())
    }

    pub fn label(&self) -> &'static str {
        match self {
            EmbedStatus::Ready(_) => "fastembed",
            EmbedStatus::Off { .. } => "off",
        }
    }

    /// Catalog fingerprint. Includes the model id so a fallback change re-embeds.
    pub fn model_id(&self) -> &str {
        match self {
            EmbedStatus::Ready(model) => model.id(),
            EmbedStatus::Off { .. } => "off",
        }
    }

    pub fn notice(&self) -> Option<&str> {
        match self {
            EmbedStatus::Off { notice } => Some(notice.as_str()),
            EmbedStatus::Ready(_) => None,
        }
    }

    pub fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, AppError> {
        match self {
            EmbedStatus::Ready(model) => model.embed(texts),
            EmbedStatus::Off { notice } => Err(AppError::new("embedder", notice.clone())),
        }
    }
}

fn load_working(cache: &Path) -> Result<Embedder, AppError> {
    std::fs::create_dir_all(cache)?;
    let marker = cache.join("selected-model");
    if let Ok(saved) = std::fs::read_to_string(&marker) {
        if let Some((model, id)) = candidate(saved.trim()) {
            if let Ok(embedder) = try_model(cache, model, id) {
                return Ok(embedder);
            }
        }
    }
    let mut errors = Vec::new();
    for (model, id) in CANDIDATES {
        match try_model(cache, model, id) {
            Ok(embedder) => {
                let _ = std::fs::write(&marker, id);
                return Ok(embedder);
            }
            Err(err) => errors.push(format!("{id}: {}", err.detail)),
        }
    }
    Err(AppError::new("embedder", errors.join("; ")))
}

const CANDIDATES: [(EmbeddingModel, &str); 2] = [
    (EmbeddingModel::BGESmallENV15Q, "bge-small-en-v1.5-q"),
    (EmbeddingModel::BGESmallENV15, "bge-small-en-v1.5"),
];

fn candidate(id: &str) -> Option<(EmbeddingModel, &'static str)> {
    CANDIDATES.into_iter().find(|(_, name)| *name == id)
}

fn try_model(cache: &Path, model: EmbeddingModel, id: &'static str) -> Result<Embedder, AppError> {
    let loaded = TextEmbedding::try_new(
        InitOptions::new(model)
            .with_cache_dir(cache.to_path_buf())
            .with_show_download_progress(false),
    )
    .map_err(|err| AppError::new("embedder", brief(&format!("{err:#}"))))?;
    let embedder = Embedder { model: loaded, id };
    // A session can load and still fail the first run (quantized SkipLayerNorm).
    embedder.embed(&["context engineering".to_string()])?;
    Ok(embedder)
}

fn brief(text: &str) -> String {
    let flat = text.replace('\n', " ");
    if flat.chars().count() > 220 {
        let end = flat
            .char_indices()
            .nth(220)
            .map(|(i, _)| i)
            .unwrap_or(flat.len());
        format!("{}…", &flat[..end])
    } else {
        flat
    }
}
