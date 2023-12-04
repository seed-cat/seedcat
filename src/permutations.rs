use std::cmp::min;
use std::collections::BTreeMap;
use std::fmt::Debug;

/***
   Generates PERMUTE(N, K) permutations where you want to select all K! permutations from a
   possible set of length N.  We generate lexicographic permutations so that the work
   can be multi-threaded without locking.

   If you are performing any expensive operations on permutations the multi-threaded code will
   be orders of magnitude faster on multiple cores.

   See algorithms from here:
   https://www.codeproject.com/Articles/1250925/Permutations-Fast-implementations-and-a-new-indexi
*/
#[derive(Debug, Clone)]
pub struct Permutations<T> {
    elements: Vec<T>,
    indices: Vec<T>,
    combination_index: u64,
    permutation_index: u64,
    len: u64,
    k_permutations: u64,
    k: usize,
    index: u64,
}

impl<T: Clone + Ord> Permutations<T> {
    pub fn new(elements: Vec<T>, k: usize) -> Self {
        let n = elements.len();
        Self::new_shard(elements, k, 0, n_permute_k(n, k))
    }

    fn new_shard(elements: Vec<T>, k: usize, index: u64, len: u64) -> Self {
        let k_permutations = n_permute_k(k, k);
        let combination_index = index / k_permutations;
        let permutation_index = index % k_permutations;

        Self {
            elements,
            indices: vec![],
            combination_index,
            permutation_index,
            len,
            k_permutations,
            k,
            index,
        }
    }

    pub fn shard(&self, num: usize) -> Vec<Permutations<T>> {
        let shard_size = self.len / num as u64;
        let mut shards = vec![];
        let mut index = 0;

        while index < self.len {
            let len = min(self.len, index + shard_size);
            shards.push(Self::new_shard(self.elements.clone(), self.k, index, len));
            index += shard_size;
        }
        shards
    }

    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn next(&mut self) -> Option<&Vec<T>> {
        if self.indices.is_empty() {
            self.next_combo();
            return Some(&self.indices);
        }

        self.index += 1;

        if self.index >= self.len {
            return None;
        }

        if self.index < self.len {
            self.next_perm();
        }
        return Some(&self.indices);
    }

    fn next_combo(&mut self) {
        let n = self.elements.len();
        let indices = indexed_combination(self.combination_index, n, self.k);
        let mut combo = vec![];
        for index in indices {
            combo.push(self.elements[index].clone());
        }
        self.indices = indexed_permutation(self.permutation_index, combo);
    }

    fn next_perm(&mut self) {
        if self.permutation_index == self.k_permutations - 1 {
            self.combination_index += 1;
            self.permutation_index = 0;
            self.next_combo();
        } else {
            self.permutation_index += 1;
            next_permutation(&mut self.indices);
        }
    }
}

/// Precomputed factorial counts
const FACTORIAL: [u64; 21] = [
    1,
    1,
    2,
    6,
    24,
    120,
    720,
    5040,
    40320,
    362880,
    3628800,
    39916800,
    479001600,
    6227020800,
    87178291200,
    1307674368000,
    20922789888000,
    355687428096000,
    6402373705728000,
    121645100408832000,
    2432902008176640000,
];

/// Fast generation of the next lexicographical permutation
fn next_permutation<T: Ord>(list: &mut Vec<T>) -> bool {
    let mut largest_index = usize::MAX;

    for i in (0..list.len() - 1).rev() {
        if list[i] < list[i + 1] {
            largest_index = i;
            break;
        }
    }

    if largest_index == usize::MAX {
        return false;
    }

    let mut largest_index2 = usize::MAX;
    for i in (0..list.len()).rev() {
        if list[largest_index] < list[i] {
            largest_index2 = i;
            break;
        }
    }

    list.swap(largest_index, largest_index2);

    let mut i = largest_index + 1;
    let mut j = list.len() - 1;
    while i < j {
        list.swap(i, j);
        i += 1;
        j -= 1;
    }

    return true;
}

fn n_permute_k(n: usize, k: usize) -> u64 {
    let mut end = 1_u64;
    for i in n - k + 1..=n {
        end = end.saturating_mul(i as u64);
    }
    end
}
/// Returns N choose K
fn n_choose_k(n: usize, k: usize) -> u64 {
    if k > n {
        0
    } else if n <= 20 {
        // Optimization when fitting into u64
        FACTORIAL[n] / FACTORIAL[k] / FACTORIAL[n - k]
    } else {
        let end = k.min(n - k) as u64;
        (1..=end).fold(1, |acc, val| acc * (n as u64 - val + 1) / val)
    }
}

/// Returns an indexed combination from k choose 0..n
/// 0 <= i < ncr(n, k)
fn indexed_combination(i: u64, n: usize, k: usize) -> Vec<usize> {
    assert!(n >= k);
    assert!(i < n_choose_k(n, k));
    let mut combo = vec![];
    let mut r = i + 1;
    let mut j = 0;
    for s in 1..k + 1 {
        let mut cs = j + 1;

        while r > n_choose_k(n - cs, k - s) {
            r -= n_choose_k(n - cs, k - s);
            cs += 1;
        }
        combo.push(cs - 1);
        j = cs;
    }
    combo
}

/// Slow generation of a lexicographical permutation at a given index
fn indexed_permutation<T: Ord>(index: u64, mut list: Vec<T>) -> Vec<T> {
    let size = list.len();
    assert!(index < FACTORIAL[size]);
    list.sort();

    let mut used = vec![false; size];
    let mut lower = FACTORIAL[size];
    let mut result_indices = BTreeMap::new();

    for i in 0..size {
        let bigger = lower;
        lower = FACTORIAL[size - i - 1];
        let mut counter = (index % bigger / lower) as isize;
        let mut result_index = 0;
        'outer: loop {
            if !used[result_index] {
                counter -= 1;
                if counter < 0 {
                    break 'outer;
                }
            }
            result_index += 1;
        }
        result_indices.insert(result_index, i);
        used[result_index] = true;
    }

    let mut result = BTreeMap::new();
    for (index, element) in list.drain(..).enumerate() {
        result.insert(result_indices[&index], element);
    }
    result.into_iter().map(|(_, element)| element).collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use crate::permutations::*;

    #[test]
    fn test_valid_shards() {
        let permutations = Permutations::new(vec![1, 2, 3], 2);
        let shards = assert_explode(permutations.shard(2));
        assert_eq!(assert_explode(vec![permutations]), shards);

        let permutations = Permutations::new(vec![1, 2, 3], 2);
        let shards = assert_explode(permutations.shard(6));
        assert_eq!(assert_explode(vec![permutations]), shards);

        let permutations = Permutations::new(vec![1, 2, 3, 4, 5, 6, 7, 8, 9], 5);
        let shards = assert_explode(permutations.shard(10));
        assert_eq!(assert_explode(vec![permutations]), shards);
    }

    fn assert_explode(permutations: Vec<Permutations<usize>>) -> Vec<Vec<usize>> {
        let mut all = vec![];
        let mut set = HashSet::new();
        for mut permutation in permutations {
            while let Some(next) = permutation.next() {
                assert_eq!(set.contains(next), false);
                set.insert(next.clone());
                all.push(next.clone());
            }
        }
        all
    }

    #[test]
    fn test_permutations_of_k() {
        let mut perm = Permutations::new(vec![1, 2, 3], 2);
        assert_eq!(perm.next(), Some(&vec![1, 2]));
        assert_eq!(perm.next(), Some(&vec![2, 1]));
        assert_eq!(perm.next(), Some(&vec![1, 3]));
        assert_eq!(perm.next(), Some(&vec![3, 1]));
        assert_eq!(perm.next(), Some(&vec![2, 3]));
        assert_eq!(perm.next(), Some(&vec![3, 2]));
        assert_eq!(perm.next(), None);
    }

    #[test]
    fn test_indexed_combo() {
        assert_eq!(indexed_combination(0, 4, 2), vec![0, 1]);
        assert_eq!(indexed_combination(1, 4, 2), vec![0, 2]);
        assert_eq!(indexed_combination(2, 4, 2), vec![0, 3]);
        assert_eq!(indexed_combination(3, 4, 2), vec![1, 2]);
        assert_eq!(indexed_combination(4, 4, 2), vec![1, 3]);
        assert_eq!(indexed_combination(5, 4, 2), vec![2, 3]);
        assert_eq!(
            indexed_combination(173103094564, 100, 10),
            vec![0, 2, 4, 10, 18, 24, 37, 65, 79, 82]
        );
    }

    #[test]
    fn test_counts() {
        assert_eq!(n_choose_k(10, 5), 252);
        assert_eq!(n_choose_k(24, 10), 1961256);
        assert_eq!(n_choose_k(30, 20), 30045015);
        assert_eq!(n_permute_k(10, 5), 30240);
        assert_eq!(n_permute_k(24, 10), 7117005772800);
    }

    #[test]
    fn test_indexed_permutation() {
        assert_eq!(indexed_permutation(0, vec![1, 2, 3]), vec![1, 2, 3]);
        assert_eq!(indexed_permutation(1, vec![1, 2, 3]), vec![1, 3, 2]);
        assert_eq!(indexed_permutation(2, vec![1, 2, 3]), vec![2, 1, 3]);
        assert_eq!(indexed_permutation(3, vec![1, 2, 3]), vec![2, 3, 1]);
        assert_eq!(indexed_permutation(4, vec![1, 2, 3]), vec![3, 1, 2]);
        assert_eq!(indexed_permutation(5, vec![1, 2, 3]), vec![3, 2, 1]);
    }

    #[test]
    fn test_next_permutation() {
        let mut list = vec![1, 2, 3];
        assert_eq!(next_permutation(&mut list), true);
        assert_eq!(list, vec![1, 3, 2]);
        assert_eq!(next_permutation(&mut list), true);
        assert_eq!(list, vec![2, 1, 3]);
        assert_eq!(next_permutation(&mut list), true);
        assert_eq!(list, vec![2, 3, 1]);
        assert_eq!(next_permutation(&mut list), true);
        assert_eq!(list, vec![3, 1, 2]);
        assert_eq!(next_permutation(&mut list), true);
        assert_eq!(list, vec![3, 2, 1]);
        assert_eq!(next_permutation(&mut list), false);
    }
}
