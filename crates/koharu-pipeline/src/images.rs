use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};

use anyhow::{Context as _, Result};
use koharu_scene::{AssetRole, BlobId, EntityId, Snapshot};

/// Decoded images shared by every stage of a run, under a byte budget rather
/// than a blob count so a whole loaded chapter's pages fit: the batch decodes
/// each page once while loading it (its size and thumbnail) and folds that
/// decode in, and the phase-ahead prefetch covers any page the budget has
/// since evicted — a chapter of ordinary manga pages stays resident and the
/// decode happens exactly once per blob. The budget bounds the worst case on
/// a very long volume (the least recently used pages then decode again,
/// hidden behind the current page's stage). A running stage keeps its own
/// `Arc`, so eviction never invalidates an image in use — it only risks a
/// later re-decode.
const RESIDENT_BUDGET_BYTES: usize = 512 * 1024 * 1024;

#[derive(Default)]
pub(crate) struct ImageCache {
    state: Mutex<CacheState>,
}

#[derive(Default)]
struct CacheState {
    cells: HashMap<BlobId, Arc<tokio::sync::OnceCell<Arc<image::DynamicImage>>>>,
    recent: VecDeque<BlobId>,
}

impl ImageCache {
    /// The shared cell for a blob, recorded as most recently used, evicting
    /// the least recent decoded images while the retained ones exceed
    /// `RESIDENT_BUDGET_BYTES` (a cell without an image yet costs nothing).
    /// Concurrent callers racing on a missing blob all receive the same cell,
    /// so the decode still runs exactly once.
    fn cell(&self, blob: BlobId) -> Arc<tokio::sync::OnceCell<Arc<image::DynamicImage>>> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(existing) = state.cells.get(&blob) {
            let existing = Arc::clone(existing);
            state.recent.retain(|&other| other != blob);
            state.recent.push_back(blob);
            return existing;
        }
        let cell = Arc::default();
        state.cells.insert(blob, Arc::clone(&cell));
        state.recent.push_back(blob);
        let mut resident: usize = state
            .recent
            .iter()
            .filter_map(|blob| state.cells.get(blob).and_then(|cell| cell.get()))
            .map(|image| image.as_bytes().len())
            .sum();
        while resident > RESIDENT_BUDGET_BYTES {
            let Some(evicted) = state.recent.pop_front() else {
                break;
            };
            if let Some(cell) = state.cells.remove(&evicted)
                && let Some(image) = cell.get()
            {
                resident -= image.as_bytes().len();
            }
        }
        cell
    }

    /// Folds an image decoded elsewhere (the batch's page load) into the
    /// cache under its content-addressed blob, so the stages reuse that exact
    /// decode instead of decoding the same bytes again. A cell claimed
    /// first keeps its image — the bytes are identical either way.
    pub(crate) fn insert(&self, blob: BlobId, image: Arc<image::DynamicImage>) {
        let cell = self.cell(blob);
        let _ = cell.set(image);
    }

    pub(crate) async fn get(
        &self,
        scene: &Snapshot,
        entity: EntityId,
        role: &str,
    ) -> Result<Option<Arc<image::DynamicImage>>> {
        let role = AssetRole::new(role)?;
        let Some(asset) = scene.asset(entity, &role)? else {
            return Ok(None);
        };
        let cell = self.cell(asset.blob);
        let image = cell
            .get_or_try_init(|| async {
                let bytes = scene.read_blob(asset.blob).await?;
                let role = role.clone();
                tokio_rayon::spawn(move || {
                    image::load_from_memory(&bytes)
                        .map(Arc::new)
                        .with_context(|| {
                            format!(
                                "failed to decode {} image for entity {entity}",
                                role.as_str()
                            )
                        })
                })
                .await
            })
            .await?
            .clone();
        Ok(Some(image))
    }
}
