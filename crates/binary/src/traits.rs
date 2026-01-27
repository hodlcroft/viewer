//! Trait schema for defining collection attributes.
//!
//! The schema maps trait names and values to bitmap positions, enabling
//! fast bitwise filtering.

use crate::{BinaryFormatError, StringRef};

/// Definition of a single trait value.
#[derive(Debug, Clone)]
pub struct ValueDef {
    /// Reference to value name in string table
    pub name: StringRef,
    /// Number of tokens with this value
    pub count: u16,
}

/// Definition of a single trait.
#[derive(Debug, Clone)]
pub struct TraitDef {
    /// Reference to trait name in string table
    pub name: StringRef,
    /// Starting bit position in the attribute bitmap
    pub bitmap_offset: u16,
    /// Possible values for this trait
    pub values: Vec<ValueDef>,
}

impl TraitDef {
    /// Number of values for this trait.
    pub fn value_count(&self) -> usize {
        self.values.len()
    }

    /// Get the bitmap bit index for a value index.
    pub fn bit_index(&self, value_idx: usize) -> u16 {
        self.bitmap_offset + value_idx as u16
    }
}

/// Complete trait schema for a collection.
#[derive(Debug, Clone)]
pub struct TraitSchema {
    /// All trait definitions
    pub traits: Vec<TraitDef>,
    /// Total number of trait:value combinations (bitmap bits needed)
    pub total_values: usize,
}

impl TraitSchema {
    /// Find a trait by name reference.
    pub fn find_trait(&self, name: StringRef) -> Option<(usize, &TraitDef)> {
        self.traits.iter().enumerate().find(|(_, t)| t.name == name)
    }

    /// Serialize to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();

        // Trait count
        buf.push(self.traits.len() as u8);

        // Total values (for quick bitmap size lookup)
        buf.extend_from_slice(&(self.total_values as u16).to_le_bytes());

        for trait_def in &self.traits {
            // Trait name ref
            buf.extend_from_slice(&trait_def.name.0.to_le_bytes());
            // Bitmap offset
            buf.extend_from_slice(&trait_def.bitmap_offset.to_le_bytes());
            // Value count
            buf.push(trait_def.values.len() as u8);

            for value in &trait_def.values {
                // Value name ref
                buf.extend_from_slice(&value.name.0.to_le_bytes());
                // Count
                buf.extend_from_slice(&value.count.to_le_bytes());
            }
        }

        buf
    }

    /// Deserialize from bytes.
    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < 3 {
            return None;
        }

        let trait_count = buf[0] as usize;
        let total_values = u16::from_le_bytes([buf[1], buf[2]]) as usize;

        let mut offset = 3;
        let mut traits = Vec::with_capacity(trait_count);

        for _ in 0..trait_count {
            if offset + 5 > buf.len() {
                return None;
            }

            let name = StringRef(u16::from_le_bytes([buf[offset], buf[offset + 1]]));
            let bitmap_offset = u16::from_le_bytes([buf[offset + 2], buf[offset + 3]]);
            let value_count = buf[offset + 4] as usize;
            offset += 5;

            let mut values = Vec::with_capacity(value_count);
            for _ in 0..value_count {
                if offset + 4 > buf.len() {
                    return None;
                }

                let value_name = StringRef(u16::from_le_bytes([buf[offset], buf[offset + 1]]));
                let count = u16::from_le_bytes([buf[offset + 2], buf[offset + 3]]);
                offset += 4;

                values.push(ValueDef {
                    name: value_name,
                    count,
                });
            }

            traits.push(TraitDef {
                name,
                bitmap_offset,
                values,
            });
        }

        Some(TraitSchema {
            traits,
            total_values,
        })
    }
}

/// Builder for constructing a trait schema during ingestion.
#[derive(Clone)]
pub struct TraitSchemaBuilder {
    traits: Vec<TraitDefBuilder>,
    current_offset: u16,
}

#[derive(Clone)]
struct TraitDefBuilder {
    name: StringRef,
    bitmap_offset: u16,
    values: Vec<(StringRef, u16)>, // (name_ref, count)
}

impl TraitSchemaBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            traits: Vec::new(),
            current_offset: 0,
        }
    }

    /// Add a trait with its values.
    ///
    /// Values should be provided as (name_ref, count) pairs.
    pub fn add_trait(
        &mut self,
        name: StringRef,
        values: Vec<(StringRef, u16)>,
    ) -> Result<(), BinaryFormatError> {
        let value_count = values.len();
        if self.current_offset as usize + value_count > 512 {
            return Err(BinaryFormatError::TooManyTraitValues(
                self.current_offset as usize + value_count,
            ));
        }

        self.traits.push(TraitDefBuilder {
            name,
            bitmap_offset: self.current_offset,
            values,
        });

        self.current_offset += value_count as u16;
        Ok(())
    }

    /// Get the total number of trait:value combinations.
    pub fn total_values(&self) -> usize {
        self.current_offset as usize
    }

    /// Get the number of traits.
    pub fn trait_count(&self) -> usize {
        self.traits.len()
    }

    /// Get the bitmap offset for a trait index.
    pub fn value_offset(&self, trait_idx: usize) -> usize {
        if trait_idx < self.traits.len() {
            self.traits[trait_idx].bitmap_offset as usize
        } else {
            0
        }
    }

    /// Build the final schema.
    pub fn build(self) -> TraitSchema {
        let total_values = self.current_offset as usize;
        let traits = self
            .traits
            .into_iter()
            .map(|t| TraitDef {
                name: t.name,
                bitmap_offset: t.bitmap_offset,
                values: t
                    .values
                    .into_iter()
                    .map(|(name, count)| ValueDef { name, count })
                    .collect(),
            })
            .collect();

        TraitSchema {
            traits,
            total_values,
        }
    }
}

impl Default for TraitSchemaBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trait_schema_builder() {
        let mut builder = TraitSchemaBuilder::new();

        // Background with 3 values
        builder
            .add_trait(
                StringRef(10),
                vec![
                    (StringRef(20), 100),
                    (StringRef(21), 150),
                    (StringRef(22), 50),
                ],
            )
            .unwrap();

        // Eyes with 2 values
        builder
            .add_trait(
                StringRef(30),
                vec![(StringRef(40), 200), (StringRef(41), 100)],
            )
            .unwrap();

        assert_eq!(builder.total_values(), 5);

        let schema = builder.build();
        assert_eq!(schema.traits.len(), 2);
        assert_eq!(schema.traits[0].bitmap_offset, 0);
        assert_eq!(schema.traits[1].bitmap_offset, 3);
    }

    #[test]
    fn test_trait_schema_serialization() {
        let mut builder = TraitSchemaBuilder::new();
        builder
            .add_trait(
                StringRef(10),
                vec![(StringRef(20), 100), (StringRef(21), 150)],
            )
            .unwrap();

        let schema = builder.build();
        let bytes = schema.to_bytes();
        let restored = TraitSchema::from_bytes(&bytes).unwrap();

        assert_eq!(restored.traits.len(), 1);
        assert_eq!(restored.traits[0].name, StringRef(10));
        assert_eq!(restored.traits[0].values.len(), 2);
        assert_eq!(restored.total_values, 2);
    }
}
