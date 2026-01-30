//! IPFS CID computation for local files.
//!
//! Computes CIDs using the same algorithm as IPFS to enable
//! checking if files already exist on IPFS before uploading.
//!
//! Supports both single-block files (raw) and multi-block files (UnixFS DAG).

use cid::Cid;
use multihash_codetable::{Code, MultihashDigest};
use std::path::Path;

/// IPFS codec identifiers
const RAW_CODEC: u64 = 0x55;
const DAG_PB_CODEC: u64 = 0x70;

/// Default IPFS chunk size (256 KiB)
const DEFAULT_CHUNK_SIZE: usize = 262144;

/// Result of CID computation with metadata about how it was computed
#[derive(Debug, Clone)]
pub struct ComputedCid {
    /// The computed CID (v1 format)
    pub cid_v1: String,
    /// The computed CID (v0 format, if applicable)
    pub cid_v0: Option<String>,
    /// Whether this is a single-block (raw) or multi-block (dag-pb) CID
    pub is_raw: bool,
    /// File size in bytes
    pub file_size: u64,
    /// Number of chunks (1 for raw/single-block)
    pub chunk_count: usize,
}

/// CID computation settings to match different IPFS configurations
#[derive(Debug, Clone)]
pub struct CidSettings {
    /// Chunk size for splitting large files
    pub chunk_size: usize,
    /// Whether to use raw leaves (CIDv1 raw codec) for leaf nodes
    pub raw_leaves: bool,
    /// Whether to wrap small files in UnixFS (false = raw codec for small files)
    pub wrap_small_files: bool,
}

impl Default for CidSettings {
    fn default() -> Self {
        Self {
            chunk_size: DEFAULT_CHUNK_SIZE,
            raw_leaves: true,
            wrap_small_files: false,
        }
    }
}

impl CidSettings {
    /// Settings that match `ipfs add --raw-leaves` (modern default)
    pub fn raw_leaves() -> Self {
        Self {
            chunk_size: DEFAULT_CHUNK_SIZE,
            raw_leaves: true,
            wrap_small_files: false,
        }
    }

    /// Settings that match old IPFS behavior (wrapped in dag-pb)
    pub fn legacy() -> Self {
        Self {
            chunk_size: DEFAULT_CHUNK_SIZE,
            raw_leaves: false,
            wrap_small_files: true,
        }
    }
}

/// Compute CID for a file using default settings (raw leaves, 256KB chunks)
pub fn compute_cid(path: &Path) -> std::io::Result<ComputedCid> {
    compute_cid_with_settings(path, &CidSettings::default())
}

/// Compute CID for raw bytes using default settings
pub fn compute_cid_bytes(data: &[u8]) -> ComputedCid {
    compute_cid_bytes_with_settings(data, &CidSettings::default())
}

/// Compute CID for a file with custom settings
pub fn compute_cid_with_settings(
    path: &Path,
    settings: &CidSettings,
) -> std::io::Result<ComputedCid> {
    let data = std::fs::read(path)?;
    Ok(compute_cid_bytes_with_settings(&data, settings))
}

/// Compute CID for raw bytes with custom settings
pub fn compute_cid_bytes_with_settings(data: &[u8], settings: &CidSettings) -> ComputedCid {
    let file_size = data.len() as u64;

    // Single chunk file - use raw codec (unless wrap_small_files is set)
    if data.len() <= settings.chunk_size && !settings.wrap_small_files {
        let digest = Code::Sha2_256.digest(data);
        let cid_v1 = Cid::new_v1(RAW_CODEC, digest.clone());

        ComputedCid {
            cid_v1: cid_v1.to_string(),
            cid_v0: None, // Raw codec can't be converted to v0
            is_raw: true,
            file_size,
            chunk_count: 1,
        }
    } else if data.len() <= settings.chunk_size && settings.wrap_small_files {
        // Wrap in UnixFS protobuf (legacy behavior)
        let wrapped = wrap_in_unixfs(data);
        let digest = Code::Sha2_256.digest(&wrapped);
        let cid_v1 = Cid::new_v1(DAG_PB_CODEC, digest.clone());
        let cid_v0 = Cid::new_v0(digest).ok();

        ComputedCid {
            cid_v1: cid_v1.to_string(),
            cid_v0: cid_v0.map(|c| c.to_string()),
            is_raw: false,
            file_size,
            chunk_count: 1,
        }
    } else {
        // Multi-chunk file - build UnixFS DAG
        compute_dag_cid(data, settings)
    }
}

/// Wrap data in a UnixFS Data protobuf message
fn wrap_in_unixfs(data: &[u8]) -> Vec<u8> {
    // UnixFS Data message:
    // message Data {
    //   enum DataType { Raw = 0; Directory = 1; File = 2; ... }
    //   required DataType Type = 1;
    //   optional bytes Data = 2;
    //   optional uint64 filesize = 3;
    //   repeated uint64 blocksizes = 4;
    // }
    //
    // For a simple file:
    // - Type = 2 (File)
    // - Data = file contents
    // - filesize = length

    use prost::Message;

    #[derive(Clone, PartialEq, Message)]
    struct UnixFsData {
        #[prost(int32, tag = "1")]
        r#type: i32,
        #[prost(bytes, optional, tag = "2")]
        data: Option<Vec<u8>>,
        #[prost(uint64, optional, tag = "3")]
        filesize: Option<u64>,
        #[prost(uint64, repeated, tag = "4")]
        blocksizes: Vec<u64>,
    }

    // DataType::File = 2
    const DATA_TYPE_FILE: i32 = 2;

    let unixfs = UnixFsData {
        r#type: DATA_TYPE_FILE,
        data: Some(data.to_vec()),
        filesize: Some(data.len() as u64),
        blocksizes: vec![],
    };

    // Now wrap in PBNode
    // message PBNode {
    //   repeated PBLink Links = 2;
    //   optional bytes Data = 1;
    // }
    #[derive(Clone, PartialEq, Message)]
    struct PBNode {
        #[prost(bytes, optional, tag = "1")]
        data: Option<Vec<u8>>,
        // Links would be tag 2, but we have none for leaf nodes
    }

    let mut unixfs_bytes = Vec::new();
    unixfs.encode(&mut unixfs_bytes).unwrap();

    let node = PBNode {
        data: Some(unixfs_bytes),
    };

    let mut node_bytes = Vec::new();
    node.encode(&mut node_bytes).unwrap();
    node_bytes
}

/// Compute CID for a multi-chunk file using balanced DAG
fn compute_dag_cid(data: &[u8], settings: &CidSettings) -> ComputedCid {
    use prost::Message;

    // Split into chunks
    let chunks: Vec<&[u8]> = data.chunks(settings.chunk_size).collect();
    let chunk_count = chunks.len();

    // Compute CID for each chunk (leaf nodes)
    let leaf_cids: Vec<(Cid, u64)> = chunks
        .iter()
        .map(|chunk| {
            let size = chunk.len() as u64;
            if settings.raw_leaves {
                // Raw leaf - just hash the data directly
                let digest = Code::Sha2_256.digest(chunk);
                (Cid::new_v1(RAW_CODEC, digest), size)
            } else {
                // Wrapped leaf - wrap in UnixFS
                let wrapped = wrap_in_unixfs(chunk);
                let digest = Code::Sha2_256.digest(&wrapped);
                (Cid::new_v1(DAG_PB_CODEC, digest), size)
            }
        })
        .collect();

    // Build root node with links to all leaves
    // For simplicity, we build a single-level DAG (all chunks as direct children)
    // Real IPFS uses a balanced tree for very large files, but this works for typical NFT images

    #[derive(Clone, PartialEq, Message)]
    struct PBLink {
        #[prost(bytes, optional, tag = "1")]
        hash: Option<Vec<u8>>,
        #[prost(string, optional, tag = "2")]
        name: Option<String>,
        #[prost(uint64, optional, tag = "3")]
        tsize: Option<u64>,
    }

    #[derive(Clone, PartialEq, Message)]
    struct PBNode {
        #[prost(message, repeated, tag = "2")]
        links: Vec<PBLink>,
        #[prost(bytes, optional, tag = "1")]
        data: Option<Vec<u8>>,
    }

    #[derive(Clone, PartialEq, Message)]
    struct UnixFsData {
        #[prost(int32, tag = "1")]
        r#type: i32,
        #[prost(bytes, optional, tag = "2")]
        data: Option<Vec<u8>>,
        #[prost(uint64, optional, tag = "3")]
        filesize: Option<u64>,
        #[prost(uint64, repeated, tag = "4")]
        blocksizes: Vec<u64>,
    }

    // DataType::File = 2
    const DATA_TYPE_FILE: i32 = 2;

    // Create UnixFS data for root (no data, just metadata)
    let blocksizes: Vec<u64> = leaf_cids.iter().map(|(_, size)| *size).collect();
    let unixfs = UnixFsData {
        r#type: DATA_TYPE_FILE,
        data: None,
        filesize: Some(data.len() as u64),
        blocksizes,
    };

    let mut unixfs_bytes = Vec::new();
    unixfs.encode(&mut unixfs_bytes).unwrap();

    // Create links to children
    let links: Vec<PBLink> = leaf_cids
        .iter()
        .map(|(cid, size)| PBLink {
            hash: Some(cid.to_bytes()),
            name: Some(String::new()),
            tsize: Some(*size),
        })
        .collect();

    let root_node = PBNode {
        links,
        data: Some(unixfs_bytes),
    };

    let mut root_bytes = Vec::new();
    root_node.encode(&mut root_bytes).unwrap();

    let digest = Code::Sha2_256.digest(&root_bytes);
    let cid_v1 = Cid::new_v1(DAG_PB_CODEC, digest.clone());
    let cid_v0 = Cid::new_v0(digest).ok();

    ComputedCid {
        cid_v1: cid_v1.to_string(),
        cid_v0: cid_v0.map(|c| c.to_string()),
        is_raw: false,
        file_size: data.len() as u64,
        chunk_count,
    }
}

/// Information about an existing CID parsed from a string
#[derive(Debug, Clone)]
pub struct CidInfo {
    /// Original CID string
    pub original: String,
    /// CID version (0 or 1)
    pub version: u8,
    /// Codec name
    pub codec: String,
    /// Codec code
    pub codec_code: u64,
    /// Hash algorithm name
    pub hash_algo: String,
    /// Hash digest (hex)
    pub digest_hex: String,
}

/// Parse a CID string and extract information about its format
pub fn parse_cid_info(cid_str: &str) -> Option<CidInfo> {
    let cid = Cid::try_from(cid_str).ok()?;

    let version = match cid.version() {
        cid::Version::V0 => 0,
        cid::Version::V1 => 1,
    };

    let codec_code = cid.codec();
    let codec = match codec_code {
        RAW_CODEC => "raw".to_string(),
        DAG_PB_CODEC => "dag-pb".to_string(),
        0x71 => "dag-cbor".to_string(),
        _ => format!("0x{:x}", codec_code),
    };

    let hash = cid.hash();
    let hash_algo = match hash.code() {
        0x12 => "sha2-256".to_string(),
        0x1b => "keccak-256".to_string(),
        0xb220 => "blake2b-256".to_string(),
        code => format!("0x{:x}", code),
    };

    let digest_hex = hex::encode(hash.digest());

    Some(CidInfo {
        original: cid_str.to_string(),
        version,
        codec,
        codec_code,
        hash_algo,
        digest_hex,
    })
}

/// Try to compute a CID that matches an existing one by trying different settings
pub fn find_matching_cid(data: &[u8], target_cid: &str) -> Option<(ComputedCid, &'static str)> {
    // Parse target to understand what we're matching against
    let target = Cid::try_from(target_cid).ok()?;
    let target_v1 = if target.version() == cid::Version::V0 {
        Cid::new_v1(target.codec(), target.hash().to_owned()).to_string()
    } else {
        target.to_string()
    };

    // Try different settings
    let strategies: Vec<(&'static str, CidSettings)> = vec![
        ("raw-leaves (default)", CidSettings::raw_leaves()),
        ("legacy (wrapped)", CidSettings::legacy()),
        (
            "raw-leaves 1MB chunks",
            CidSettings {
                chunk_size: 1024 * 1024,
                raw_leaves: true,
                wrap_small_files: false,
            },
        ),
    ];

    for (name, settings) in strategies {
        let computed = compute_cid_bytes_with_settings(data, &settings);

        // Check v1 match
        if computed.cid_v1 == target_v1 {
            return Some((computed, name));
        }

        // Check v0 match (if target was v0)
        if target.version() == cid::Version::V0 {
            if let Some(ref v0) = computed.cid_v0 {
                if v0 == target_cid {
                    return Some((computed, name));
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_small_file_raw_cid() {
        // Small file should use raw codec
        let data = b"Hello, World!";
        let result = compute_cid_bytes(data);

        assert!(result.is_raw);
        assert_eq!(result.chunk_count, 1);
        assert!(result.cid_v1.starts_with("bafk")); // raw codec prefix
        assert!(result.cid_v0.is_none()); // raw can't be v0
    }

    #[test]
    fn test_small_file_wrapped_cid() {
        // With wrap_small_files, should use dag-pb
        let data = b"Hello, World!";
        let settings = CidSettings::legacy();
        let result = compute_cid_bytes_with_settings(data, &settings);

        assert!(!result.is_raw);
        assert_eq!(result.chunk_count, 1);
        assert!(result.cid_v1.starts_with("bafy")); // dag-pb prefix
        assert!(result.cid_v0.is_some()); // dag-pb can be v0
        assert!(result.cid_v0.as_ref().unwrap().starts_with("Qm"));
    }

    #[test]
    fn test_parse_cid_v0() {
        let info = parse_cid_info("QmYwAPJzv5CZsnA625s3Xf2nemtYgPpHdWEz79ojWnPbdG").unwrap();
        assert_eq!(info.version, 0);
        assert_eq!(info.codec, "dag-pb");
        assert_eq!(info.hash_algo, "sha2-256");
    }

    #[test]
    fn test_parse_cid_v1_raw() {
        // Generate a known CID
        let data = b"test";
        let result = compute_cid_bytes(data);
        let info = parse_cid_info(&result.cid_v1).unwrap();

        assert_eq!(info.version, 1);
        assert_eq!(info.codec, "raw");
        assert_eq!(info.hash_algo, "sha2-256");
    }
}
