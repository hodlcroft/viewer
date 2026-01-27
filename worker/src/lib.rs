mod bundle_reader;

use viewer_format::AssetDetails;
use serde::{Deserialize, Serialize};
use worker::*;

use crate::bundle_reader::BundleReader;

/// Preview configuration stored in R2 alongside the generation
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PreviewConfig {
    /// Access token required to view this project's preview
    token: String,
}

#[event(start)]
fn start() {
    console_error_panic_hook::set_once();
}

#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let router = Router::new();

    router
        // Health check
        .get("/health", |_req, _ctx| {
            Response::ok("Preview viewer healthy")
        })
        // API endpoints under /api/ prefix to avoid conflicting with SPA routes
        // Asset details endpoint: GET /api/:project/:seed/asset_details.json
        .get_async("/api/:project/:seed/asset_details.json", get_asset_details)
        // Bundle index endpoint: GET /api/:project/:seed/index.json
        .get_async("/api/:project/:seed/index.json", get_index)
        // Image endpoint: GET /api/:project/:seed/images/:id
        .get_async("/api/:project/:seed/images/:id", get_image)
        // Sprite sheet endpoint: GET /api/:project/:seed/sprites/:sheet
        .get_async("/api/:project/:seed/sprites/:sheet", get_sprite_sheet)
        // Token details endpoint: GET /api/:project/:seed/token/:id
        .get_async("/api/:project/:seed/token/:id", get_token_details)
        // All other routes fall through to static assets (SPA)
        .run(req, env)
        .await
}

/// Extract access token from request (query param or headers)
fn extract_request_token(req: &Request) -> Result<Option<String>> {
    // Check query parameter first
    let url = req.url()?;
    let query_token = url
        .query_pairs()
        .find(|(key, _)| key == "token")
        .map(|(_, value)| value.to_string());

    if query_token.is_some() {
        return Ok(query_token);
    }

    // Check Authorization header
    if let Ok(Some(header_value)) = req.headers().get("Authorization") {
        // Support "Bearer <token>" format
        let token = header_value
            .strip_prefix("Bearer ")
            .unwrap_or(&header_value);
        return Ok(Some(token.to_string()));
    }

    // Check X-Access-Token header
    if let Ok(Some(token)) = req.headers().get("X-Access-Token") {
        return Ok(Some(token));
    }

    Ok(None)
}

/// Load preview config from R2 for a specific project/seed
async fn load_preview_config(
    bucket: &Bucket,
    project: &str,
    seed: &str,
) -> Result<Option<PreviewConfig>> {
    let key = format!("generations/{project}/{seed}/preview.json");

    let object = bucket.get(&key).execute().await?;

    match object {
        Some(obj) => {
            let body = obj
                .body()
                .ok_or_else(|| Error::RustError("Preview config body not available".to_string()))?;

            let bytes = body.bytes().await?;
            let config: PreviewConfig = serde_json::from_slice(&bytes)
                .map_err(|e| Error::RustError(format!("Failed to parse preview config: {e}")))?;

            Ok(Some(config))
        }
        None => Ok(None),
    }
}

/// Verify access token for a specific project/seed
/// Returns Ok(()) if valid, Err with appropriate error if invalid
async fn verify_project_access(
    req: &Request,
    bucket: &Bucket,
    project: &str,
    seed: &str,
) -> Result<()> {
    // Load preview config from R2
    let preview_config = load_preview_config(bucket, project, seed).await?;

    match preview_config {
        Some(config) => {
            // Project has preview enabled, verify token
            let request_token = extract_request_token(req)?;

            match request_token {
                Some(token) if token == config.token => Ok(()),
                Some(_) => Err(Error::RustError(
                    "Unauthorized: Invalid access token".to_string(),
                )),
                None => Err(Error::RustError(
                    "Unauthorized: Access token required".to_string(),
                )),
            }
        }
        None => {
            // No preview.json means preview is not enabled for this project
            Err(Error::RustError(
                "Preview not enabled for this project".to_string(),
            ))
        }
    }
}

/// GET /:project/:seed/index.json
/// Fetch and return the index.json file from R2
async fn get_index(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let project = ctx
        .param("project")
        .ok_or_else(|| Error::RustError("Missing project parameter".to_string()))?;
    let seed = ctx
        .param("seed")
        .ok_or_else(|| Error::RustError("Missing seed parameter".to_string()))?;

    // Get R2 bucket
    let bucket = ctx.bucket("COMPOSITOR_BUCKET")?;

    // Verify access token for this project
    if let Err(e) = verify_project_access(&req, &bucket, project, seed).await {
        return Response::error(e.to_string(), 401);
    }

    // Construct R2 path
    let key = format!("generations/{project}/{seed}/index.json");

    // Fetch from R2
    let object = bucket.get(&key).execute().await?;

    match object {
        Some(obj) => {
            let body = obj
                .body()
                .ok_or_else(|| Error::RustError("Object body not available".to_string()))?;

            let bytes = body.bytes().await?;

            // Return with CORS headers
            let mut response = Response::from_bytes(bytes)?;
            let headers = response.headers_mut();
            headers.set("Content-Type", "application/json")?;
            headers.set("Access-Control-Allow-Origin", "*")?;
            headers.set("Cache-Control", "public, max-age=3600")?;
            Ok(response)
        }
        None => Response::error("Index not found", 404),
    }
}

/// GET /:project/:seed/asset_details.json
/// Fetch and return the asset_details.json file from R2
async fn get_asset_details(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let project = ctx
        .param("project")
        .ok_or_else(|| Error::RustError("Missing project parameter".to_string()))?;
    let seed = ctx
        .param("seed")
        .ok_or_else(|| Error::RustError("Missing seed parameter".to_string()))?;

    // Get R2 bucket
    let bucket = ctx.bucket("COMPOSITOR_BUCKET")?;

    // Verify access token for this project
    if let Err(e) = verify_project_access(&req, &bucket, project, seed).await {
        return Response::error(e.to_string(), 401);
    }

    // Construct R2 path
    let key = format!("generations/{project}/{seed}/asset_details.json");

    // Fetch from R2
    let object = bucket.get(&key).execute().await?;

    match object {
        Some(obj) => {
            let body = obj
                .body()
                .ok_or_else(|| Error::RustError("Object body not available".to_string()))?;

            let bytes = body.bytes().await?;

            // Return with CORS headers
            let mut response = Response::from_bytes(bytes)?;
            let headers = response.headers_mut();
            headers.set("Content-Type", "application/json")?;
            headers.set("Access-Control-Allow-Origin", "*")?;
            headers.set("Cache-Control", "public, max-age=3600")?;
            Ok(response)
        }
        None => Response::error("Collection not found", 404),
    }
}

/// GET /:project/:seed/images/:id
/// Extract and return a specific image from the bundle
async fn get_image(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let project = ctx
        .param("project")
        .ok_or_else(|| Error::RustError("Missing project parameter".to_string()))?;
    let seed = ctx
        .param("seed")
        .ok_or_else(|| Error::RustError("Missing seed parameter".to_string()))?;
    let id = ctx
        .param("id")
        .ok_or_else(|| Error::RustError("Missing id parameter".to_string()))?;

    // Get R2 bucket
    let bucket = ctx.bucket("COMPOSITOR_BUCKET")?;

    // Verify access token for this project
    if let Err(e) = verify_project_access(&req, &bucket, project, seed).await {
        return Response::error(e.to_string(), 401);
    }

    // Get KV store for caching (optional)
    let kv = ctx.kv("CACHE").ok();

    // Create bundle reader
    let bundle_reader = BundleReader::new(&bucket, project, seed);

    // Get image from bundle
    match bundle_reader.get_image(id, kv.as_ref()).await {
        Ok((image_bytes, image_format)) => {
            // Determine content type from format
            let content_type = match image_format.as_str() {
                "png" => "image/png",
                "webp" => "image/webp",
                "jpg" | "jpeg" => "image/jpeg",
                _ => "application/octet-stream",
            };

            // Return image with caching headers
            let mut response = Response::from_bytes(image_bytes)?;
            let headers = response.headers_mut();
            headers.set("Content-Type", content_type)?;
            headers.set("Access-Control-Allow-Origin", "*")?;
            headers.set("Cache-Control", "public, max-age=31536000, immutable")?;
            Ok(response)
        }
        Err(e) => Response::error(format!("Image not found: {e}"), 404),
    }
}

/// GET /:project/:seed/sprites/:sheet
/// Fetch and return a sprite sheet from R2
async fn get_sprite_sheet(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let project = ctx
        .param("project")
        .ok_or_else(|| Error::RustError("Missing project parameter".to_string()))?;
    let seed = ctx
        .param("seed")
        .ok_or_else(|| Error::RustError("Missing seed parameter".to_string()))?;
    let sheet = ctx
        .param("sheet")
        .ok_or_else(|| Error::RustError("Missing sheet parameter".to_string()))?;

    // Get R2 bucket
    let bucket = ctx.bucket("COMPOSITOR_BUCKET")?;

    // Verify access token for this project
    if let Err(e) = verify_project_access(&req, &bucket, project, seed).await {
        return Response::error(e.to_string(), 401);
    }

    // Construct R2 path - sheet param is just the number, we add the prefix/suffix
    let key = format!("generations/{project}/{seed}/sprites_{sheet}.webp");

    // Fetch from R2
    let object = bucket.get(&key).execute().await?;

    match object {
        Some(obj) => {
            let body = obj
                .body()
                .ok_or_else(|| Error::RustError("Object body not available".to_string()))?;

            let bytes = body.bytes().await?;

            // Return with caching headers (sprites are immutable)
            let mut response = Response::from_bytes(bytes)?;
            let headers = response.headers_mut();
            headers.set("Content-Type", "image/webp")?;
            headers.set("Access-Control-Allow-Origin", "*")?;
            headers.set("Cache-Control", "public, max-age=31536000, immutable")?;
            Ok(response)
        }
        None => Response::error("Sprite sheet not found", 404),
    }
}

/// GET /:project/:seed/token/:id
/// Get details for a specific token
async fn get_token_details(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let project = ctx
        .param("project")
        .ok_or_else(|| Error::RustError("Missing project parameter".to_string()))?;
    let seed = ctx
        .param("seed")
        .ok_or_else(|| Error::RustError("Missing seed parameter".to_string()))?;
    let id = ctx
        .param("id")
        .ok_or_else(|| Error::RustError("Missing id parameter".to_string()))?;

    // Get R2 bucket
    let bucket = ctx.bucket("COMPOSITOR_BUCKET")?;

    // Verify access token for this project
    if let Err(e) = verify_project_access(&req, &bucket, project, seed).await {
        return Response::error(e.to_string(), 401);
    }

    // Fetch asset_details.json
    let key = format!("generations/{project}/{seed}/asset_details.json");
    let object = bucket.get(&key).execute().await?;

    match object {
        Some(obj) => {
            let body = obj
                .body()
                .ok_or_else(|| Error::RustError("Object body not available".to_string()))?;

            let bytes = body.bytes().await?;
            let details: AssetDetails = serde_json::from_slice(&bytes)
                .map_err(|e| Error::RustError(format!("Failed to parse asset details: {e}")))?;

            // Find the specific token
            let token = details
                .tokens
                .iter()
                .find(|t| t.id == *id)
                .ok_or_else(|| Error::RustError(format!("Token {id} not found")))?;

            let response_json = serde_json::to_string(token)
                .map_err(|e| Error::RustError(format!("Failed to serialize token: {e}")))?;

            let mut response = Response::from_bytes(response_json.into_bytes())?;
            let headers = response.headers_mut();
            headers.set("Content-Type", "application/json")?;
            headers.set("Access-Control-Allow-Origin", "*")?;
            headers.set("Cache-Control", "public, max-age=3600")?;
            Ok(response)
        }
        None => Response::error("Collection not found", 404),
    }
}
