//! Source provenance metadata.
//!
//! Collections can have one or more blockchain sources. Each token is
//! associated with a source, enabling filtering and display by origin.
//!
//! For single-source collections (the common case), no per-token overhead
//! is added - all tokens implicitly belong to source 0.

use crate::StringRef;

/// Metadata for a single blockchain source.
#[derive(Debug, Clone)]
pub struct SourceMetadata {
    /// Chain name (e.g., "cardano", "ethereum") - reference into string table
    pub chain: StringRef,
    /// Chain-specific identifier (policy_id, contract address) - reference into string table
    pub id: StringRef,
    /// Number of tokens from this source
    pub token_count: u32,
    /// When this source was synced (Unix timestamp)
    pub synced_at: u32,
}

impl SourceMetadata {
    /// Byte size of a serialized source entry.
    pub const SIZE: usize = 12; // 2 + 2 + 4 + 4

    /// Serialize to bytes.
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut buf = [0u8; Self::SIZE];
        buf[0..2].copy_from_slice(&self.chain.0.to_le_bytes());
        buf[2..4].copy_from_slice(&self.id.0.to_le_bytes());
        buf[4..8].copy_from_slice(&self.token_count.to_le_bytes());
        buf[8..12].copy_from_slice(&self.synced_at.to_le_bytes());
        buf
    }

    /// Deserialize from bytes.
    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < Self::SIZE {
            return None;
        }
        Some(Self {
            chain: StringRef(u16::from_le_bytes([buf[0], buf[1]])),
            id: StringRef(u16::from_le_bytes([buf[2], buf[3]])),
            token_count: u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]),
            synced_at: u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]),
        })
    }
}

/// Sources section containing all source metadata.
#[derive(Debug, Clone)]
pub struct SourcesSection {
    pub sources: Vec<SourceMetadata>,
}

impl SourcesSection {
    /// Create a new sources section.
    pub fn new(sources: Vec<SourceMetadata>) -> Self {
        Self { sources }
    }

    /// Create for a single Cardano source.
    pub fn single_cardano(chain_ref: StringRef, id_ref: StringRef, token_count: u32) -> Self {
        Self {
            sources: vec![SourceMetadata {
                chain: chain_ref,
                id: id_ref,
                token_count,
                synced_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as u32)
                    .unwrap_or(0),
            }],
        }
    }

    /// Check if this is a multi-source collection.
    pub fn is_multi_source(&self) -> bool {
        self.sources.len() > 1
    }

    /// Number of sources.
    pub fn count(&self) -> usize {
        self.sources.len()
    }

    /// Serialize to bytes.
    ///
    /// Format: [count: u8][sources: SourceMetadata * count]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(1 + self.sources.len() * SourceMetadata::SIZE);
        buf.push(self.sources.len() as u8);
        for source in &self.sources {
            buf.extend_from_slice(&source.to_bytes());
        }
        buf
    }

    /// Deserialize from bytes.
    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.is_empty() {
            return None;
        }
        let count = buf[0] as usize;
        let mut sources = Vec::with_capacity(count);
        let mut offset = 1;

        for _ in 0..count {
            if offset + SourceMetadata::SIZE > buf.len() {
                return None;
            }
            sources.push(SourceMetadata::from_bytes(&buf[offset..])?);
            offset += SourceMetadata::SIZE;
        }

        Some(Self { sources })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_source_metadata_roundtrip() {
        let source = SourceMetadata {
            chain: StringRef(10),
            id: StringRef(20),
            token_count: 2000,
            synced_at: 1706400000,
        };

        let bytes = source.to_bytes();
        let restored = SourceMetadata::from_bytes(&bytes).unwrap();

        assert_eq!(restored.chain, StringRef(10));
        assert_eq!(restored.id, StringRef(20));
        assert_eq!(restored.token_count, 2000);
        assert_eq!(restored.synced_at, 1706400000);
    }

    #[test]
    fn test_sources_section_roundtrip() {
        let section = SourcesSection::new(vec![
            SourceMetadata {
                chain: StringRef(10),
                id: StringRef(20),
                token_count: 1000,
                synced_at: 1706400000,
            },
            SourceMetadata {
                chain: StringRef(30),
                id: StringRef(40),
                token_count: 500,
                synced_at: 1706500000,
            },
        ]);

        assert!(section.is_multi_source());

        let bytes = section.to_bytes();
        let restored = SourcesSection::from_bytes(&bytes).unwrap();

        assert_eq!(restored.sources.len(), 2);
        assert_eq!(restored.sources[0].token_count, 1000);
        assert_eq!(restored.sources[1].token_count, 500);
    }

    #[test]
    fn test_single_source() {
        let section = SourcesSection::single_cardano(StringRef(5), StringRef(10), 2000);

        assert!(!section.is_multi_source());
        assert_eq!(section.count(), 1);
    }
}
