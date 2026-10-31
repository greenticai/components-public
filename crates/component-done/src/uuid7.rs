//! UUID v7 (RFC 9562), built from a millisecond clock and ten random bytes.
//!
//! The clock and the entropy are parameters so the layout is testable and the
//! host decides where they come from. The id is the request's idempotency key:
//! the Core deduplicates on it, so a node that is retried must reuse the id it
//! generated the first time. That is the caller's rule (see `ops::create`); this
//! module only formats.

/// Format `unix_ms` (48 bits used) and `rand` (80 bits used) as a UUID v7.
pub fn format(unix_ms: u64, rand: [u8; 10]) -> String {
    let mut b = [0u8; 16];
    let ms = unix_ms & 0x0000_FFFF_FFFF_FFFF;
    b[0] = (ms >> 40) as u8;
    b[1] = (ms >> 32) as u8;
    b[2] = (ms >> 24) as u8;
    b[3] = (ms >> 16) as u8;
    b[4] = (ms >> 8) as u8;
    b[5] = ms as u8;
    b[6..].copy_from_slice(&rand[..10]);
    b[6] = (b[6] & 0x0F) | 0x70; // version 7
    b[8] = (b[8] & 0x3F) | 0x80; // variant 10
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// True for a lowercase or uppercase 8-4-4-4-12 hex id. Version is not checked:
/// a caller may pass an id generated elsewhere, and the Core is the authority.
pub fn is_well_formed(id: &str) -> bool {
    let parts: Vec<&str> = id.split('-').collect();
    let widths = [8usize, 4, 4, 4, 12];
    parts.len() == 5
        && parts
            .iter()
            .zip(widths)
            .all(|(p, w)| p.len() == w && p.bytes().all(|c| c.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_layout_carries_version_variant_and_time() {
        let id = format(0x0123_4567_89AB, [0xFF; 10]);
        assert_eq!(id.len(), 36);
        assert!(id.starts_with("01234567-89ab-7"), "got {id}");
        // variant nibble is 8, 9, a or b
        assert!(
            matches!(id.as_bytes()[19], b'8' | b'9' | b'a' | b'b'),
            "got {id}"
        );
        assert!(is_well_formed(&id));
    }

    #[test]
    fn ids_sort_by_time() {
        let a = format(1_000, [0; 10]);
        let b = format(2_000, [0; 10]);
        assert!(a < b);
    }

    #[test]
    fn well_formed_rejects_garbage() {
        assert!(!is_well_formed(""));
        assert!(!is_well_formed("not-a-uuid"));
        assert!(!is_well_formed("0123456789ab-7000-8000-0000-000000000000"));
        assert!(!is_well_formed("0123456g-89ab-7000-8000-000000000000"));
    }
}
