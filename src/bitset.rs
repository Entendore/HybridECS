use core::ops::{BitAnd, BitOr, Sub};

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct BitSet(pub(crate) Vec<u64>);

impl BitSet {
    pub(crate) fn set(&mut self, id: u32) {
        let word = id as usize / 64;
        if self.0.len() <= word { self.0.resize(word + 1, 0); }
        let bit = id as usize % 64;
        self.0[word] |= 1 << bit;
    }

    pub(crate) fn clear(&mut self, id: u32) {
        let word = id as usize / 64;
        if word < self.0.len() {
            let bit = id as usize % 64;
            self.0[word] &= !(1 << bit);
        }
    }

    #[inline]
    pub(crate) fn contains(&self, id: u32) -> bool {
        let word = id as usize / 64;
        let bit = id as usize % 64;
        self.0.get(word).map_or(false, |w| (w >> bit) & 1 == 1)
    }

    #[inline]
    pub(crate) fn iter(&self) -> impl Iterator<Item = u32> + '_ {
        self.0.iter().enumerate().flat_map(|(i, &word)| {
            let mut bits = word;
            let base = i as u32 * 64;
            core::iter::from_fn(move || {
                if bits == 0 { None } else {
                    let bit = bits.trailing_zeros();
                    bits &= bits - 1;
                    Some(base + bit)
                }
            })
        })
    }
}

impl BitAnd for &BitSet {
    type Output = BitSet;
    fn bitand(self, rhs: Self) -> Self::Output {
        let mut result = BitSet::default();
        let min_len = self.0.len().min(rhs.0.len());
        result.0.resize(min_len, 0);
        for i in 0..min_len { result.0[i] = self.0[i] & rhs.0[i]; }
        result
    }
}

impl BitOr for &BitSet {
    type Output = BitSet;
    fn bitor(self, rhs: Self) -> Self::Output {
        let mut result = BitSet::default();
        let max_len = self.0.len().max(rhs.0.len());
        result.0.resize(max_len, 0);
        for i in 0..max_len {
            let a = self.0.get(i).copied().unwrap_or(0);
            let b = rhs.0.get(i).copied().unwrap_or(0);
            result.0[i] = a | b;
        }
        result
    }
}

impl Sub for &BitSet {
    type Output = BitSet;
    fn sub(self, rhs: Self) -> Self::Output {
        let mut result = BitSet::default();
        result.0.resize(self.0.len(), 0);
        for i in 0..self.0.len() {
            let b = rhs.0.get(i).copied().unwrap_or(0);
            result.0[i] = self.0[i] & !b;
        }
        result
    }
}
