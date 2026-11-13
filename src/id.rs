//! Stable entry ids. They are only a hint for "last used", not a security boundary.

/// FNV-1a over several byte slices, with a separator byte between them.
pub fn fnv1a(parts: &[&[u8]]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for part in parts {
        for &b in part.iter().chain(core::iter::once(&0xffu8)) {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_input_same_id() {
        assert_eq!(fnv1a(&[b"disk", b"Fedora"]), fnv1a(&[b"disk", b"Fedora"]));
    }

    #[test]
    fn different_disk_or_name_differs() {
        assert_ne!(fnv1a(&[b"disk1", b"Fedora"]), fnv1a(&[b"disk2", b"Fedora"]));
        assert_ne!(fnv1a(&[b"disk1", b"Fedora"]), fnv1a(&[b"disk1", b"Ubuntu"]));
    }

    #[test]
    fn part_boundaries_matter() {
        assert_ne!(fnv1a(&[b"ab", b"c"]), fnv1a(&[b"a", b"bc"]));
    }
}
