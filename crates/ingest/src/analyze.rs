//! Trait analysis for determining bitmap sizing and building schemas.

use crate::NormalizedAsset;
use std::collections::BTreeMap;
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
                max: BitmapSize::max_supported(),
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

    /// Total number of trait values (alias for total_combinations).
    pub fn total_values(&self) -> usize {
        self.total_combinations
    }

    /// Iterator over trait names and their values with counts.
    ///
    /// Returns tuples of (trait_name, Vec<(value, count)>).
    pub fn trait_values(&self) -> impl Iterator<Item = (&str, Vec<(String, u16)>)> {
        self.trait_values.iter().map(|(name, values)| {
            let values_with_counts: Vec<(String, u16)> =
                values.iter().map(|(v, c)| (v.clone(), *c as u16)).collect();
            (name.as_str(), values_with_counts)
        })
    }

    /// Encode a trait name and its values to (trait_index, value_index) pairs.
    ///
    /// Returns a pair for each value the token has for this trait.
    /// Returns an empty vec if the trait is not found (e.g., was ignored).
    pub fn encode_trait(&self, name: &str, values: &[String]) -> Vec<(u8, u8)> {
        let Some(trait_idx) = self.trait_values.keys().position(|n| n == name) else {
            return Vec::new();
        };
        let Some(trait_values) = self.trait_values.get(name) else {
            return Vec::new();
        };

        values
            .iter()
            .filter_map(|v| {
                let value_idx = trait_values.keys().position(|tv| tv == v)?;
                Some((trait_idx as u8, value_idx as u8))
            })
            .collect()
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
    use std::collections::HashMap;

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
