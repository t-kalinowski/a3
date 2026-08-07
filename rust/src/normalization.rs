//! Normalization — the three transformations A3 applies to a valid document.
//!
//! Normalization is deliberately minimal and total: every function here
//! succeeds for every input, because deciding whether an input is *legal* is
//! [`crate::validation`]'s job, not this module's. The complete list is:
//!
//! 1. the sequence is case-folded to uppercase
//! 2. positions are sorted ascending
//! 3. ranges are sorted by start, then by end
//!
//! Nothing else changes. In particular, author-supplied key order — annotation
//! names, and a variant record's extra members — is preserved exactly as it
//! appears in the document.

/// Case-fold a sequence to uppercase.
///
/// `to_ascii_uppercase` is correct here and `to_uppercase` is not: A3 restricts
/// sequences to `[A-Za-z*]`, which is pure ASCII, and the Unicode-aware version
/// can change a string's length and can map non-ASCII code points onto valid
/// ASCII letters. Both would let an invalid sequence through.
pub fn normalize_sequence(sequence: &str) -> String {
    sequence.to_ascii_uppercase()
}

/// Sort positions ascending. Duplicates are preserved — rejecting them is
/// [`crate::validation`]'s job, and silently collapsing them would discard the
/// evidence that the document was assembled wrongly.
pub fn sort_positions(mut positions: Vec<i64>) -> Vec<i64> {
    positions.sort_unstable();
    positions
}

/// Sort ranges by start, then by end.
///
/// Overlapping and inverted ranges are preserved; both are rejected by
/// [`crate::validation`] after this sort has run.
pub fn sort_ranges(mut ranges: Vec<[i64; 2]>) -> Vec<[i64; 2]> {
    ranges.sort_unstable_by(|[a_s, a_e], [b_s, b_e]| a_s.cmp(b_s).then_with(|| a_e.cmp(b_e)));
    ranges
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_is_uppercased() {
        assert_eq!(normalize_sequence("maeprq"), "MAEPRQ");
        assert_eq!(normalize_sequence("MaEp*"), "MAEP*");
    }

    #[test]
    fn positions_sort_ascending_and_keep_duplicates() {
        assert_eq!(sort_positions(vec![3, 1, 2]), vec![1, 2, 3]);
        assert_eq!(sort_positions(vec![7, 3, 7]), vec![3, 7, 7]);
        assert_eq!(sort_positions(vec![5, 0, -3]), vec![-3, 0, 5]);
    }

    #[test]
    fn ranges_sort_by_start_then_end() {
        assert_eq!(sort_ranges(vec![[5, 10], [1, 3]]), vec![[1, 3], [5, 10]]);
        assert_eq!(sort_ranges(vec![[1, 9], [1, 3]]), vec![[1, 3], [1, 9]]);
        // Inverted ranges are sorted, not rejected, here.
        assert_eq!(sort_ranges(vec![[12, 8], [3, 3]]), vec![[3, 3], [12, 8]]);
    }
}
