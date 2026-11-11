#![allow(clippy::manual_range_contains)]

use std::{convert::TryFrom, fmt};

#[cfg(feature = "serde")]
use serde::{Deserialize, Deserializer, Serialize};

// ----------------------------------------------------------------------------

/// Is the given string a non-empty snake_case string?
/// In particular, does it match  ^[_a-z][_a-z0-9]*$  ?
pub const fn is_snake_case(string: &str) -> bool {
    // we only care about ascii chars, which fit in a byte.
    // iterating over utf8 continuation bytes and the like will not count as valid snake case anyway.
    //
    // Performance notes:
    // * bytes are checked in fixed-size chunks with no early exit inside a chunk,
    //   so LLVM turns each chunk check into a few SIMD instructions.
    // * lengths that don't fit the chunks exactly are covered by overlapping chunks,
    //   so there is no byte-by-byte tail loop for anything over 3 bytes.
    // * slice patterns and `[u8; N]` chunks instead of slice indexing: no bounds checks, no panics.
    const fn is_snake_case_character(c: u8) -> bool {
        // branchless on purpose, see above.
        (c.wrapping_sub(b'a') < 26) | (c.wrapping_sub(b'0') < 10) | (c == b'_')
    }
    const fn all_snake_case_characters<const N: usize>(chunk: &[u8; N]) -> bool {
        let mut all_valid = true;
        let mut i = 0;
        while i < N {
            all_valid &= is_snake_case_character(chunk[i]);
            i += 1;
        }
        all_valid
    }

    let bytes = string.as_bytes();
    // Every byte gets checked with is_snake_case_character() below, so the only extra rule for the
    // first byte is "not a digit". It's checked in each branch, combined with the result: checking it
    // once up front, or returning early on it, lets LLVM reuse that early load of byte 0 and build
    // the SIMD chunks out of small pieces instead of loading them whole.
    if let Some(last) = bytes.last_chunk::<16>() {
        // 16+ bytes: 16-byte chunks, then the last 16 bytes for whatever is left over.
        let valid_start = !bytes[0].is_ascii_digit();
        let mut rest = bytes;
        while let Some((chunk, tail)) = rest.split_first_chunk::<16>() {
            if !all_snake_case_characters(chunk) {
                return false;
            }
            rest = tail;
        }
        valid_start & all_snake_case_characters(last)
    } else if let (Some(first), Some(last)) = (bytes.first_chunk::<8>(), bytes.last_chunk::<8>()) {
        // 8..=15 bytes: the first 8 and the last 8, which may overlap.
        !first[0].is_ascii_digit()
            & all_snake_case_characters(first)
            & all_snake_case_characters(last)
    } else if let (Some(first), Some(last)) = (bytes.first_chunk::<4>(), bytes.last_chunk::<4>()) {
        // 4..=7 bytes: the first 4 and the last 4, which may overlap.
        !first[0].is_ascii_digit()
            & all_snake_case_characters(first)
            & all_snake_case_characters(last)
    } else {
        // 1..=3 bytes: one at a time.
        let [first, ..] = bytes else { return false };
        let mut all_valid = !first.is_ascii_digit();
        let mut rest = bytes;
        while let [c, tail @ ..] = rest {
            all_valid &= is_snake_case_character(*c);
            rest = tail;
        }
        all_valid
    }
}

// ----------------------------------------------------------------------------

/// Only one possible error: the given string was not valid snake_case.
#[derive(Clone, Debug)]
pub struct InvalidSnakeCase;

// ----------------------------------------------------------------------------

/// An owning string type that can only contain valid snake_case.
/// In other words, it always matches  ^[_a-z][_a-z0-9]*$
/// * Non-empty
/// * Starts with a lower case ASCII letter or underscore
/// * Contains only lower case ASCII letters, underscores and digits
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct SnakeCase(String);

impl SnakeCase {
    pub fn try_from_str(s: &str) -> Result<SnakeCase, InvalidSnakeCase> {
        if is_snake_case(s) {
            Ok(SnakeCase(s.to_string()))
        } else {
            Err(InvalidSnakeCase)
        }
    }

    pub fn try_from_string(s: String) -> Result<SnakeCase, InvalidSnakeCase> {
        if is_snake_case(&s) {
            Ok(SnakeCase(s))
        } else {
            Err(InvalidSnakeCase)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn as_ref(&self) -> SnakeCaseRef<'_> {
        SnakeCaseRef(&self.0)
    }
}

impl TryFrom<&str> for SnakeCase {
    type Error = InvalidSnakeCase;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        SnakeCase::try_from_str(s)
    }
}

impl TryFrom<String> for SnakeCase {
    type Error = InvalidSnakeCase;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        SnakeCase::try_from_string(s)
    }
}

impl std::borrow::Borrow<str> for SnakeCase {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SnakeCase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_str().fmt(f)
    }
}

impl fmt::Display for SnakeCase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_str().fmt(f)
    }
}

#[cfg(feature = "serde")]
impl<'de> Deserialize<'de> for SnakeCase {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let string = String::deserialize(deserializer)?;
        SnakeCase::try_from_str(&string).map_err(|_: InvalidSnakeCase| {
            serde::de::Error::custom(format!("Expected snake_case, got '{}'", string))
        })
    }
}

impl std::cmp::PartialEq<SnakeCase> for &str {
    fn eq(&self, other: &SnakeCase) -> bool {
        *self == other.as_str()
    }
}

impl std::cmp::PartialEq<str> for SnakeCase {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl std::cmp::PartialEq<&str> for SnakeCase {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl std::cmp::PartialEq<String> for SnakeCase {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == *other
    }
}

// ----------------------------------------------------------------------------

/// An non-owning string type that can only refer to string containing valid snake_case.
/// In other words, it always matches  ^[_a-z][_a-z0-9]*$
/// * Non-empty
/// * Starts with a lower case ASCII letter or underscore
/// * Contains only lower case ASCII letters, underscores and digits
#[derive(Copy, Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct SnakeCaseRef<'a>(&'a str);

impl<'a> SnakeCaseRef<'a> {
    pub const fn try_from_str(s: &str) -> Result<SnakeCaseRef<'_>, InvalidSnakeCase> {
        if is_snake_case(s) {
            Ok(SnakeCaseRef(s))
        } else {
            Err(InvalidSnakeCase)
        }
    }

    pub const fn as_str(&self) -> &'a str {
        self.0
    }

    pub fn to_owned(&self) -> SnakeCase {
        SnakeCase(self.0.to_string())
    }
}

#[cfg(feature = "const_literals")]
/// an unsafe constructor for SnakeCaseRef.
///
/// # Safety
/// The caller has to make sure the input is in fact valid snake_case (see [`is_snake_case`]).
pub const unsafe fn from_str_unchecked(s: &str) -> SnakeCaseRef<'_> {
    SnakeCaseRef(s)
}
#[cfg(feature = "const_literals")]
/// this will construct a SnakeCafeRef<'static> with compile-time validation for string literals.
///
/// ```
/// use snake_case::snake_case_lit;
/// let snake_case = snake_case_lit!("my_little_snake");
/// // let bad_snake =  snake_case_lit!("Python"); <- this wont compile
/// ```
#[macro_export]
macro_rules! snake_case_lit {
    ($s:expr) => {{
        struct Valid<const B: bool>;
        let _valid: Valid<true> = Valid::<{ snake_case::is_snake_case($s) }>;
        unsafe {
            // this is perfectly safe, wouldnt even compile otherwise.
            snake_case::from_str_unchecked($s)
        }
    }};
}

impl<'a> TryFrom<&'a str> for SnakeCaseRef<'a> {
    type Error = InvalidSnakeCase;

    fn try_from(s: &'a str) -> Result<Self, Self::Error> {
        SnakeCaseRef::try_from_str(s)
    }
}

impl std::borrow::Borrow<str> for SnakeCaseRef<'_> {
    fn borrow(&self) -> &str {
        self.0
    }
}

impl fmt::Debug for SnakeCaseRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_str().fmt(f)
    }
}

impl fmt::Display for SnakeCaseRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_str().fmt(f)
    }
}

impl std::cmp::PartialEq<SnakeCaseRef<'_>> for str {
    fn eq(&self, other: &SnakeCaseRef<'_>) -> bool {
        self == other.0
    }
}

impl std::cmp::PartialEq<SnakeCaseRef<'_>> for &str {
    fn eq(&self, other: &SnakeCaseRef<'_>) -> bool {
        *self == other.0
    }
}

impl std::cmp::PartialEq<str> for SnakeCaseRef<'_> {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl std::cmp::PartialEq<&str> for SnakeCaseRef<'_> {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl std::cmp::PartialEq<String> for SnakeCaseRef<'_> {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == *other
    }
}

// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snake_case() {
        assert_eq!(SnakeCase::try_from_str("_hello42").unwrap(), "_hello42");
        assert_eq!(
            SnakeCase::try_from_str("_hello42").unwrap(),
            "_hello42".to_string()
        );
        assert_eq!("_hello42", SnakeCase::try_from_str("_hello42").unwrap());
        assert!(SnakeCase::try_from_str("").is_err());
        assert!(SnakeCase::try_from_str("42").is_err());
        assert!(SnakeCase::try_from_str("_").is_ok());
        assert!(SnakeCase::try_from_str("aB").is_err());
        assert!(SnakeCase::try_from_str("foo!").is_err());
    }

    #[test]
    fn is_snake_case_every_length_and_position() {
        // covers each chunk size and every way the overlapping chunks line up.
        for len in 1..80 {
            let valid: String = "abc_def_0123456789_".chars().cycle().take(len).collect();
            assert!(is_snake_case(&valid), "{:?}", valid);
            for pos in 0..len {
                let mut invalid = valid.clone().into_bytes();
                invalid[pos] = b'X';
                let invalid = String::from_utf8(invalid).unwrap();
                assert!(!is_snake_case(&invalid), "{:?}", invalid);
            }
            let leading_digit = format!("1{}", &valid[1..]);
            assert!(!is_snake_case(&leading_digit), "{:?}", leading_digit);
        }
    }

    #[test]
    fn snake_case_ref() {
        assert_eq!(SnakeCaseRef::try_from_str("_hello42").unwrap(), "_hello42");
        assert_eq!(
            SnakeCaseRef::try_from_str("_hello42").unwrap(),
            "_hello42".to_string()
        );
        assert_eq!("_hello42", SnakeCaseRef::try_from_str("_hello42").unwrap());
        assert!(SnakeCaseRef::try_from_str("").is_err());
        assert!(SnakeCaseRef::try_from_str("42").is_err());
        assert!(SnakeCaseRef::try_from_str("_").is_ok());
    }

    #[test]
    fn snake_case_conversions() {
        let sc = SnakeCase::try_from_str("hello_world").unwrap();
        let scr: SnakeCaseRef = sc.as_ref();
        assert_eq!(scr, "hello_world");
        let sc2: SnakeCase = scr.to_owned();
        assert_eq!(sc2, "hello_world");

        use std::collections::HashSet;
        let mut set: HashSet<SnakeCase> = HashSet::new();
        set.insert(SnakeCase::try_from_str("hello_world").unwrap());
        assert!(set.contains(SnakeCaseRef::try_from_str("hello_world").unwrap().as_str()));
    }
}
