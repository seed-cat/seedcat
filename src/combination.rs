use std::cmp::min;
use std::collections::BTreeSet;
use std::fmt::Debug;
use std::fs::File;
use std::io::{BufWriter, Write};

use anyhow::{format_err, Error, Result};
use gzp::deflate::Gzip;
use gzp::par::compress::{ParCompress, ParCompressBuilder};
use gzp::ZWriter;

use crate::logger::Logger;
use crate::permutations::Permutations;

/// Generates combinations of elements in a fast way
#[derive(Debug, Clone)]
pub struct Combinations<T> {
    permute_indices: BTreeSet<usize>,
    elements: Vec<Vec<T>>,
    indices: Vec<usize>,
    next: Vec<T>,
    position: u64,
    combinations: u64,
    permutations: Permutations<usize>,
    length: usize,
    permutation: Vec<usize>,
}

impl<T: Clone + Debug> Combinations<T> {
    pub fn new(elements: Vec<Vec<T>>) -> Self {
        let len = elements.len();
        Self::permute(elements, vec![], len)
    }

    pub fn permute(elements: Vec<Vec<T>>, permute_indices: Vec<usize>, length: usize) -> Self {
        let permute_len = permute_indices.len() - (elements.len() - length);
        let mut permutations = Permutations::new(permute_indices.clone(), permute_len);
        let permute_indices = BTreeSet::from_iter(permute_indices.into_iter());
        let permutation = permutations.next().unwrap_or(&vec![]).clone();

        Self::new_shard(elements, permutations, permute_indices, length, permutation)
    }

    fn new_shard(
        elements: Vec<Vec<T>>,
        permutations: Permutations<usize>,
        permute_indices: BTreeSet<usize>,
        length: usize,
        permutation: Vec<usize>,
    ) -> Self {
        let indices = vec![0; elements.len()];
        Self {
            permute_indices,
            permutations,
            permutation,
            elements,
            indices,
            next: vec![],
            position: 0,
            combinations: 1,
            length,
        }
    }

    pub fn fixed_positions(&self) -> Vec<Option<T>> {
        let mut fixed = vec![];
        for i in 0..self.len() {
            if !self.permute_indices.contains(&i) && self.elements[i].len() == 1 {
                fixed.push(Some(self.elements[i][0].clone()));
            } else {
                fixed.push(None);
            }
        }
        fixed
    }

    pub fn begin(&self) -> Vec<T> {
        let mut vec = vec![];
        for i in 0..self.length {
            vec.push(self.elements[i][0].clone());
        }
        vec
    }

    pub fn end(&self) -> Vec<T> {
        let mut vec = vec![];
        let mut permute = self.permute_indices.clone();
        for i in 0..self.length {
            let mut j = i;
            if self.permute_indices.contains(&i) {
                j = permute.pop_last().unwrap();
            }
            let len = self.elements[j].len();
            vec.push(self.elements[j][len - 1].clone());
        }
        vec
    }

    pub fn elements(&self) -> Vec<Vec<T>> {
        self.elements.clone()
    }

    pub fn total(&self) -> u64 {
        // Runs fast and is usually accurate for large permutations
        self.estimate_total(10_000_000)
    }

    pub fn estimate_total(&self, sample_size: u64) -> u64 {
        let mut total_combo = 1_u64;
        let mut total_perm = 0_u64;
        let mut sizes = vec![];

        for i in 0..self.elements.len() {
            let len = self.elements[i].len() as u64;
            if self.permute_indices.contains(&i) {
                sizes.push(self.elements[i].len() as u64);
            } else {
                total_combo = total_combo.saturating_mul(len);
            }
        }
        if sizes.len() == 0 {
            return total_combo;
        }

        let mut count = 0;
        let mut permutations = Permutations::new(sizes, self.permutation.len());
        let num_permutations = self.permutations() as f64;
        while let Some(next) = permutations.next() {
            count += 1;
            total_perm = total_perm.saturating_add(next.iter().product());
            if count == sample_size {
                total_perm = (total_perm as f64 * (num_permutations / sample_size as f64)) as u64;
                break;
            }
        }

        total_perm.saturating_mul(total_combo)
    }

    pub fn permutations(&self) -> u64 {
        let n = self.permute_indices.len() as u64;
        let r = self.permutation.len() as u64;
        let mut permutations = 1_u64;
        for i in n - r + 1..=n {
            permutations = permutations.saturating_mul(i);
        }
        permutations
    }

    pub fn len(&self) -> usize {
        self.length
    }

    fn next_index_rev(&self, index: &usize, permutation_index: &mut usize) -> usize {
        if self.permute_indices.contains(&index) {
            *permutation_index -= 1;
            return self.permutation[*permutation_index];
        }
        return *index;
    }

    fn combinations(&self) -> u64 {
        let mut permutation_index = self.permutation.len();
        let mut combinations = 1_u64;
        for i in (0..self.length).rev() {
            let j = self.next_index_rev(&i, &mut permutation_index);
            combinations = combinations.saturating_mul(self.elements[j].len() as u64);
        }
        combinations
    }

    fn next_permute(&mut self) {
        if self.position == self.combinations && self.permutations.len() > 1 {
            if let Some(permutation) = self.permutations.next() {
                self.permutation = permutation.clone();
                self.combinations = self.combinations();
                self.position = 0;
                self.indices = vec![0; self.elements.len()];
            }
        }
    }

    pub fn next(&mut self) -> Option<&Vec<T>> {
        if self.position >= self.combinations {
            return None;
        }

        self.position += 1;
        let mut permutation_index = self.permutation.len();

        if self.position == 1 {
            self.next.clear();
            for i in (0..self.length).rev() {
                let j = self.next_index_rev(&i, &mut permutation_index);
                self.next.push(self.elements[j][0].clone());
            }
            self.combinations = self.combinations();
            self.next.reverse();
            self.next_permute();
            return Some(&self.next);
        }

        for i in (0..self.length).rev() {
            let j = self.next_index_rev(&i, &mut permutation_index);

            if self.indices[j] < self.elements[j].len() - 1 {
                self.indices[j] += 1;
                self.next[i] = self.elements[j][self.indices[j]].clone();
                break;
            } else {
                self.indices[j] = 0;
                self.next[i] = self.elements[j][0].clone();
            }
        }
        self.next_permute();
        Some(&self.next)
    }

    // Splits seeds into a minimum number of shards
    pub fn shard(&self, num: usize) -> Vec<Combinations<T>> {
        let mut shards = vec![];

        if self.permutations.len() > 1 {
            let perm_shards = min(num as u64, self.permutations.len()) as usize;
            for mut perm in self.permutations.shard(perm_shards) {
                let permutation = perm.next().unwrap_or(&vec![]).clone();
                shards.push(Self::new_shard(
                    self.elements.clone(),
                    perm,
                    self.permute_indices.clone(),
                    self.length,
                    permutation,
                ));
            }
        } else {
            shards.push(self.clone());
        }

        for i in 0..self.elements.len() {
            if !self.permute_indices.contains(&i) {
                shards = Self::shard_index(shards, i);
                if shards.len() >= num {
                    break;
                }
            }
        }

        shards
    }

    fn shard_index(shards: Vec<Combinations<T>>, index: usize) -> Vec<Combinations<T>> {
        let mut next_shards = vec![];
        for s in shards {
            for choice in &s.elements[index] {
                let mut elements = s.elements.clone();
                elements[index] = vec![choice.clone()];
                let ns = Self::new_shard(
                    elements,
                    s.permutations.clone(),
                    s.permute_indices.clone(),
                    s.length,
                    s.permutation.clone(),
                );
                next_shards.push(ns);
            }
        }
        next_shards
    }
}

impl Combinations<String> {
    /// Write all combinations to a gz in parallel (very fast with multiple CPUs)
    pub async fn write_zip(&mut self, filename: &str, log: &Logger) -> Result<()> {
        let err = format_err!("Failed to create gzip file '{:?}'", filename);
        let file = File::create(filename).map_err(|_| err)?;
        let writer = BufWriter::new(file);
        let logname = format!("Writing Dictionary '{}'", filename);
        let timer = log.time(&logname, self.total()).await;
        let timer_handle = timer.start().await;

        let mut parz: ParCompress<Gzip> = ParCompressBuilder::new().from_writer(writer);
        let mut as_bytes = self.to_bytes();
        while let Some(strs) = as_bytes.next() {
            for str in strs {
                parz.write_all(str).expect("Failed to write");
            }
            parz.write(&[10]).unwrap();
            timer.add(1);
        }

        parz.finish().map_err(Error::msg)?;
        timer_handle.await.expect("Timer failed");
        Ok(())
    }

    fn to_bytes(&self) -> Combinations<&[u8]> {
        let mut vecs = vec![];
        for element in &self.elements {
            let mut vec = vec![];
            for str in element {
                vec.push(str.as_bytes());
            }
            vecs.push(vec);
        }
        Combinations::new(vecs)
    }
}

#[cfg(test)]
mod tests {
    use crate::combination::*;

    fn expand(seeds: Vec<Combinations<u32>>) -> Vec<Vec<u32>> {
        let mut expanded = vec![];
        let mut set = BTreeSet::new();
        for mut seed in seeds {
            while let Some(next) = seed.next() {
                assert_eq!(set.contains(next), false);
                set.insert(next.clone());
                expanded.push(next.clone());
            }
        }
        expanded
    }

    #[test]
    fn can_get_begin_and_end() {
        let combinations = Combinations::permute(
            vec![vec![1], vec![2], vec![3], vec![4]],
            vec![0, 1, 2, 3],
            3,
        );
        assert_eq!(combinations.begin(), vec![1, 2, 3]);
        assert_eq!(combinations.end(), vec![4, 3, 2]);
    }

    #[test]
    fn can_shard() {
        let combinations = Combinations::new(vec![vec![1, 2], vec![3, 4], vec![5, 6], vec![7, 8]]);
        assert_eq!(
            expand(vec![combinations.clone()]),
            expand(combinations.shard(3))
        );

        let combinations = Combinations::new(vec![vec![1, 2], vec![3]]);
        assert_eq!(
            expand(vec![combinations.clone()]),
            expand(combinations.shard(100))
        );

        let combinations = Combinations::permute(
            vec![vec![1, 2], vec![3, 4], vec![5, 6], vec![7, 8]],
            vec![0, 1, 2, 3],
            2,
        );
        let shards = combinations.shard(1000);
        assert_eq!(shards.len(), 12);
        assert_eq!(
            expand(vec![combinations.clone()]).len(),
            expand(shards).len()
        );

        let combinations = Combinations::permute(
            vec![vec![1, 2, 3], vec![4, 5], vec![6], vec![7]],
            vec![1, 2, 3],
            2,
        );
        let shards = combinations.shard(100);
        assert_eq!(shards.len(), 9);
        assert_eq!(expand(vec![combinations.clone()]), expand(shards));
    }

    #[test]
    fn writes_permutations1() {
        let mut combinations =
            Combinations::permute(vec![vec![1, 2], vec![3], vec![4]], vec![0, 1, 2], 2);
        assert_eq!(combinations.next(), Some(&vec![1, 3]));
        assert_eq!(combinations.next(), Some(&vec![2, 3]));
        assert_eq!(combinations.next(), Some(&vec![3, 1]));
        assert_eq!(combinations.next(), Some(&vec![3, 2]));
        assert_eq!(combinations.next(), Some(&vec![1, 4]));
        assert_eq!(combinations.next(), Some(&vec![2, 4]));
        assert_eq!(combinations.next(), Some(&vec![4, 1]));
        assert_eq!(combinations.next(), Some(&vec![4, 2]));
        assert_eq!(combinations.next(), Some(&vec![3, 4]));
        assert_eq!(combinations.next(), Some(&vec![4, 3]));
        assert_eq!(combinations.next(), None);

        let combinations =
            Combinations::permute(vec![vec![1, 2], vec![4, 5], vec![7, 8]], vec![0, 1, 2], 3);
        permute_assert(combinations, 6, 48);

        let combinations = Combinations::permute(
            vec![vec![1, 2, 3], vec![4, 5, 6], vec![7, 8, 9]],
            vec![0, 1, 2],
            2,
        );
        permute_assert(combinations, 6, 54);

        let combinations = Combinations::permute(
            vec![vec![10, 11], vec![1, 2, 3], vec![4, 5, 6], vec![7, 8, 9]],
            vec![1, 2, 3],
            3,
        );
        permute_assert(combinations, 6, 108);

        let combinations = Combinations::permute(
            vec![
                vec![0, 1, 2],
                vec![3, 4],
                vec![5, 6, 7, 8, 9],
                vec![10, 11, 12],
            ],
            vec![0, 1, 2, 3],
            3,
        );
        permute_assert(combinations, 24, 738);
    }

    fn permute_assert(mut combinations: Combinations<usize>, permutations: u64, exact: u64) {
        assert_eq!(combinations.permutations(), permutations);
        assert_eq!(combinations.total(), exact);

        let mut perms = vec![];
        while let Some(next) = combinations.next() {
            assert!(!perms.contains(next));
            perms.push(next.clone());
        }
        assert_eq!(perms.len() as u64, exact);
    }

    #[test]
    fn writes_permutations2() {
        let mut combinations =
            Combinations::permute(vec![vec![1], vec![2], vec![3, 4]], vec![0, 2], 3);
        assert_eq!(combinations.total(), 4);
        assert_eq!(combinations.next(), Some(&vec![1, 2, 3]));
        assert_eq!(combinations.next(), Some(&vec![1, 2, 4]));
        assert_eq!(combinations.next(), Some(&vec![3, 2, 1]));
        assert_eq!(combinations.next(), Some(&vec![4, 2, 1]));
        assert_eq!(combinations.next(), None);
    }

    #[test]
    fn writes_all_combinations1() {
        let mut combinations = Combinations::new(vec![vec![1, 2], vec![3, 4], vec![5, 6, 7]]);
        assert_eq!(combinations.next(), Some(&vec![1, 3, 5]));
        assert_eq!(combinations.next(), Some(&vec![1, 3, 6]));
        assert_eq!(combinations.next(), Some(&vec![1, 3, 7]));
        assert_eq!(combinations.next(), Some(&vec![1, 4, 5]));
        assert_eq!(combinations.next(), Some(&vec![1, 4, 6]));
        assert_eq!(combinations.next(), Some(&vec![1, 4, 7]));
        assert_eq!(combinations.next(), Some(&vec![2, 3, 5]));
        assert_eq!(combinations.next(), Some(&vec![2, 3, 6]));
        assert_eq!(combinations.next(), Some(&vec![2, 3, 7]));
        assert_eq!(combinations.next(), Some(&vec![2, 4, 5]));
        assert_eq!(combinations.next(), Some(&vec![2, 4, 6]));
        assert_eq!(combinations.next(), Some(&vec![2, 4, 7]));
        assert_eq!(combinations.next(), None);
    }

    #[test]
    fn writes_all_combinations2() {
        let mut combinations = Combinations::new(vec![vec![1, 2], vec![3], vec![4, 5]]);
        assert_eq!(combinations.next(), Some(&vec![1, 3, 4]));
        assert_eq!(combinations.next(), Some(&vec![1, 3, 5]));
        assert_eq!(combinations.next(), Some(&vec![2, 3, 4]));
        assert_eq!(combinations.next(), Some(&vec![2, 3, 5]));
        assert_eq!(combinations.next(), None);
    }
}
