use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// SHA-256 of an input, model set, settings or timeline, computed by the engine. Stored as
/// lowercase hex.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Digest(pub [u8; 32]);

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|b| write!(f, "{b:02x}"))
    }
}

impl fmt::Debug for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Digest({self})")
    }
}

impl std::str::FromStr for Digest {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        const INVALID: &str = "expected 64 hex digits";
        if s.len() != 64 || !s.is_ascii() {
            return Err(INVALID);
        }
        let mut bytes = [0; 32];
        for (byte, pair) in bytes.iter_mut().zip(s.as_bytes().chunks(2)) {
            let pair = std::str::from_utf8(pair).map_err(|_| INVALID)?;
            *byte = u8::from_str_radix(pair, 16).map_err(|_| INVALID)?;
        }
        Ok(Self(bytes))
    }
}

impl Serialize for Digest {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Digest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// What a review was made against. Changing any part invalidates earlier review responses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ReviewKey {
    pub input: Digest,
    pub models: Digest,
    pub settings: Digest,
    pub timeline: Digest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
