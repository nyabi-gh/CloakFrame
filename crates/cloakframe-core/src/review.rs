/// SHA-256 of an input, model set, settings or timeline, computed by the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Digest(pub [u8; 32]);

/// What a review was made against. Changing any part invalidates earlier review responses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReviewKey {
    pub input: Digest,
    pub models: Digest,
    pub settings: Digest,
    pub timeline: Digest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReviewRevision {
    key: ReviewKey,
    number: u64,
}

impl ReviewRevision {
    pub fn first(key: ReviewKey) -> Self {
        Self { key, number: 0 }
    }

    #[must_use]
    pub fn next(self) -> Self {
        Self {
            key: self.key,
            number: self.number + 1,
        }
    }

    pub fn key(&self) -> ReviewKey {
        self.key
    }

    pub fn number(&self) -> u64 {
        self.number
    }

    /// A response computed for an older revision, or for different inputs, must not be applied.
    pub fn accepts(&self, response: &ReviewRevision) -> bool {
        self == response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(input: u8) -> ReviewKey {
        ReviewKey {
            input: Digest([input; 32]),
            models: Digest([1; 32]),
            settings: Digest([2; 32]),
            timeline: Digest([3; 32]),
        }
    }

    #[test]
    fn rejects_stale_and_foreign_responses() {
        let first = ReviewRevision::first(key(0));
        let current = first.next();
        assert!(current.accepts(&current));
        assert!(!current.accepts(&first));
        assert!(!current.accepts(&ReviewRevision::first(key(9)).next()));
        assert_eq!(current.number(), 1);
    }
}
