//! Claude model pricing data (per-million USD, base tier only).
//!
//! Source: <https://github.com/BerriAI/litellm/blob/98ced0ae4371b47d6f70527be7054c18df755266/model_prices_and_context_window.json>

/// Pricing for a single Claude model, stored as per-million-token USD costs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PricingEntry {
    pub input_per_million: f64,
    pub output_per_million: f64,
    pub cache_creation_per_million: f64,
    pub cache_read_per_million: f64,
}

const PRICING_TABLE: &[(&str, PricingEntry)] = &[
    (
        "claude-3-7-sonnet-20250219",
        PricingEntry {
            input_per_million: 3.0,
            output_per_million: 15.0,
            cache_creation_per_million: 3.75,
            cache_read_per_million: 0.3,
        },
    ),
    (
        "claude-3-haiku-20240307",
        PricingEntry {
            input_per_million: 0.25,
            output_per_million: 1.25,
            cache_creation_per_million: 0.3,
            cache_read_per_million: 0.03,
        },
    ),
    (
        "claude-3-opus-20240229",
        PricingEntry {
            input_per_million: 15.0,
            output_per_million: 75.0,
            cache_creation_per_million: 18.75,
            cache_read_per_million: 1.5,
        },
    ),
    (
        "claude-4-opus-20250514",
        PricingEntry {
            input_per_million: 15.0,
            output_per_million: 75.0,
            cache_creation_per_million: 18.75,
            cache_read_per_million: 1.5,
        },
    ),
    (
        "claude-4-sonnet-20250514",
        PricingEntry {
            input_per_million: 3.0,
            output_per_million: 15.0,
            cache_creation_per_million: 3.75,
            cache_read_per_million: 0.3,
        },
    ),
    (
        "claude-haiku-4-5",
        PricingEntry {
            input_per_million: 1.0,
            output_per_million: 5.0,
            cache_creation_per_million: 1.25,
            cache_read_per_million: 0.1,
        },
    ),
    (
        "claude-haiku-4-5-20251001",
        PricingEntry {
            input_per_million: 1.0,
            output_per_million: 5.0,
            cache_creation_per_million: 1.25,
            cache_read_per_million: 0.1,
        },
    ),
    (
        "claude-opus-4-1",
        PricingEntry {
            input_per_million: 15.0,
            output_per_million: 75.0,
            cache_creation_per_million: 18.75,
            cache_read_per_million: 1.5,
        },
    ),
    (
        "claude-opus-4-1-20250805",
        PricingEntry {
            input_per_million: 15.0,
            output_per_million: 75.0,
            cache_creation_per_million: 18.75,
            cache_read_per_million: 1.5,
        },
    ),
    (
        "claude-opus-4-20250514",
        PricingEntry {
            input_per_million: 15.0,
            output_per_million: 75.0,
            cache_creation_per_million: 18.75,
            cache_read_per_million: 1.5,
        },
    ),
    (
        "claude-opus-4-5",
        PricingEntry {
            input_per_million: 5.0,
            output_per_million: 25.0,
            cache_creation_per_million: 6.25,
            cache_read_per_million: 0.5,
        },
    ),
    (
        "claude-opus-4-5-20251101",
        PricingEntry {
            input_per_million: 5.0,
            output_per_million: 25.0,
            cache_creation_per_million: 6.25,
            cache_read_per_million: 0.5,
        },
    ),
    (
        "claude-opus-4-6",
        PricingEntry {
            input_per_million: 5.0,
            output_per_million: 25.0,
            cache_creation_per_million: 6.25,
            cache_read_per_million: 0.5,
        },
    ),
    (
        "claude-opus-4-6-20260205",
        PricingEntry {
            input_per_million: 5.0,
            output_per_million: 25.0,
            cache_creation_per_million: 6.25,
            cache_read_per_million: 0.5,
        },
    ),
    (
        "claude-opus-4-7",
        PricingEntry {
            input_per_million: 5.0,
            output_per_million: 25.0,
            cache_creation_per_million: 6.25,
            cache_read_per_million: 0.5,
        },
    ),
    (
        "claude-opus-4-7-20260416",
        PricingEntry {
            input_per_million: 5.0,
            output_per_million: 25.0,
            cache_creation_per_million: 6.25,
            cache_read_per_million: 0.5,
        },
    ),
    (
        "claude-sonnet-4-20250514",
        PricingEntry {
            input_per_million: 3.0,
            output_per_million: 15.0,
            cache_creation_per_million: 3.75,
            cache_read_per_million: 0.3,
        },
    ),
    (
        "claude-sonnet-4-5",
        PricingEntry {
            input_per_million: 3.0,
            output_per_million: 15.0,
            cache_creation_per_million: 3.75,
            cache_read_per_million: 0.3,
        },
    ),
    (
        "claude-sonnet-4-5-20250929",
        PricingEntry {
            input_per_million: 3.0,
            output_per_million: 15.0,
            cache_creation_per_million: 3.75,
            cache_read_per_million: 0.3,
        },
    ),
    (
        "claude-sonnet-4-5-20250929-v1:0",
        PricingEntry {
            input_per_million: 3.0,
            output_per_million: 15.0,
            cache_creation_per_million: 3.75,
            cache_read_per_million: 0.3,
        },
    ),
    (
        "claude-sonnet-4-6",
        PricingEntry {
            input_per_million: 3.0,
            output_per_million: 15.0,
            cache_creation_per_million: 3.75,
            cache_read_per_million: 0.3,
        },
    ),
];

/// Look up pricing for a Claude model by its exact model ID.
///
/// Returns `None` for unknown, empty, or non-exact-match model IDs.
pub fn lookup_pricing(model_id: &str) -> Option<PricingEntry> {
    PRICING_TABLE
        .iter()
        .find(|(k, _)| *k == model_id)
        .map(|(_, v)| *v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opus_4_7() {
        assert_eq!(
            lookup_pricing("claude-opus-4-7"),
            Some(PricingEntry {
                input_per_million: 5.0,
                output_per_million: 25.0,
                cache_creation_per_million: 6.25,
                cache_read_per_million: 0.5,
            })
        );
    }

    #[test]
    fn test_haiku_3() {
        assert_eq!(
            lookup_pricing("claude-3-haiku-20240307"),
            Some(PricingEntry {
                input_per_million: 0.25,
                output_per_million: 1.25,
                cache_creation_per_million: 0.3,
                cache_read_per_million: 0.03,
            })
        );
    }

    #[test]
    fn test_haiku_4_5() {
        assert_eq!(
            lookup_pricing("claude-haiku-4-5"),
            Some(PricingEntry {
                input_per_million: 1.0,
                output_per_million: 5.0,
                cache_creation_per_million: 1.25,
                cache_read_per_million: 0.1,
            })
        );
    }

    #[test]
    fn test_unknown_model_returns_none() {
        assert_eq!(lookup_pricing("gpt-4"), None);
    }

    #[test]
    fn test_empty_string_returns_none() {
        assert_eq!(lookup_pricing(""), None);
    }

    #[test]
    fn test_no_fuzzy_match() {
        // "claude-opus-4" is not in the table; only exact IDs match
        assert_eq!(lookup_pricing("claude-opus-4"), None);
    }

    #[test]
    fn test_table_coverage() {
        assert_eq!(PRICING_TABLE.len(), 21);

        let expected: &[(&str, [f64; 4])] = &[
            ("claude-3-7-sonnet-20250219", [3.0, 15.0, 3.75, 0.3]),
            ("claude-3-haiku-20240307", [0.25, 1.25, 0.3, 0.03]),
            ("claude-3-opus-20240229", [15.0, 75.0, 18.75, 1.5]),
            ("claude-4-opus-20250514", [15.0, 75.0, 18.75, 1.5]),
            ("claude-4-sonnet-20250514", [3.0, 15.0, 3.75, 0.3]),
            ("claude-haiku-4-5", [1.0, 5.0, 1.25, 0.1]),
            ("claude-haiku-4-5-20251001", [1.0, 5.0, 1.25, 0.1]),
            ("claude-opus-4-1", [15.0, 75.0, 18.75, 1.5]),
            ("claude-opus-4-1-20250805", [15.0, 75.0, 18.75, 1.5]),
            ("claude-opus-4-20250514", [15.0, 75.0, 18.75, 1.5]),
            ("claude-opus-4-5", [5.0, 25.0, 6.25, 0.5]),
            ("claude-opus-4-5-20251101", [5.0, 25.0, 6.25, 0.5]),
            ("claude-opus-4-6", [5.0, 25.0, 6.25, 0.5]),
            ("claude-opus-4-6-20260205", [5.0, 25.0, 6.25, 0.5]),
            ("claude-opus-4-7", [5.0, 25.0, 6.25, 0.5]),
            ("claude-opus-4-7-20260416", [5.0, 25.0, 6.25, 0.5]),
            ("claude-sonnet-4-20250514", [3.0, 15.0, 3.75, 0.3]),
            ("claude-sonnet-4-5", [3.0, 15.0, 3.75, 0.3]),
            ("claude-sonnet-4-5-20250929", [3.0, 15.0, 3.75, 0.3]),
            ("claude-sonnet-4-5-20250929-v1:0", [3.0, 15.0, 3.75, 0.3]),
            ("claude-sonnet-4-6", [3.0, 15.0, 3.75, 0.3]),
        ];

        for (model_id, [input, output, creation, read]) in expected {
            let entry =
                lookup_pricing(model_id).unwrap_or_else(|| panic!("missing entry for {model_id}"));
            assert_eq!(
                entry.input_per_million, *input,
                "input mismatch for {model_id}"
            );
            assert_eq!(
                entry.output_per_million, *output,
                "output mismatch for {model_id}"
            );
            assert_eq!(
                entry.cache_creation_per_million, *creation,
                "cache_creation mismatch for {model_id}"
            );
            assert_eq!(
                entry.cache_read_per_million, *read,
                "cache_read mismatch for {model_id}"
            );
        }
    }

    #[test]
    fn test_type_surface() {
        // Field-syntax construction
        let e = PricingEntry {
            input_per_million: 1.0,
            output_per_million: 2.0,
            cache_creation_per_million: 3.0,
            cache_read_per_million: 4.0,
        };

        // Clone: use Clone::clone to avoid clippy::clone_on_copy lint
        let e2 = Clone::clone(&e);
        assert_eq!(e, e2);

        // Copy: move after use should compile — pass by value twice
        let e3 = e;
        let e4 = e; // Copy allows second use
        assert_eq!(e3, e4);

        // Debug output contains field name
        assert!(format!("{e:?}").contains("input_per_million"));
    }
}
