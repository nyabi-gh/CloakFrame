//! Maximum-weight assignment of tracks to detections (Hungarian method). 1.12.1 took the pairs
//! greedily by IoU, which can give a track the detection its neighbour needed when two subjects
//! cross; the plan (4.4) asks for an optimal assignment.

/// `weights[row][col]` is `Some` for a pair that may be matched. Returns the matched
/// `(row, col)` pairs that give the largest total weight, rows ascending.
pub(super) fn max_weight_matching(weights: &[Vec<Option<f32>>]) -> Vec<(usize, usize)> {
    let rows = weights.len();
    let cols = weights.first().map_or(0, Vec::len);
    // Most tracks overlap one detection at most; solving each connected group on its own keeps a
    // crowded frame from becoming one large cubic problem.
    let mut parent: Vec<usize> = (0..rows + cols).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for (r, row) in weights.iter().enumerate() {
        for (c, w) in row.iter().enumerate() {
            if w.is_some() {
                let (a, b) = (root(&mut parent, r), root(&mut parent, rows + c));
                parent[a.max(b)] = a.min(b);
            }
        }
    }
    let mut groups: Vec<(Vec<usize>, Vec<usize>)> = Vec::new();
    let mut group_of = vec![usize::MAX; rows + cols];
    for node in 0..rows + cols {
        let r = root(&mut parent, node);
        if group_of[r] == usize::MAX {
            group_of[r] = groups.len();
            groups.push((Vec::new(), Vec::new()));
        }
        let g = &mut groups[group_of[r]];
        if node < rows {
            g.0.push(node);
        } else {
            g.1.push(node - rows);
        }
    }
    let mut matches = Vec::new();
    for (group_rows, group_cols) in groups {
        if group_rows.is_empty() || group_cols.is_empty() {
            continue;
        }
        let transposed = group_rows.len() > group_cols.len();
        let (a, b) = if transposed {
            (&group_cols, &group_rows)
        } else {
            (&group_rows, &group_cols)
        };
        let weight = |i: usize, j: usize| {
            let (r, c) = if transposed {
                (b[j], a[i])
            } else {
                (a[i], b[j])
            };
            weights[r][c]
        };
        let cost: Vec<Vec<f64>> = (0..a.len())
            .map(|i| {
                (0..b.len())
                    .map(|j| -f64::from(weight(i, j).unwrap_or(0.0)))
                    .collect()
            })
            .collect();
        for (i, j) in min_cost_assignment(&cost).into_iter().enumerate() {
            if weight(i, j).is_some() {
                matches.push(if transposed {
                    (b[j], a[i])
                } else {
                    (a[i], b[j])
                });
            }
        }
    }
    matches.sort_unstable();
    matches
}

/// For `n <= m` rows and columns, the column of each row in an assignment of least total cost
/// (the potentials method, O(n²m)).
fn min_cost_assignment(cost: &[Vec<f64>]) -> Vec<usize> {
    let n = cost.len();
    let m = cost[0].len();
    debug_assert!(n <= m);
    let mut u = vec![0.0f64; n + 1];
    let mut v = vec![0.0f64; m + 1];
    // p[j]: the row (1-based) holding column j; way[j]: the column before j on the path.
    let mut p = vec![0usize; m + 1];
    let mut way = vec![0usize; m + 1];
    for i in 1..=n {
        p[0] = i;
        let mut j0 = 0;
        let mut minv = vec![f64::INFINITY; m + 1];
        let mut used = vec![false; m + 1];
        loop {
            used[j0] = true;
            let i0 = p[j0];
            let mut delta = f64::INFINITY;
            let mut j1 = 0;
            for j in 1..=m {
                if !used[j] {
                    let cur = cost[i0 - 1][j - 1] - u[i0] - v[j];
                    if cur < minv[j] {
                        minv[j] = cur;
                        way[j] = j0;
                    }
                    if minv[j] < delta {
                        delta = minv[j];
                        j1 = j;
                    }
                }
            }
            for j in 0..=m {
                if used[j] {
                    u[p[j]] += delta;
                    v[j] -= delta;
                } else {
                    minv[j] -= delta;
                }
            }
            j0 = j1;
            if p[j0] == 0 {
                break;
            }
        }
        loop {
            let j1 = way[j0];
            p[j0] = p[j1];
            j0 = j1;
            if j0 == 0 {
                break;
            }
        }
    }
    let mut column = vec![0usize; n];
    for j in 1..=m {
        if p[j] != 0 {
            column[p[j] - 1] = j - 1;
        }
    }
    column
}

#[cfg(test)]
mod tests {
    use super::*;

    fn total(weights: &[Vec<Option<f32>>], matches: &[(usize, usize)]) -> f32 {
        matches.iter().map(|&(r, c)| weights[r][c].unwrap()).sum()
    }

    /// Every assignment of `rows` rows to distinct columns or none, for checking small cases.
    fn best_by_search(weights: &[Vec<Option<f32>>]) -> f32 {
        fn go(weights: &[Vec<Option<f32>>], r: usize, used: &mut Vec<bool>) -> f32 {
            if r == weights.len() {
                return 0.0;
            }
            let mut best = go(weights, r + 1, used);
            for c in 0..used.len() {
                if let (false, Some(w)) = (used[c], weights[r][c]) {
                    used[c] = true;
                    best = best.max(w + go(weights, r + 1, used));
                    used[c] = false;
                }
            }
            best
        }
        let cols = weights.first().map_or(0, Vec::len);
        go(weights, 0, &mut vec![false; cols])
    }

    #[test]
    fn the_total_beats_taking_the_best_pair_first() {
        // Greedy takes (0, 0) at 0.9 and leaves row 1 nothing; the best total is 0.8 + 0.7.
        let w = vec![vec![Some(0.9), Some(0.8)], vec![Some(0.7), None]];
        let m = max_weight_matching(&w);
        assert_eq!(m, [(0, 1), (1, 0)]);
    }

    #[test]
    fn forbidden_pairs_are_never_matched() {
        let w = vec![
            vec![None, None],
            vec![None, Some(0.4)],
            vec![Some(0.5), None],
        ];
        assert_eq!(max_weight_matching(&w), [(1, 1), (2, 0)]);
        assert!(max_weight_matching(&[vec![None; 3]]).is_empty());
        assert!(max_weight_matching(&[]).is_empty());
    }

    #[test]
    fn matches_an_exhaustive_search() {
        // A fixed pseudo-random sequence of small rectangular problems.
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..500 {
            let rows = (next() % 5) as usize;
            let cols = (next() % 5) as usize + 1;
            let w: Vec<Vec<Option<f32>>> = (0..rows)
                .map(|_| {
                    (0..cols)
                        .map(|_| (next() % 3 != 0).then(|| (next() % 100) as f32 / 100.0 + 0.3))
                        .collect()
                })
                .collect();
            let m = max_weight_matching(&w);
            let mut seen_r = vec![false; rows];
            let mut seen_c = vec![false; cols];
            for &(r, c) in &m {
                assert!(!seen_r[r] && !seen_c[c] && w[r][c].is_some());
                (seen_r[r], seen_c[c]) = (true, true);
            }
            assert!((total(&w, &m) - best_by_search(&w)).abs() < 1e-4, "{w:?}");
        }
    }
}
