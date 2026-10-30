//! Whether an update may be installed (plan 10, P-7). The feed, the packages and the digests it
//! publishes are all in the hands of whoever can publish a release, so the only part of an update
//! they cannot forge is an Ed25519 signature made with a key kept offline, whose public half the
//! build pins. A build that pins no key installs no update.
//!
//! The signature covers a short text naming the channel, version, file name and SHA-256 of the
//! package, which 1.12.1 signed the same way: a signed package cannot be offered as another
//! version, so an older one cannot come back as an update.

use std::fmt;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ring::signature::{ED25519, UnparsedPublicKey};
use sha2::{Digest, Sha256};

/// The `updates` pre-release of this repository, which holds every package, its signature and
/// the feed (plan 0.1).
pub const FEED: &str = "https://github.com/nyabi-gh/CloakFrame/releases/download/updates/";

const PUBLIC_KEY_LEN: usize = 32;
const SIGNATURE_LEN: usize = 64;

/// What the feed declares about one package, all of which the signature covers.
#[derive(Debug, Clone, Copy)]
pub struct Release<'a> {
    pub channel: &'a str,
    pub version: &'a str,
    pub file_name: &'a str,
    pub sha256: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The build pins no update key.
    NoKey,
    NotADigest(String),
    /// A field that is empty or holds a line break, which would make the signed text ambiguous.
    Unsignable(String),
    MalformedKey,
    MalformedSignature,
    /// The signature is not the pinned key's over this release.
    NotSigned,
    /// The package on disk is not the one the signature names.
    PackageDiffers {
        expected: String,
        actual: String,
    },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoKey => f.write_str("this build has no update key, so it installs no update"),
            Self::NotADigest(s) => {
                write!(f, "the feed declared '{s}', which is not a SHA-256 digest")
            }
            Self::Unsignable(s) => write!(f, "the feed declared '{s}', which cannot be signed"),
            Self::MalformedKey => f.write_str("the pinned update key is not an Ed25519 key"),
            Self::MalformedSignature => f.write_str("the update signature is malformed"),
            Self::NotSigned => f.write_str("the update is not signed by the key this build pins"),
            Self::PackageDiffers { expected, actual } => write!(
                f,
                "the downloaded package has SHA-256 {actual}, not the signed {expected}"
            ),
        }
    }
}

impl std::error::Error for Refusal {}

/// The exact bytes the release workflow signs for `release`.
pub fn payload(release: &Release) -> Result<Vec<u8>, Refusal> {
    let digest = normalized_digest(release.sha256)?;
    for field in [release.channel, release.version, release.file_name] {
        if field.is_empty() || field.contains(['\n', '\r']) {
            return Err(Refusal::Unsignable(field.to_owned()));
        }
    }
    Ok(format!(
        "CloakFrame update v2\nchannel: {}\nversion: {}\nfile: {}\nsha256: {digest}\n",
        release.channel, release.version, release.file_name
    )
    .into_bytes())
}

/// Lowercase hex, as the signer writes it; the feed may differ in case and surrounding space.
fn normalized_digest(sha256: &str) -> Result<String, Refusal> {
    let digest = sha256.trim().to_ascii_lowercase();
    if digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(digest)
    } else {
        Err(Refusal::NotADigest(sha256.to_owned()))
    }
}

/// Succeeds only on a positive match; malformed input is a refusal, never a pass.
pub fn verify(payload: &[u8], signature: &str, public_key: &str) -> Result<(), Refusal> {
    let key = decode(public_key, PUBLIC_KEY_LEN).ok_or(Refusal::MalformedKey)?;
    let signature = decode(signature, SIGNATURE_LEN).ok_or(Refusal::MalformedSignature)?;
    if payload.is_empty() {
        return Err(Refusal::NotSigned);
    }
    UnparsedPublicKey::new(&ED25519, key)
        .verify(payload, &signature)
        .map_err(|_| Refusal::NotSigned)
}

fn decode(base64: &str, len: usize) -> Option<Vec<u8>> {
    STANDARD
        .decode(base64.trim())
        .ok()
        .filter(|b| b.len() == len)
}

/// Whether `release` may be installed, given the signature published beside it and the key the
/// build pins.
///
/// Only the declaration is checked, so the package that is installed has to be the one it names:
/// [`check_package`] compares the downloaded file with the signed digest.
pub fn check(release: &Release, signature: &str, pinned_key: Option<&str>) -> Result<(), Refusal> {
    let key = pinned_key.filter(|k| !k.is_empty()).ok_or(Refusal::NoKey)?;
    verify(&payload(release)?, signature, key)
}

/// Whether the file at `path` is the package `release` names.
pub fn check_package(path: &Path, release: &Release) -> Result<(), PackageError> {
    let expected = normalized_digest(release.sha256).map_err(PackageError::Refused)?;
    let actual = sha256_of_file(path).map_err(PackageError::Io)?;
    if actual == expected {
        Ok(())
    } else {
        Err(PackageError::Refused(Refusal::PackageDiffers {
            expected,
            actual,
        }))
    }
}

#[derive(Debug)]
pub enum PackageError {
    Io(io::Error),
    Refused(Refusal),
}

impl fmt::Display for PackageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => e.fmt(f),
            Self::Refused(r) => r.fmt(f),
        }
    }
}

impl std::error::Error for PackageError {}

/// Lowercase hex SHA-256 of a file, read in pieces so a package never has to fit in memory.
pub fn sha256_of_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1 << 20];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TestDir, fixtures};
    use serde_json::Value;

    fn text(value: &Value, key: &str) -> String {
        value[key].as_str().unwrap_or_default().to_owned()
    }

    fn release(call: &Value) -> [String; 4] {
        let r = &call["release"];
        ["channel", "version", "file_name", "sha256_hex"].map(|k| text(r, k))
    }

    fn as_release(fields: &[String; 4]) -> Release<'_> {
        Release {
            channel: &fields[0],
            version: &fields[1],
            file_name: &fields[2],
            sha256: &fields[3],
        }
    }

    /// Replays every call 1.12.1's signature tests recorded. A build without a key, which 1.12.1
    /// called unpinned and let through, refuses (P-7).
    #[test]
    fn the_calls_recorded_from_1_12_1_give_the_same_answers() {
        let dir = fixtures().join("legacy/update_signature_tests");
        let mut replayed = 0;
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let recorded: Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            for call in recorded["calls"].as_array().unwrap() {
                let name = format!("{} {call}", path.display());
                match call["fn"].as_str().unwrap() {
                    "verifyUpdateSignature" => {
                        let payload = STANDARD.decode(text(call, "payload_base64")).unwrap();
                        let result = verify(
                            &payload,
                            &text(call, "signature_base64"),
                            &text(call, "public_key_base64"),
                        );
                        assert_eq!(result.is_ok(), call["result"] == true, "{name}");
                    }
                    "evaluateUpdateTrust" => {
                        let fields = release(call);
                        let key = text(call, "pinned_public_key_base64");
                        let result = check(
                            &as_release(&fields),
                            &text(call, "signature_base64"),
                            Some(key.as_str()),
                        );
                        match call["trust"].as_str().unwrap() {
                            "trusted" => assert_eq!(result, Ok(()), "{name}"),
                            "rejected" => assert!(result.is_err(), "{name}"),
                            "unpinned" => assert_eq!(result, Err(Refusal::NoKey), "{name}"),
                            other => panic!("{other}"),
                        }
                    }
                    "updateSignaturePayload" => {
                        let fields = release(call);
                        let expected = call["payload_base64"]
                            .as_str()
                            .map(|p| STANDARD.decode(p).unwrap());
                        assert_eq!(payload(&as_release(&fields)).ok(), expected, "{name}");
                    }
                    other => panic!("{other} in {name}"),
                }
                replayed += 1;
            }
        }
        assert_eq!(replayed, 30);
    }

    const KEY: &str = "pj5PD2/aCU//cagRcHnlYKJAdW9xSr9q1b2Gm2pzs24=";
    const SIGNATURE: &str =
        "ODzLa/zVr09xJ6rG8Sbn5AoWLSAAolzKuSn0PZsRJYLMma88Xm0gOUXdilSU9aLw8ItUxlRHRxlW2fH+5u8+DA==";
    /// SHA-256 of "abc".
    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn signed() -> Release<'static> {
        Release {
            channel: "win",
            version: "1.12.0",
            file_name: "CloakFrame-1.12.0-full.nupkg",
            sha256: ABC,
        }
    }

    #[test]
    fn the_signed_text_is_the_workflows() {
        assert_eq!(
            String::from_utf8(payload(&signed()).unwrap()).unwrap(),
            format!(
                "CloakFrame update v2\nchannel: win\nversion: 1.12.0\n\
                 file: CloakFrame-1.12.0-full.nupkg\nsha256: {ABC}\n"
            )
        );
    }

    #[test]
    fn a_build_without_a_key_refuses_every_update() {
        assert_eq!(check(&signed(), SIGNATURE, Some(KEY)), Ok(()));
        assert_eq!(check(&signed(), SIGNATURE, None), Err(Refusal::NoKey));
        assert_eq!(check(&signed(), SIGNATURE, Some("")), Err(Refusal::NoKey));
        assert_eq!(check(&signed(), "", None), Err(Refusal::NoKey));
    }

    #[test]
    fn the_package_has_to_be_the_one_signed() {
        let dir = TestDir::new();
        let package = dir.write_bytes("p.nupkg", b"abc");
        assert_eq!(sha256_of_file(&package).unwrap(), ABC);
        assert!(check_package(&package, &signed()).is_ok());

        let other = dir.write_bytes("q.nupkg", b"abd");
        assert!(matches!(
            check_package(&other, &signed()),
            Err(PackageError::Refused(Refusal::PackageDiffers { .. }))
        ));
        assert!(matches!(
            check_package(&dir.path().join("missing"), &signed()),
            Err(PackageError::Io(_))
        ));
    }
}
