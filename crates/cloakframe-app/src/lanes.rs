//! Rows of the timeline: tracks that do not overlap share a row, so a crowd's thousands of tracks
//! take as many rows as are on screen at once (docs/M5_PLAN.md, 2,572 tracks in 3.4 minutes).

use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// The row of each span `first..=last`, in the order given: the lowest row free at its start.
pub fn pack(spans: &[(u64, u64)]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..spans.len()).collect();
    order.sort_by_key(|&i| spans[i]);
    let mut rows = vec![0; spans.len()];
    // The rows in use with the frame each is free from, and the rows freed since.
    let mut busy: BinaryHeap<Reverse<(u64, usize)>> = BinaryHeap::new();
    let mut free: BinaryHeap<Reverse<usize>> = BinaryHeap::new();
    let mut count = 0;
    for i in order {
        let (first, last) = spans[i];
        while let Some(&Reverse((free_from, row))) = busy.peek() {
            if free_from > first {
                break;
            }
            busy.pop();
            free.push(Reverse(row));
        }
        let row = match free.pop() {
            Some(Reverse(row)) => row,
            None => {
                count += 1;
                count - 1
            }
        };
        rows[i] = row;
        busy.push(Reverse((last.saturating_add(1), row)));
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_that_do_not_overlap_share_a_row() {
        assert_eq!(pack(&[]), Vec::<usize>::new());
        assert_eq!(pack(&[(0, 9), (10, 19), (5, 12)]), [0, 0, 1]);
        assert_eq!(pack(&[(0, 9), (9, 19)]), [0, 1], "a shared frame overlaps");
        assert_eq!(pack(&[(20, 30), (0, 5), (3, 25), (6, 8)]), [0, 0, 1, 0]);
        let crowd: Vec<(u64, u64)> = (0..1000).map(|i| (i * 10, i * 10 + 25)).collect();
        let rows = pack(&crowd);
        assert_eq!(rows.iter().max(), Some(&2));
        for i in 0..crowd.len() {
            for j in i + 1..crowd.len() {
                let (a, b) = (crowd[i], crowd[j]);
                if rows[i] == rows[j] {
                    assert!(a.1 < b.0 || b.1 < a.0, "{a:?} and {b:?} share a row");
                }
            }
        }
    }
}
