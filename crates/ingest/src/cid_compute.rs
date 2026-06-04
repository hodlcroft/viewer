//! IPFS CID computation for local files.
//!
//! Computes CIDs using the same algorithm as IPFS to enable
//! checking if files already exist on IPFS before uploading.
//!
//! Matches the default `ipfs add` behavior (CIDv0, no raw leaves, 256KB chunks).

use cid::Cid;
use multihash_codetable::{Code, MultihashDigest};
use std::path::Path;

/// IPFS codec for dag-pb (protobuf)
const DAG_PB_CODEC: u64 = 0x70;

/// Default IPFS chunk size (256 KiB)
const DEFAULT_CHUNK_SIZE: usize = 262144;

/// Maximum links per node in balanced DAG (IPFS default)
const MAX_LINKS_PER_NODE: usize = 174;

/// Result of CID computation with metadata
#[derive(Debug, Clone)]
pub struct ComputedCid {
    /// The computed CID (v1 format)
    pub cid_v1: String,
    /// The computed CID (v0 format)
    pub cid_v0: Option<String>,
    /// File size in bytes
    pub file_size: u64,
}

/// Compute CID for a file (matches `ipfs add` defaults)
pub fn compute_cid(path: &Path) -> std::io::Result<ComputedCid> {
    let data = std::fs::read(path)?;
    Ok(compute_cid_bytes(&data))
}

/// Compute CID for raw bytes (matches `ipfs add` defaults)
pub fn compute_cid_bytes(data: &[u8]) -> ComputedCid {
    let file_size = data.len() as u64;

    // Single chunk - wrap in UnixFS and return
    if data.len() <= DEFAULT_CHUNK_SIZE {
        let node_bytes = create_unixfs_file_node(data, &[]);
        let digest = Code::Sha2_256.digest(&node_bytes);
        let cid_v1 = Cid::new_v1(DAG_PB_CODEC, digest);
        let cid_v0 = Cid::new_v0(digest).ok();

        return ComputedCid {
            cid_v1: cid_v1.to_string(),
            cid_v0: cid_v0.map(|c| c.to_string()),
            file_size,
        };
    }

    // Multi-chunk file - build balanced DAG
    let chunks: Vec<&[u8]> = data.chunks(DEFAULT_CHUNK_SIZE).collect();

    // Create leaf nodes (each chunk wrapped in UnixFS)
    let mut leaves: Vec<LinkInfo> = chunks
        .iter()
        .map(|chunk| {
            let node_bytes = create_unixfs_file_node(chunk, &[]);
            let digest = Code::Sha2_256.digest(&node_bytes);
            let cid = Cid::new_v1(DAG_PB_CODEC, digest);
            LinkInfo {
                cid,
                // tsize is the size of the serialized node (for leaves, this is the protobuf size)
                total_size: node_bytes.len() as u64,
                // blocksize is the actual file data size in this chunk
                file_size: chunk.len() as u64,
            }
        })
        .collect();

    // Build tree bottom-up until we have a single root
    while leaves.len() > 1 {
        leaves = build_tree_level(&leaves);
    }

    let root = &leaves[0];
    let cid_v0 = Cid::new_v0(root.cid.hash().to_owned()).ok();

    ComputedCid {
        cid_v1: root.cid.to_string(),
        cid_v0: cid_v0.map(|c| c.to_string()),
        file_size,
    }
}

/// Information about a link in the DAG
struct LinkInfo {
    cid: Cid,
    total_size: u64, // dag-pb tsize (size of linked subtree)
    file_size: u64,  // UnixFS blocksize
}

/// Build one level of the tree by grouping nodes
fn build_tree_level(nodes: &[LinkInfo]) -> Vec<LinkInfo> {
    nodes
        .chunks(MAX_LINKS_PER_NODE)
        .map(|group| {
            let total_file_size: u64 = group.iter().map(|n| n.file_size).sum();
            let links: Vec<_> = group.iter().collect();
            let node_bytes = create_unixfs_link_node(&links, total_file_size);
            let digest = Code::Sha2_256.digest(&node_bytes);
            let cid = Cid::new_v1(DAG_PB_CODEC, digest);

            LinkInfo {
                cid,
                total_size: node_bytes.len() as u64
                    + group.iter().map(|n| n.total_size).sum::<u64>(),
                file_size: total_file_size,
            }
        })
        .collect()
}

/// Create a UnixFS file node (leaf node with data)
fn create_unixfs_file_node(data: &[u8], _links: &[&LinkInfo]) -> Vec<u8> {
    use prost::Message;

    // UnixFS Data message
    #[derive(Clone, PartialEq, Message)]
    struct UnixFsData {
        #[prost(int32, tag = "1")]
        r#type: i32,
        #[prost(bytes, optional, tag = "2")]
        data: Option<Vec<u8>>,
        #[prost(uint64, optional, tag = "3")]
        filesize: Option<u64>,
    }

    // PBNode message (dag-pb)
    #[derive(Clone, PartialEq, Message)]
    struct PBNode {
        #[prost(bytes, optional, tag = "1")]
        data: Option<Vec<u8>>,
    }

    const DATA_TYPE_FILE: i32 = 2;

    let unixfs = UnixFsData {
        r#type: DATA_TYPE_FILE,
        data: Some(data.to_vec()),
        filesize: Some(data.len() as u64),
    };

    let mut unixfs_bytes = Vec::new();
    unixfs.encode(&mut unixfs_bytes).unwrap();

    let node = PBNode {
        data: Some(unixfs_bytes),
    };

    let mut node_bytes = Vec::new();
    node.encode(&mut node_bytes).unwrap();
    node_bytes
}

/// Create a UnixFS link node (intermediate node with links to children)
///
/// dag-pb requires a specific canonical encoding order: Links (field 2) before Data (field 1)
/// This is non-standard protobuf ordering, so we encode manually.
fn create_unixfs_link_node(links: &[&LinkInfo], total_file_size: u64) -> Vec<u8> {
    use prost::Message;

    // UnixFS Data message
    #[derive(Clone, PartialEq, Message)]
    struct UnixFsData {
        #[prost(int32, tag = "1")]
        r#type: i32,
        #[prost(uint64, optional, tag = "3")]
        filesize: Option<u64>,
        #[prost(uint64, repeated, packed = "false", tag = "4")]
        blocksizes: Vec<u64>,
    }

    // PBLink message
    #[derive(Clone, PartialEq, Message)]
    struct PBLink {
        #[prost(bytes, optional, tag = "1")]
        hash: Option<Vec<u8>>,
        #[prost(string, optional, tag = "2")]
        name: Option<String>,
        #[prost(uint64, optional, tag = "3")]
        tsize: Option<u64>,
    }

    const DATA_TYPE_FILE: i32 = 2;

    let blocksizes: Vec<u64> = links.iter().map(|l| l.file_size).collect();

    let unixfs = UnixFsData {
        r#type: DATA_TYPE_FILE,
        filesize: Some(total_file_size),
        blocksizes,
    };

    let mut unixfs_bytes = Vec::new();
    unixfs.encode(&mut unixfs_bytes).unwrap();

    // Manually encode PBNode with dag-pb canonical ordering:
    // Links (field 2) MUST come before Data (field 1)
    let mut node_bytes = Vec::new();

    // Encode each link as field 2 (tag = 0x12 for length-delimited field 2)
    for link in links {
        let pb_link = PBLink {
            // dag-pb links store the raw multihash, not the full CID bytes
            hash: Some(link.cid.hash().to_bytes()),
            name: Some(String::new()),
            tsize: Some(link.total_size),
        };
        let mut link_bytes = Vec::new();
        pb_link.encode(&mut link_bytes).unwrap();

        // Field 2, wire type 2 (length-delimited) = (2 << 3) | 2 = 0x12
        node_bytes.push(0x12);
        encode_varint(link_bytes.len() as u64, &mut node_bytes);
        node_bytes.extend(&link_bytes);
    }

    // Encode Data as field 1 (tag = 0x0a for length-delimited field 1)
    node_bytes.push(0x0a);
    encode_varint(unixfs_bytes.len() as u64, &mut node_bytes);
    node_bytes.extend(&unixfs_bytes);

    node_bytes
}

/// Encode a varint
fn encode_varint(mut value: u64, buf: &mut Vec<u8>) {
    while value >= 0x80 {
        buf.push((value as u8) | 0x80);
        value >>= 7;
    }
    buf.push(value as u8);
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
        0x55 => "raw".to_string(),
        0x70 => "dag-pb".to_string(),
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

/// Try to match a computed CID against a target CID
pub fn find_matching_cid(data: &[u8], target_cid: &str) -> Option<ComputedCid> {
    let target = Cid::try_from(target_cid).ok()?;
    let target_v1 = if target.version() == cid::Version::V0 {
        Cid::new_v1(target.codec(), target.hash().to_owned()).to_string()
    } else {
        target.to_string()
    };

    let computed = compute_cid_bytes(data);

    // Check v1 match
    if computed.cid_v1 == target_v1 {
        return Some(computed);
    }

    // Check v0 match
    if target.version() == cid::Version::V0
        && let Some(ref v0) = computed.cid_v0
            && v0 == target_cid {
                return Some(computed);
            }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_small_file_cid() {
        let data = b"Hello, World!";
        let result = compute_cid_bytes(data);

        assert!(result.cid_v0.is_some());
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
    fn test_two_chunk_file() {
        // 512KB of zeros = 2 chunks of 256KB each
        let data = vec![0u8; 524288];
        let result = compute_cid_bytes(&data);

        // Expected CID from `ipfs add --only-hash` on this file
        assert_eq!(
            result.cid_v0.as_ref().unwrap(),
            "QmeBAFpC3fbNhVMsExM8uS23gKmiaPQJbNu5rFEKDGdhcW"
        );
    }

    #[test]
    fn test_single_chunk_zeros() {
        // 256KB of zeros = 1 chunk
        let data = vec![0u8; 262144];
        let result = compute_cid_bytes(&data);

        // Expected CID from `ipfs add --only-hash` on this file
        assert_eq!(
            result.cid_v0.as_ref().unwrap(),
            "QmRk1rduJvo5DfEYAaLobS2za9tDszk35hzaNSDCJ74DA7"
        );
    }

    #[test]
    fn test_link_node_encoding() {
        use prost::Message;

        #[derive(Clone, PartialEq, Message)]
        struct PBLink {
            #[prost(bytes, optional, tag = "1")]
            hash: Option<Vec<u8>>,
            #[prost(string, optional, tag = "2")]
            name: Option<String>,
            #[prost(uint64, optional, tag = "3")]
            tsize: Option<u64>,
        }

        // The leaf CID for 256KB of zeros
        let leaf_cid: Cid = "QmRk1rduJvo5DfEYAaLobS2za9tDszk35hzaNSDCJ74DA7"
            .parse()
            .unwrap();

        let pb_link = PBLink {
            hash: Some(leaf_cid.to_bytes()),
            name: Some(String::new()),
            tsize: Some(262158),
        };

        let mut link_bytes = Vec::new();
        pb_link.encode(&mut link_bytes).unwrap();

        // Expected from IPFS:
        let expected_hex =
            "0a221220328f549938f9ca71d855f81335f36dafa2a8ba0e8ec8595c583e08e2f70995f81200188e8010";
        let actual_hex = hex::encode(&link_bytes);

        eprintln!("Expected: {}", expected_hex);
        eprintln!("Actual:   {}", actual_hex);

        assert_eq!(actual_hex, expected_hex, "Link encoding mismatch");
    }

    #[test]
    fn test_root_node_encoding() {
        // Build what the root node should be for 512KB of zeros (2 chunks)
        // IPFS produces: 122a0a221220328f549938f9ca71d855f81335f36dafa2a8ba0e8ec8595c583e08e2f70995f81200188e8010122a0a221220328f549938f9ca71d855f81335f36dafa2a8ba0e8ec8595c583e08e2f70995f81200188e80100a0e0802188080202080801020808010

        let leaf_cid: Cid = "QmRk1rduJvo5DfEYAaLobS2za9tDszk35hzaNSDCJ74DA7"
            .parse()
            .unwrap();

        let links = [
            LinkInfo {
                cid: leaf_cid,
                total_size: 262158, // protobuf size of leaf
                file_size: 262144,  // actual data size
            },
            LinkInfo {
                cid: leaf_cid,
                total_size: 262158,
                file_size: 262144,
            },
        ];

        let link_refs: Vec<&LinkInfo> = links.iter().collect();
        let node_bytes = create_unixfs_link_node(&link_refs, 524288);

        let expected_hex = "122a0a221220328f549938f9ca71d855f81335f36dafa2a8ba0e8ec8595c583e08e2f70995f81200188e8010122a0a221220328f549938f9ca71d855f81335f36dafa2a8ba0e8ec8595c583e08e2f70995f81200188e80100a0e0802188080202080801020808010";
        let actual_hex = hex::encode(&node_bytes);

        eprintln!("Expected: {}", expected_hex);
        eprintln!("Actual:   {}", actual_hex);
        eprintln!("Expected len: {}", expected_hex.len() / 2);
        eprintln!("Actual len:   {}", node_bytes.len());

        assert_eq!(actual_hex, expected_hex, "Root node encoding mismatch");
    }

    #[test]
    fn test_leaf_node_size() {
        // 256KB of zeros
        let chunk = vec![0u8; 262144];
        let node_bytes = create_unixfs_file_node(&chunk, &[]);

        eprintln!("Leaf node size: {} (expected 262158)", node_bytes.len());
        eprintln!("Leaf node hex: {}", hex::encode(&node_bytes[..20]));

        // IPFS tsize for this leaf is 262158
        assert_eq!(node_bytes.len(), 262158, "Leaf node size mismatch");
    }

    #[test]
    fn test_full_two_chunk_debug() {
        // Manually walk through the entire process
        let data = vec![0u8; 524288]; // 512KB

        // Step 1: Split into chunks
        let chunks: Vec<&[u8]> = data.chunks(DEFAULT_CHUNK_SIZE).collect();
        eprintln!("Chunk count: {}", chunks.len());
        assert_eq!(chunks.len(), 2);

        // Step 2: Create leaf nodes
        let leaves: Vec<LinkInfo> = chunks
            .iter()
            .map(|chunk| {
                let node_bytes = create_unixfs_file_node(chunk, &[]);
                let digest = Code::Sha2_256.digest(&node_bytes);
                let cid = Cid::new_v1(DAG_PB_CODEC, digest);
                eprintln!(
                    "Leaf: cid={}, node_size={}, chunk_size={}",
                    cid,
                    node_bytes.len(),
                    chunk.len()
                );
                LinkInfo {
                    cid,
                    total_size: node_bytes.len() as u64,
                    file_size: chunk.len() as u64,
                }
            })
            .collect();

        // Verify leaf CIDs
        let expected_leaf = "QmRk1rduJvo5DfEYAaLobS2za9tDszk35hzaNSDCJ74DA7";
        let leaf_v0 = Cid::new_v0(leaves[0].cid.hash().to_owned()).unwrap();
        eprintln!("Leaf v0: {}", leaf_v0);
        assert_eq!(leaf_v0.to_string(), expected_leaf, "Leaf CID mismatch");

        // Step 3: Build root node
        let link_refs: Vec<&LinkInfo> = leaves.iter().collect();
        let root_bytes = create_unixfs_link_node(&link_refs, 524288);
        eprintln!("Root node size: {}", root_bytes.len());

        let root_digest = Code::Sha2_256.digest(&root_bytes);
        let root_cid = Cid::new_v1(DAG_PB_CODEC, root_digest);
        let root_v0 = Cid::new_v0(root_cid.hash().to_owned()).unwrap();

        eprintln!("Root CID v1: {}", root_cid);
        eprintln!("Root CID v0: {}", root_v0);

        assert_eq!(
            root_v0.to_string(),
            "QmeBAFpC3fbNhVMsExM8uS23gKmiaPQJbNu5rFEKDGdhcW",
            "Root CID mismatch"
        );
    }
}
