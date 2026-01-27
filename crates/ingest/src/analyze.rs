//! Trait analysis for determining bitmap sizing and building schemas.

use crate::NormalizedAsset;
use std::collections::{BTreeMap, HashMap};
use viewer_binary::BitmapSize;

/// Result of analyzing a collection's traits.
#[derive(Debug)]
pub struct TraitAnalysis {
    /// Trait name -> (value -> count) mapping
    pub trait_values: BTreeMap<String, BTreeMap<String, usize>>,
    /// Total number of trait:value combinations
    pub total_combinations: usize,
    /// Selected bitmap size
    pub bitmap_size: BitmapSize,
}

impl TraitAnalysis {
    /// Analyze traits from a collection of assets.
    ///
    /// Ignores traits in the `ignore` list (typically unique identifiers).
    pub fn from_assets(
        assets: &[NormalizedAsset],
        ignore: &[String],
    ) -> Result<Self, AnalysisError> {
        let ignore_set: std::collections::HashSet<_> = ignore.iter().collect();
        let mut trait_values: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();

        for asset in assets {
            for (trait_name, values) in &asset.traits {
                if ignore_set.contains(trait_name) {
                    continue;
                }

                let value_map = trait_values.entry(trait_name.clone()).or_default();
                for value in values {
                    *value_map.entry(value.clone()).or_default() += 1;
                }
            }
        }

        let total_combinations: usize = trait_values.values().map(|v| v.len()).sum();

        let bitmap_size =
            BitmapSize::for_count(total_combinations).ok_or(AnalysisError::TooManyTraitValues {
                count: total_combinations,
                max: 512,
            })?;

        Ok(Self {
            trait_values,
            total_combinations,
            bitmap_size,
        })
    }

    /// Get a summary string for logging.
    pub fn summary(&self) -> String {
        let trait_summary: Vec<String> = self
            .trait_values
            .iter()
            .map(|(name, values)| format!("{} ({})", name, values.len()))
            .collect();

        format!(
            "{} traits, {} total values, bitmap: {}\n  {}",
            self.trait_values.len(),
            self.total_combinations,
            self.bitmap_size,
            trait_summary.join(", ")
        )
    }
}

/// Errors during trait analysis.
#[derive(Debug, thiserror::Error)]
pub enum AnalysisError {
    #[error("Too many trait:value combinations: {count} (max {max}). Add traits to ignore list.")]
    TooManyTraitValues { count: usize, max: usize },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_asset(traits: &[(&str, &str)]) -> NormalizedAsset {
        let mut trait_map = HashMap::new();
        for (name, value) in traits {
            trait_map
                .entry(name.to_string())
                .or_insert_with(Vec::new)
                .push(value.to_string());
        }
        NormalizedAsset {
            encoded_name: "test".to_string(),
            display_name: "Test".to_string(),
            traits: trait_map,
            rarity_rank: None,
            image_url: None,
        }
    }

    #[test]
    fn test_basic_analysis() {
        let assets = vec![
            make_asset(&[("Background", "Blue"), ("Eyes", "Happy")]),
            make_asset(&[("Background", "Red"), ("Eyes", "Sad")]),
            make_asset(&[("Background", "Blue"), ("Eyes", "Happy")]),
        ];

        let analysis = TraitAnalysis::from_assets(&assets, &[]).unwrap();

        assert_eq!(analysis.trait_values.len(), 2);
        assert_eq!(analysis.total_combinations, 4); // Blue, Red, Happy, Sad
        assert_eq!(analysis.bitmap_size, BitmapSize::U64);

        // Check counts
        assert_eq!(analysis.trait_values["Background"]["Blue"], 2);
        assert_eq!(analysis.trait_values["Background"]["Red"], 1);
    }

    #[test]
    fn test_ignore_traits() {
        let assets = vec![
            make_asset(&[("Background", "Blue"), ("Serial", "001")]),
            make_asset(&[("Background", "Red"), ("Serial", "002")]),
        ];

        let analysis = TraitAnalysis::from_assets(&assets, &["Serial".to_string()]).unwrap();

        assert_eq!(analysis.trait_values.len(), 1);
        assert!(!analysis.trait_values.contains_key("Serial"));
    }
}
