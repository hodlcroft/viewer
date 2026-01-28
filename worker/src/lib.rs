//! NFT Collection Viewer Worker
//!
//! This worker serves the frontend SPA and provides a health check endpoint.
//! Collection data is fetched directly from R2 by the frontend via files.hodlcroft.com.

use worker_stack::prelude::*;

#[event(start)]
fn start() {
    worker_utils::set_panic_hook();
    worker_utils::init_tracing(None);
}

#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let router = Router::new();

    router
        // Health check
        .get("/health", |_req, _ctx| Response::ok("Viewer healthy"))
        // All other routes fall through to static assets (SPA)
        .run(req, env)
        .await
}
