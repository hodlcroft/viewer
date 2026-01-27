//! Image fetching stage of the pipeline.

use crate::NormalizedAsset;
use crate::ipfs::{FetchError, IpfsFetcher};
use crate::pipeline::Pipeline;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Semaphore;
use tracing::{debug, trace, warn};

/// Result of fetching images for a collection.
#[derive(Debug)]
pub struct FetchResult {
    pub fetched: usize,
    pub skipped: usize,
    pub failed: usize,
}

/// Progress callback for reporting fetch progress to the CLI.
pub type ProgressCallback = Box<dyn Fn(usize, usize, usize, usize) + Send + Sync>;

/// Fetch all raw images for a collection.
///
/// - Downloads to the `raw/` directory in original format
/// - Skips images that already exist
/// - Saves state periodically for resumability
/// - Calls progress callback for CLI output
pub async fn fetch_images(
    pipeline: &mut Pipeline,
    assets: &[NormalizedAsset],
    on_progress: Option<ProgressCallback>,
) -> Result<FetchResult, FetchError> {
    let fetcher = Arc::new(IpfsFetcher::new(pipeline.config.fetch_concurrency));

    let fetched = Arc::new(AtomicUsize::new(0));
    let skipped = Arc::new(AtomicUsize::new(0));
    let failed = Arc::new(AtomicUsize::new(0));

    let total = assets.len();
    debug!("Starting fetch of {} images", total);

    // Process in batches for progress reporting and state saving
    let batch_size = 100;
    let semaphore = Arc::new(Semaphore::new(pipeline.config.fetch_concurrency));

    for (batch_idx, batch) in assets.chunks(batch_size).enumerate() {
        let mut handles = Vec::with_capacity(batch.len());

        for asset in batch {
            // Skip if already fetched
            if pipeline.raw_exists(&asset.encoded_name).is_some() {
                skipped.fetch_add(1, Ordering::Relaxed);
                continue;
            }

            let Some(ref url) = asset.image_url else {
                warn!(encoded_name = %asset.encoded_name, "Asset has no image URL");
                failed.fetch_add(1, Ordering::Relaxed);
                continue;
            };

            let permit = semaphore.clone().acquire_owned().await.unwrap();
            let fetcher = fetcher.clone();
            let url = url.clone();
            let encoded_name = asset.encoded_name.clone();
            let raw_dir = pipeline.dirs.raw.clone();
            let fetched = fetched.clone();
            let failed = failed.clone();

            handles.push(tokio::spawn(async move {
                let _permit = permit;

                match fetcher.fetch_image(&url).await {
                    Ok(image) => {
                        let path = raw_dir.join(format!(
                            "{}.{}",
                            encoded_name,
                            image.format.extension()
                        ));

                        if let Err(e) = std::fs::write(&path, &image.bytes) {
                            warn!(path = %path.display(), error = %e, "Failed to write image");
                            failed.fetch_add(1, Ordering::Relaxed);
                        } else {
                            trace!(path = %path.display(), bytes = image.bytes.len(), "Saved image");
                            fetched.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    Err(e) => {
                        warn!(encoded_name = %encoded_name, error = %e, "Failed to fetch image");
                        failed.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }));
        }

        // Wait for batch to complete
        for handle in handles {
            let _ = handle.await;
        }

        // Update state and report progress
        let current_fetched = fetched.load(Ordering::Relaxed);
        let current_skipped = skipped.load(Ordering::Relaxed);
        let current_failed = failed.load(Ordering::Relaxed);
        let processed = current_fetched + current_skipped + current_failed;

        pipeline.state.images_fetched = current_fetched + current_skipped;
        pipeline.state.images_failed = current_failed;

        // Report progress via callback (for CLI output)
        if let Some(ref callback) = on_progress {
            if (batch_idx + 1) % 5 == 0 || processed == total {
                callback(processed, total, current_fetched, current_failed);
            }
        }

        // Save state periodically
        if (batch_idx + 1) % 10 == 0 {
            pipeline.save_state().ok();
        }
    }

    // Final state save
    pipeline.save_state().ok();

    let result = FetchResult {
        fetched: fetched.load(Ordering::Relaxed),
        skipped: skipped.load(Ordering::Relaxed),
        failed: failed.load(Ordering::Relaxed),
    };

    debug!(
        fetched = result.fetched,
        skipped = result.skipped,
        failed = result.failed,
        "Image fetch complete"
    );

    Ok(result)
}
