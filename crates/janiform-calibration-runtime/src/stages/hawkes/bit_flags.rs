const BITS_PER_WORD: usize = u64::BITS as usize;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct HawkesBitFlags {
    words: Vec<u64>,
}

impl HawkesBitFlags {
    pub(super) fn new() -> Self {
        Self { words: Vec::new() }
    }

    pub(super) fn push(&mut self, bit_position: usize, value: bool) {
        let word_position = bit_position / BITS_PER_WORD;
        if self.words.len() <= word_position {
            self.words.resize(word_position.saturating_add(1), 0);
        }

        if value {
            self.words[word_position] |= 1u64 << (bit_position % BITS_PER_WORD);
        }
    }

    pub(super) fn is_set(&self, bit_position: usize) -> bool {
        let word_position = bit_position / BITS_PER_WORD;

        self.words
            .get(word_position)
            .map(|word| (word & (1u64 << (bit_position % BITS_PER_WORD))) != 0)
            .unwrap_or(false)
    }
}
