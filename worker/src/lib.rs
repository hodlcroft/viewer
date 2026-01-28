//! NFT Collection Viewer Worker
//!
//! This worker serves the frontend SPA and provides API endpoints.
//! Collection data is fetched directly from R2 by the frontend via files.hodlcroft.com.

use serde::Serialize;
use viewer_binary::{HEADER_SIZE, Header, MAGIC, TraitSchema};
use worker::Fetch;
use worker_stack::prelude::*;

const FILES_BASE_URL: &str = "https://files.hodlcroft.com/collections";

#[event(start)]
fn start() {
    worker_utils::set_panic_hook();
    worker_utils::init_tracing(None);
}

#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let router = Router::new();

    router
        // API routes
        .get("/api/health", |_req, _ctx| Response::ok("Viewer healthy"))
        .get_async("/api/debug/:slug", handle_debug)
        // All other routes fall through to static assets (SPA)
        .run(req, env)
        .await
}

/// Debug endpoint - parses collection.bin and returns JSON
async fn handle_debug(_req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let slug = ctx.param("slug").map(|s| s.as_str()).unwrap_or("unknown");

    // Fetch collection.bin from R2
    let url = format!("{}/{}/collection.bin", FILES_BASE_URL, slug);
    let fetch_req = Request::new(&url, worker::Method::Get)?;
    let mut resp = Fetch::Request(fetch_req).send().await?;

    if resp.status_code() != 200 {
        return Response::error(
            format!("Failed to fetch collection.bin: {}", resp.status_code()),
            404,
        );
    }

    let data = resp.bytes().await?.to_vec();

    // Parse and return debug info
    match parse_debug_info(&data) {
        Ok(info) => Response::from_json(&info),
        Err(e) => Response::error(format!("Parse error: {}", e), 500),
    }
}

#[derive(Serialize)]
struct DebugInfo {
    file_size: usize,
    header: HeaderInfo,
    string_table: StringTableInfo,
    traits: Vec<TraitInfo>,
    tokens: Vec<TokenDebugInfo>,
}

#[derive(Serialize)]
struct HeaderInfo {
    magic: String,
    version: u16,
    token_count: u32,
    trait_count: u8,
    bitmap_size: u8,
    string_table_offset: u32,
    trait_schema_offset: u32,
    token_table_offset: u32,
    asset_id_index_offset: u32,
}

#[derive(Serialize)]
struct StringTableInfo {
    length_prefix: u32,
    data_start: usize,
    first_50_bytes_hex: String,
}

#[derive(Serialize)]
struct TraitInfo {
    name: String,
    name_ref: u16,
    value_count: usize,
    values: Vec<String>,
}

#[derive(Serialize)]
struct TokenDebugInfo {
    index: u32,
    sprite_sheet: u16,
    sprite_x: u8,
    sprite_y: u8,
    rarity_rank: u16,
    name_ref_raw: u16,
    name_ref_masked: u16,
    has_custom_name: bool,
    name: String,
    asset_id: String,
    bitmap_hex: String,
}

fn parse_debug_info(data: &[u8]) -> std::result::Result<DebugInfo, String> {
    if data.len() < HEADER_SIZE {
        return Err(format!("File too small: {} bytes", data.len()));
    }

    // Parse header
    let header_bytes: [u8; HEADER_SIZE] = data[..HEADER_SIZE]
        .try_into()
        .map_err(|_| "Failed to read header")?;
    let header = Header::from_bytes(&header_bytes);

    if header.magic != MAGIC {
        return Err(format!("Invalid magic: {:?}", header.magic));
    }

    // String table info
    let st_offset = header.string_table_offset as usize;
    let st_length = u32::from_le_bytes([
        data[st_offset],
        data[st_offset + 1],
        data[st_offset + 2],
        data[st_offset + 3],
    ]);
    let st_data_start = st_offset + 4;
    let st_first_50: String = data
        [st_data_start..st_data_start + 50.min(data.len() - st_data_start)]
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect();

    // Parse traits
    let trait_schema_start = header.trait_schema_offset as usize;
    let trait_schema_end = header.trait_index_offset as usize;
    let trait_schema = TraitSchema::from_bytes(&data[trait_schema_start..trait_schema_end])
        .ok_or("Failed to parse trait schema")?;

    let string_table_data = &data[st_data_start..header.trait_schema_offset as usize];

    let traits: Vec<TraitInfo> = trait_schema
        .traits
        .iter()
        .map(|t| TraitInfo {
            name: read_string(string_table_data, t.name.0),
            name_ref: t.name.0,
            value_count: t.values.len(),
            values: t
                .values
                .iter()
                .map(|v| read_string(string_table_data, v.name.0))
                .collect(),
        })
        .collect();

    // Parse first 10 tokens
    let bitmap_size = header.bitmap_size().ok_or("Invalid bitmap size")?;
    let bitmap_bytes = bitmap_size.byte_size();
    let token_fixed_size = 10;
    let token_entry_size = token_fixed_size + bitmap_bytes;
    let token_table_start = header.token_table_offset as usize;
    let asset_id_index_start = header.asset_id_index_offset as usize;

    let mut tokens = Vec::new();
    for i in 0..10.min(header.token_count) {
        let offset = token_table_start + (i as usize) * token_entry_size;
        let entry = &data[offset..offset + token_entry_size];

        let sprite_sheet = u16::from_le_bytes([entry[0], entry[1]]);
        let sprite_x = entry[2];
        let sprite_y = entry[3];
        let rarity_rank = u16::from_le_bytes([entry[4], entry[5]]);
        let name_ref = u16::from_le_bytes([entry[8], entry[9]]);
        let bitmap = &entry[10..10 + bitmap_bytes];

        // Read asset ID from new format
        let aid_offset_entry = asset_id_index_start + (i as usize) * 4;
        let aid_str_offset = u32::from_le_bytes([
            data[aid_offset_entry],
            data[aid_offset_entry + 1],
            data[aid_offset_entry + 2],
            data[aid_offset_entry + 3],
        ]) as usize;
        let asset_id = read_string_at(&data[asset_id_index_start..], aid_str_offset);

        let has_custom_name = name_ref & 0x8000 != 0;
        let name_ref_masked = name_ref & 0x7FFF;
        let name = if has_custom_name {
            read_string(string_table_data, name_ref_masked)
        } else {
            format!("#{}", name_ref)
        };

        tokens.push(TokenDebugInfo {
            index: i,
            sprite_sheet,
            sprite_x,
            sprite_y,
            rarity_rank,
            name_ref_raw: name_ref,
            name_ref_masked,
            has_custom_name,
            name,
            asset_id,
            bitmap_hex: bitmap.iter().map(|b| format!("{:02x}", b)).collect(),
        });
    }

    Ok(DebugInfo {
        file_size: data.len(),
        header: HeaderInfo {
            magic: String::from_utf8_lossy(&header.magic).to_string(),
            version: header.version,
            token_count: header.token_count,
            trait_count: header.trait_count,
            bitmap_size: header.bitmap_size,
            string_table_offset: header.string_table_offset,
            trait_schema_offset: header.trait_schema_offset,
            token_table_offset: header.token_table_offset,
            asset_id_index_offset: header.asset_id_index_offset,
        },
        string_table: StringTableInfo {
            length_prefix: st_length,
            data_start: st_data_start,
            first_50_bytes_hex: st_first_50,
        },
        traits,
        tokens,
    })
}

fn read_string(table: &[u8], offset: u16) -> String {
    read_string_at(table, offset as usize)
}

fn read_string_at(data: &[u8], offset: usize) -> String {
    if offset >= data.len() {
        return format!("<out of bounds: {} >= {}>", offset, data.len());
    }
    let end = data[offset..]
        .iter()
        .position(|&b| b == 0)
        .map(|p| offset + p)
        .unwrap_or(data.len());
    String::from_utf8_lossy(&data[offset..end]).to_string()
}
