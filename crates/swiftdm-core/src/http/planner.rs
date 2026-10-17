//! Segment planning and dynamic splitting.
use crate::models::Segment;

pub const MIB: u64 = 1024 * 1024;
/// Smaller files use a single connection.
pub const SEGMENT_THRESHOLD: u64 = 4 * MIB;
pub const MIN_SEGMENT: u64 = MIB;
pub const SPLIT_MIN_REMAINING: u64 = 2 * MIB;

pub fn plan(total: Option<u64>, ranges: bool, max_segments: u32) -> Vec<Segment> {
    match total {
        Some(0) => vec![],
        None => vec![Segment::new(0, 0, -1)],
        Some(t) if !ranges || t < SEGMENT_THRESHOLD || max_segments <= 1 => vec![Segment::new(0, 0, t as i64 - 1)],
        Some(t) => {
            let n = (max_segments as u64).min(t.div_ceil(MIN_SEGMENT)).max(1);
            let size = t / n;
            let mut segs = Vec::with_capacity(n as usize);
            let mut start = 0u64;
            for i in 0..n {
                let end = if i == n - 1 { t - 1 } else { start + size - 1 };
                segs.push(Segment::new(i as u32, start, end as i64));
                start = end + 1;
            }
            segs
        }
    }
}

/// Last byte the running segment should keep, or None if not worth splitting.
pub fn split_point(pos: u64, end: u64) -> Option<u64> {
    if end < pos {
        return None;
    }
    let remaining = end - pos + 1;
    if remaining < SPLIT_MIN_REMAINING {
        return None;
    }
    let keep = remaining / 2;
    (keep >= MIN_SEGMENT / 2).then(|| pos + keep - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plan_tiles_exactly() {
        for total in [1u64, 4 * MIB - 1, 4 * MIB, 10 * MIB + 7, 1 << 32] {
            for n in [1u32, 3, 8, 16, 32] {
                let s = plan(Some(total), true, n);
                assert_eq!(s[0].start, 0);
                assert_eq!(s.last().unwrap().end, total as i64 - 1);
                for w in s.windows(2) {
                    assert_eq!(w[0].end + 1, w[1].start as i64);
                }
            }
        }
        assert!(plan(Some(0), true, 8).is_empty());
        assert_eq!(plan(None, false, 8), vec![Segment::new(0, 0, -1)]);
        assert_eq!(plan(Some(100 * MIB), false, 8).len(), 1);
    }
    #[test]
    fn split_bounds() {
        assert_eq!(split_point(0, MIB), None);
        let k = split_point(1000, 1000 + 10 * MIB - 1).unwrap();
        assert!(k > 1000 && k < 1000 + 10 * MIB - 1);
    }
}
