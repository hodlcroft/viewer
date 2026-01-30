//! Image fetching stage of the pipeline.

use crate::NormalizedAsset;
use crate::ipfs::{FetchError, Gateway, IpfsFetcher};
use crate::nftcdn::NftcdnClient;
use crate::pipeline::Pipeline;
use cardano_assets::AssetId;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Semaphore;
use tracing::{debug, info, trace, warn};
use viewer_format::ImageSourceConfig;

/// Result of fetching images for a collection.
#[derive(Debug)]
pub struct FetchResult {
    pub fetched: usize,
    pub skipped: usize,
    pub failed: Vec<String>,
}

/// Progress callback for reporting fetch progress to the CLI.
pub type ProgressCallback = Box<dyn Fn(usize, usize, usize, usize) + Send + Sync>;

/// Fetch all raw images for a collection.
///
/// - Downloads to the `raw/` directory in original format
/// - Skips images that already exist
/// - Saves state periodically for resumability
/// - Calls progress callback for CLI output
///
/// If `image_config` specifies custom gateways, those are used instead of the default gateways.
pub async fn fetch_images(
    pipeline: &mut Pipeline,
    assets: &[NormalizedAsset],
    image_config: &ImageSourceConfig,
    on_progress: Option<ProgressCallback>,
) -> Result<FetchResult, FetchError> {
    // Create fetcher with custom gateways if specified, otherwise use defaults
    let fetcher = if image_config.gateways.is_empty() {
        IpfsFetcher::new(pipeline.config.fetch_concurrency)
    } else {
        info!(
            gateways = ?image_config.gateways,
            "Using custom IPFS gateways for collection"
        );
        let custom_gateways: Vec<Gateway> = image_config
            .gateways
            .iter()
            .map(|&gw| Gateway::from(gw))
            .collect();
        IpfsFetcher::with_gateways(pipeline.config.fetch_concurrency, custom_gateways)
    };
    let fetcher = Arc::new(fetcher);

    let fetched = Arc::new(AtomicUsize::new(0));
    let skipped = Arc::new(AtomicUsize::new(0));
    let failed_count = Arc::new(AtomicUsize::new(0));
    let failed_ids: Arc<std::sync::Mutex<Vec<String>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));

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
                failed_count.fetch_add(1, Ordering::Relaxed);
                failed_ids.lock().unwrap().push(asset.encoded_name.clone());
                continue;
            };

            let permit = semaphore.clone().acquire_owned().await.unwrap();
            let fetcher = fetcher.clone();
            let url = url.clone();
            let encoded_name = asset.encoded_name.clone();
            let raw_dir = pipeline.dirs.raw.clone();
            let fetched = fetched.clone();
            let failed_count = failed_count.clone();
            let failed_ids = failed_ids.clone();

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
                            failed_count.fetch_add(1, Ordering::Relaxed);
                            failed_ids.lock().unwrap().push(encoded_name);
                        } else {
                            trace!(path = %path.display(), bytes = image.bytes.len(), "Saved image");
                            fetched.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    Err(e) => {
                        warn!(encoded_name = %encoded_name, error = %e, "Failed to fetch image");
                        failed_count.fetch_add(1, Ordering::Relaxed);
                        failed_ids.lock().unwrap().push(encoded_name);
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
        let current_failed = failed_count.load(Ordering::Relaxed);
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
        failed: Arc::try_unwrap(failed_ids).unwrap().into_inner().unwrap(),
    };

    debug!(
        fetched = result.fetched,
        skipped = result.skipped,
        failed = result.failed.len(),
        "Image fetch complete"
    );

    Ok(result)
}

/// Fetch all images for a collection from IIIF server.
///
/// - Downloads to the `raw/` directory as JPEG
/// - Skips images that already exist
/// - Saves state periodically for resumability
/// - Calls progress callback for CLI output (every 50 images)
pub async fn fetch_images_iiif(
    pipeline: &mut Pipeline,
    assets: &[NormalizedAsset],
    policy_id: &str,
    image_config: &ImageSourceConfig,
    on_progress: Option<ProgressCallback>,
) -> Result<FetchResult, FetchError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("Failed to create HTTP client");
    let client = Arc::new(client);

    let fetched = Arc::new(AtomicUsize::new(0));
    let skipped = Arc::new(AtomicUsize::new(0));
    let failed_count = Arc::new(AtomicUsize::new(0));
    let failed_ids: Arc<std::sync::Mutex<Vec<String>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    let last_reported = Arc::new(AtomicUsize::new(0));

    let total = assets.len();
    debug!("Starting IIIF fetch of {} images", total);

    // Progress reporting interval
    const PROGRESS_INTERVAL: usize = 50;

    // Process in batches for state saving
    let batch_size = 100;
    let semaphore = Arc::new(Semaphore::new(pipeline.config.fetch_concurrency));

    // Wrap callback in Arc for sharing across tasks
    let on_progress = on_progress.map(Arc::new);

    for (batch_idx, batch) in assets.chunks(batch_size).enumerate() {
        let mut handles = Vec::with_capacity(batch.len());

        for asset in batch {
            // Skip if already fetched (check for .jpg since IIIF returns JPEG)
            let output_path = pipeline
                .dirs
                .raw
                .join(format!("{}.jpg", asset.encoded_name));
            if output_path.exists() {
                skipped.fetch_add(1, Ordering::Relaxed);

                // Check if we should report progress after skip
                if let Some(ref callback) = on_progress {
                    let current_fetched = fetched.load(Ordering::Relaxed);
                    let current_skipped = skipped.load(Ordering::Relaxed);
                    let current_failed = failed_count.load(Ordering::Relaxed);
                    let processed = current_fetched + current_skipped + current_failed;
                    let last = last_reported.load(Ordering::Relaxed);

                    if processed >= last + PROGRESS_INTERVAL || processed == total {
                        last_reported.store(processed, Ordering::Relaxed);
                        callback(processed, total, current_fetched, current_failed);
                    }
                }
                continue;
            }

            let Some(url) = image_config.iiif_url(policy_id, &asset.encoded_name) else {
                warn!(encoded_name = %asset.encoded_name, "Failed to build IIIF URL");
                failed_count.fetch_add(1, Ordering::Relaxed);
                failed_ids.lock().unwrap().push(asset.encoded_name.clone());
                continue;
            };

            let permit = semaphore.clone().acquire_owned().await.unwrap();
            let client = client.clone();
            let encoded_name = asset.encoded_name.clone();
            let output_path = output_path.clone();
            let fetched = fetched.clone();
            let failed_count = failed_count.clone();
            let failed_ids = failed_ids.clone();
            let skipped = skipped.clone();
            let last_reported = last_reported.clone();
            let on_progress = on_progress.clone();

            handles.push(tokio::spawn(async move {
                let _permit = permit;

                match client.get(&url).send().await {
                    Ok(response) => {
                        if !response.status().is_success() {
                            warn!(
                                encoded_name = %encoded_name,
                                status = %response.status(),
                                "IIIF request failed"
                            );
                            failed_count.fetch_add(1, Ordering::Relaxed);
                            failed_ids.lock().unwrap().push(encoded_name.clone());
                        } else {
                            match response.bytes().await {
                                Ok(bytes) => {
                                    // Validate it's actually a JPEG
                                    if !bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
                                        warn!(
                                            encoded_name = %encoded_name,
                                            "IIIF response is not a JPEG"
                                        );
                                        failed_count.fetch_add(1, Ordering::Relaxed);
                                        failed_ids.lock().unwrap().push(encoded_name.clone());
                                    } else if let Err(e) = std::fs::write(&output_path, &bytes) {
                                        warn!(
                                            path = %output_path.display(),
                                            error = %e,
                                            "Failed to write image"
                                        );
                                        failed_count.fetch_add(1, Ordering::Relaxed);
                                        failed_ids.lock().unwrap().push(encoded_name.clone());
                                    } else {
                                        trace!(
                                            path = %output_path.display(),
                                            bytes = bytes.len(),
                                            "Saved IIIF image"
                                        );
                                        fetched.fetch_add(1, Ordering::Relaxed);
                                    }
                                }
                                Err(e) => {
                                    warn!(
                                        encoded_name = %encoded_name,
                                        error = %e,
                                        "Failed to read IIIF response body"
                                    );
                                    failed_count.fetch_add(1, Ordering::Relaxed);
                                    failed_ids.lock().unwrap().push(encoded_name.clone());
                                }
                            }
                        }
                    }
                    Err(e) => {
                        warn!(
                            encoded_name = %encoded_name,
                            error = %e,
                            "IIIF request error"
                        );
                        failed_count.fetch_add(1, Ordering::Relaxed);
                        failed_ids.lock().unwrap().push(encoded_name.clone());
                    }
                }

                // Check if we should report progress
                if let Some(ref callback) = on_progress {
                    let current_fetched = fetched.load(Ordering::Relaxed);
                    let current_skipped = skipped.load(Ordering::Relaxed);
                    let current_failed = failed_count.load(Ordering::Relaxed);
                    let processed = current_fetched + current_skipped + current_failed;
                    let last = last_reported.load(Ordering::Relaxed);

                    if processed >= last + PROGRESS_INTERVAL || processed == total {
                        // Use compare_exchange to avoid duplicate reports
                        if last_reported
                            .compare_exchange(last, processed, Ordering::SeqCst, Ordering::Relaxed)
                            .is_ok()
                        {
                            callback(processed, total, current_fetched, current_failed);
                        }
                    }
                }
            }));
        }

        // Wait for batch to complete
        for handle in handles {
            let _ = handle.await;
        }

        // Update pipeline state
        let current_fetched = fetched.load(Ordering::Relaxed);
        let current_skipped = skipped.load(Ordering::Relaxed);
        let current_failed = failed_count.load(Ordering::Relaxed);

        pipeline.state.images_fetched = current_fetched + current_skipped;
        pipeline.state.images_failed = current_failed;

        // Save state periodically
        if (batch_idx + 1) % 10 == 0 {
            pipeline.save_state().ok();
        }
    }

    // Final progress report if we haven't reported the final count
    let current_fetched = fetched.load(Ordering::Relaxed);
    let current_skipped = skipped.load(Ordering::Relaxed);
    let current_failed = failed_count.load(Ordering::Relaxed);
    let processed = current_fetched + current_skipped + current_failed;

    if let Some(ref callback) = on_progress {
        let last = last_reported.load(Ordering::Relaxed);
        if processed > last {
            callback(processed, total, current_fetched, current_failed);
        }
    }

    // Final state save
    pipeline.save_state().ok();

    let failed = Arc::try_unwrap(failed_ids).unwrap().into_inner().unwrap();

    debug!(
        fetched = current_fetched,
        skipped = current_skipped,
        failed = failed.len(),
        "IIIF fetch complete"
    );

    Ok(FetchResult {
        fetched: current_fetched,
        skipped: current_skipped,
        failed,
    })
}

/// Fetch thumbnails from Pinata with image optimization.
///
/// - Downloads to the `thumbnails/` directory as PNG
/// - Skips images that already exist
/// - Calls progress callback for CLI output
pub async fn fetch_thumbnails_pinata(
    pipeline: &mut Pipeline,
    assets: &[NormalizedAsset],
    pinata: &crate::pinata::PinataClient,
    thumbnail_size: u32,
    on_progress: Option<ProgressCallback>,
) -> Result<FetchResult, crate::pinata::PinataError> {
    let fetched = Arc::new(AtomicUsize::new(0));
    let skipped = Arc::new(AtomicUsize::new(0));
    let failed_count = Arc::new(AtomicUsize::new(0));
    let failed_ids: Arc<std::sync::Mutex<Vec<String>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    let last_reported = Arc::new(AtomicUsize::new(0));

    let total = assets.len();
    debug!("Starting Pinata thumbnail fetch of {} images", total);

    // Create thumbnails directory if needed
    let thumbnails_dir = pipeline.dirs.root.join("thumbnails");
    std::fs::create_dir_all(&thumbnails_dir).ok();

    // Progress reporting interval
    const PROGRESS_INTERVAL: usize = 50;

    // Process in batches
    let batch_size = 100;
    let semaphore = Arc::new(Semaphore::new(pipeline.config.fetch_concurrency));

    // Wrap callback in Arc for sharing
    let on_progress = on_progress.map(Arc::new);

    for (batch_idx, batch) in assets.chunks(batch_size).enumerate() {
        let mut handles = Vec::with_capacity(batch.len());

        for asset in batch {
            // Skip if already fetched (check for any image extension)
            let exists = ["png", "jpg", "webp", "gif"].iter().any(|ext| {
                thumbnails_dir
                    .join(format!("{}.{}", asset.encoded_name, ext))
                    .exists()
            });
            if exists {
                skipped.fetch_add(1, Ordering::Relaxed);

                // Check if we should report progress after skip
                if let Some(ref callback) = on_progress {
                    let current_fetched = fetched.load(Ordering::Relaxed);
                    let current_skipped = skipped.load(Ordering::Relaxed);
                    let current_failed = failed_count.load(Ordering::Relaxed);
                    let processed = current_fetched + current_skipped + current_failed;
                    let last = last_reported.load(Ordering::Relaxed);

                    if processed >= last + PROGRESS_INTERVAL || processed == total {
                        last_reported.store(processed, Ordering::Relaxed);
                        callback(processed, total, current_fetched, current_failed);
                    }
                }
                continue;
            }

            // Extract CID from image URL
            let Some(ref image_url) = asset.image_url else {
                warn!(encoded_name = %asset.encoded_name, "Asset has no image URL");
                failed_count.fetch_add(1, Ordering::Relaxed);
                failed_ids.lock().unwrap().push(asset.encoded_name.clone());
                continue;
            };

            let Some(cid) = extract_cid(image_url) else {
                warn!(encoded_name = %asset.encoded_name, url = %image_url, "Could not extract CID from URL");
                failed_count.fetch_add(1, Ordering::Relaxed);
                failed_ids.lock().unwrap().push(asset.encoded_name.clone());
                continue;
            };

            let permit = semaphore.clone().acquire_owned().await.unwrap();
            let pinata = pinata.clone();
            let cid = cid.to_string();
            let encoded_name = asset.encoded_name.clone();
            let thumbnails_dir = thumbnails_dir.clone();
            let fetched = fetched.clone();
            let failed_count = failed_count.clone();
            let failed_ids = failed_ids.clone();
            let skipped = skipped.clone();
            let last_reported = last_reported.clone();
            let on_progress = on_progress.clone();

            handles.push(tokio::spawn(async move {
                let _permit = permit;

                match pinata.fetch_thumbnail(&cid, thumbnail_size).await {
                    Ok((bytes, ext)) => {
                        let output_path = thumbnails_dir.join(format!("{}.{}", encoded_name, ext));
                        if let Err(e) = std::fs::write(&output_path, &bytes) {
                            warn!(
                                path = %output_path.display(),
                                error = %e,
                                "Failed to write thumbnail"
                            );
                            failed_count.fetch_add(1, Ordering::Relaxed);
                            failed_ids.lock().unwrap().push(encoded_name.clone());
                        } else {
                            trace!(
                                path = %output_path.display(),
                                bytes = bytes.len(),
                                format = %ext,
                                "Saved Pinata thumbnail"
                            );
                            fetched.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    Err(e) => {
                        warn!(
                            encoded_name = %encoded_name,
                            cid = %cid,
                            error = %e,
                            "Pinata thumbnail fetch failed"
                        );
                        failed_count.fetch_add(1, Ordering::Relaxed);
                        failed_ids.lock().unwrap().push(encoded_name.clone());
                    }
                }

                // Check if we should report progress
                if let Some(ref callback) = on_progress {
                    let current_fetched = fetched.load(Ordering::Relaxed);
                    let current_skipped = skipped.load(Ordering::Relaxed);
                    let current_failed = failed_count.load(Ordering::Relaxed);
                    let processed = current_fetched + current_skipped + current_failed;
                    let last = last_reported.load(Ordering::Relaxed);

                    if processed >= last + PROGRESS_INTERVAL || processed == total {
                        if last_reported
                            .compare_exchange(last, processed, Ordering::SeqCst, Ordering::Relaxed)
                            .is_ok()
                        {
                            callback(processed, total, current_fetched, current_failed);
                        }
                    }
                }
            }));
        }

        // Wait for batch to complete
        for handle in handles {
            let _ = handle.await;
        }

        // Update pipeline state
        let current_fetched = fetched.load(Ordering::Relaxed);
        let current_skipped = skipped.load(Ordering::Relaxed);
        let current_failed = failed_count.load(Ordering::Relaxed);

        pipeline.state.images_fetched = current_fetched + current_skipped;
        pipeline.state.images_failed = current_failed;

        // Save state periodically
        if (batch_idx + 1) % 10 == 0 {
            pipeline.save_state().ok();
        }
    }

    // Final progress report
    let current_fetched = fetched.load(Ordering::Relaxed);
    let current_skipped = skipped.load(Ordering::Relaxed);
    let current_failed = failed_count.load(Ordering::Relaxed);
    let processed = current_fetched + current_skipped + current_failed;

    if let Some(ref callback) = on_progress {
        let last = last_reported.load(Ordering::Relaxed);
        if processed > last {
            callback(processed, total, current_fetched, current_failed);
        }
    }

    // Final state save
    pipeline.save_state().ok();

    let failed = Arc::try_unwrap(failed_ids).unwrap().into_inner().unwrap();

    debug!(
        fetched = current_fetched,
        skipped = current_skipped,
        failed = failed.len(),
        "Pinata thumbnail fetch complete"
    );

    Ok(FetchResult {
        fetched: current_fetched,
        skipped: current_skipped,
        failed,
    })
}

/// Extract CID from an IPFS URL.
fn extract_cid(url: &str) -> Option<&str> {
    let url = url.trim();

    // ipfs://CID or ipfs://CID/path
    if let Some(rest) = url.strip_prefix("ipfs://") {
        let rest = rest.strip_prefix("ipfs/").unwrap_or(rest);
        return Some(rest.split('/').next().unwrap_or(rest));
    }

    // Bare CID
    if url.starts_with("Qm") || url.starts_with("bafy") {
        return Some(url.split('/').next().unwrap_or(url));
    }

    None
}

/// Fetch all images for a collection from NFTCDN.
///
/// - Downloads to the `raw/` directory in original format
/// - Skips images that already exist
/// - Uses CIP-14 asset fingerprints for URL construction
/// - Calls progress callback for CLI output
pub async fn fetch_images_nftcdn(
    pipeline: &mut Pipeline,
    assets: &[NormalizedAsset],
    policy_id: &str,
    nftcdn: &NftcdnClient,
    on_progress: Option<ProgressCallback>,
) -> Result<FetchResult, crate::nftcdn::NftcdnError> {
    let fetched = Arc::new(AtomicUsize::new(0));
    let skipped = Arc::new(AtomicUsize::new(0));
    let failed_count = Arc::new(AtomicUsize::new(0));
    let failed_ids: Arc<std::sync::Mutex<Vec<String>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    let last_reported = Arc::new(AtomicUsize::new(0));

    let total = assets.len();
    debug!("Starting NFTCDN fetch of {} images", total);

    // Progress reporting interval
    const PROGRESS_INTERVAL: usize = 50;

    // Process in batches for state saving
    let batch_size = 100;
    let semaphore = Arc::new(Semaphore::new(pipeline.config.fetch_concurrency));

    // Wrap callback in Arc for sharing across tasks
    let on_progress = on_progress.map(Arc::new);

    for (batch_idx, batch) in assets.chunks(batch_size).enumerate() {
        let mut handles = Vec::with_capacity(batch.len());

        for asset in batch {
            // Skip if already fetched (check for any common extension)
            let exists = ["png", "jpg", "jpeg", "webp", "gif"].iter().any(|ext| {
                pipeline
                    .dirs
                    .raw
                    .join(format!("{}.{}", asset.encoded_name, ext))
                    .exists()
            });
            if exists {
                skipped.fetch_add(1, Ordering::Relaxed);

                // Check if we should report progress after skip
                if let Some(ref callback) = on_progress {
                    let current_fetched = fetched.load(Ordering::Relaxed);
                    let current_skipped = skipped.load(Ordering::Relaxed);
                    let current_failed = failed_count.load(Ordering::Relaxed);
                    let processed = current_fetched + current_skipped + current_failed;
                    let last = last_reported.load(Ordering::Relaxed);

                    if processed >= last + PROGRESS_INTERVAL || processed == total {
                        last_reported.store(processed, Ordering::Relaxed);
                        callback(processed, total, current_fetched, current_failed);
                    }
                }
                continue;
            }

            // Build AssetId from policy_id and encoded asset name (which is hex)
            let asset_id = match AssetId::new(policy_id.to_string(), asset.encoded_name.clone()) {
                Ok(id) => id,
                Err(e) => {
                    warn!(
                        encoded_name = %asset.encoded_name,
                        error = %e,
                        "Failed to create AssetId"
                    );
                    failed_count.fetch_add(1, Ordering::Relaxed);
                    failed_ids.lock().unwrap().push(asset.encoded_name.clone());
                    continue;
                }
            };

            let permit = semaphore.clone().acquire_owned().await.unwrap();
            let nftcdn = nftcdn.clone();
            let encoded_name = asset.encoded_name.clone();
            let raw_dir = pipeline.dirs.raw.clone();
            let fetched = fetched.clone();
            let failed_count = failed_count.clone();
            let failed_ids = failed_ids.clone();
            let skipped = skipped.clone();
            let last_reported = last_reported.clone();
            let on_progress = on_progress.clone();

            handles.push(tokio::spawn(async move {
                let _permit = permit;

                match nftcdn.fetch_image(&asset_id).await {
                    Ok(bytes) => {
                        // Detect format from magic bytes
                        let ext = detect_image_format(&bytes).unwrap_or("bin");
                        let output_path = raw_dir.join(format!("{}.{}", encoded_name, ext));

                        if let Err(e) = std::fs::write(&output_path, &bytes) {
                            warn!(
                                path = %output_path.display(),
                                error = %e,
                                "Failed to write image"
                            );
                            failed_count.fetch_add(1, Ordering::Relaxed);
                            failed_ids.lock().unwrap().push(encoded_name.clone());
                        } else {
                            trace!(
                                path = %output_path.display(),
                                bytes = bytes.len(),
                                format = %ext,
                                "Saved NFTCDN image"
                            );
                            fetched.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    Err(e) => {
                        warn!(
                            encoded_name = %encoded_name,
                            error = %e,
                            "NFTCDN fetch failed"
                        );
                        failed_count.fetch_add(1, Ordering::Relaxed);
                        failed_ids.lock().unwrap().push(encoded_name.clone());
                    }
                }

                // Check if we should report progress
                if let Some(ref callback) = on_progress {
                    let current_fetched = fetched.load(Ordering::Relaxed);
                    let current_skipped = skipped.load(Ordering::Relaxed);
                    let current_failed = failed_count.load(Ordering::Relaxed);
                    let processed = current_fetched + current_skipped + current_failed;
                    let last = last_reported.load(Ordering::Relaxed);

                    if processed >= last + PROGRESS_INTERVAL || processed == total {
                        // Use compare_exchange to avoid duplicate reports
                        if last_reported
                            .compare_exchange(last, processed, Ordering::SeqCst, Ordering::Relaxed)
                            .is_ok()
                        {
                            callback(processed, total, current_fetched, current_failed);
                        }
                    }
                }
            }));
        }

        // Wait for batch to complete
        for handle in handles {
            let _ = handle.await;
        }

        // Update pipeline state
        let current_fetched = fetched.load(Ordering::Relaxed);
        let current_skipped = skipped.load(Ordering::Relaxed);
        let current_failed = failed_count.load(Ordering::Relaxed);

        pipeline.state.images_fetched = current_fetched + current_skipped;
        pipeline.state.images_failed = current_failed;

        // Save state periodically
        if (batch_idx + 1) % 10 == 0 {
            pipeline.save_state().ok();
        }
    }

    // Final progress report if we haven't reported the final count
    let current_fetched = fetched.load(Ordering::Relaxed);
    let current_skipped = skipped.load(Ordering::Relaxed);
    let current_failed = failed_count.load(Ordering::Relaxed);
    let processed = current_fetched + current_skipped + current_failed;

    if let Some(ref callback) = on_progress {
        let last = last_reported.load(Ordering::Relaxed);
        if processed > last {
            callback(processed, total, current_fetched, current_failed);
        }
    }

    // Final state save
    pipeline.save_state().ok();

    let failed = Arc::try_unwrap(failed_ids).unwrap().into_inner().unwrap();

    debug!(
        fetched = current_fetched,
        skipped = current_skipped,
        failed = failed.len(),
        "NFTCDN fetch complete"
    );

    Ok(FetchResult {
        fetched: current_fetched,
        skipped: current_skipped,
        failed,
    })
}

/// Detect image format from magic bytes.
fn detect_image_format(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() < 4 {
        return None;
    }

    // PNG: 89 50 4E 47
    if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        return Some("png");
    }

    // JPEG: FF D8 FF
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("jpg");
    }

    // WebP: RIFF....WEBP
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("webp");
    }

    // GIF: GIF87a or GIF89a
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("gif");
    }

    None
}
