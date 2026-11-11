//! Standalone x86 A/B bench for is_snake_case. Build: rustc -O --edition 2024 x86_bench.rs
use std::hint::black_box; use std::time::Instant;

// master (127e5f7) with the last-byte bug fixed
mod master {
pub const fn is_snake_case(string: &str) -> bool {
    // we only care about ascii chars, which fit in a byte.
    // iterating over utf8 continuation bytes and the like will not count as valid snake case anyway.
    let (len, bytes) = (string.len(), string.as_bytes());
    const fn valid_start(b: u8) -> bool {
        b == b'_' || b'a' <= b && b <= b'z'
    }
    const fn is_snake_case_character(c: u8) -> bool {
        b'a' <= c && c <= b'z' || b'0' <= c && c <= b'9' || c == b'_'
    }
    // non-empty and starts with a..z or _
    if bytes.is_empty() || !valid_start(bytes[0]) {
        return false;
    }
    //check the rest
    let mut i = 1; // we already checked the first byte, its fine
    loop {
        if i >= len {
            break true;
        }
        if !is_snake_case_character(bytes[i]) {
            break false;
        }
        i += 1;
    }
}
}

// simd-check-and-deref (final, readable)
mod current {
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
    // One loop over both chunks, so LLVM checks them as a single SIMD chunk
    // (two separate checks compile to much worse code on x86).
    const fn both_snake_case_characters<const N: usize>(a: &[u8; N], b: &[u8; N]) -> bool {
        let mut all_valid = true;
        let mut i = 0;
        while i < N {
            all_valid &= is_snake_case_character(a[i]) & is_snake_case_character(b[i]);
            i += 1;
        }
        all_valid
    }

    let bytes = string.as_bytes();
    // Every byte gets checked with is_snake_case_character() below, so the only extra rule for the
    // first byte is "not a digit". It's combined with the result rather than returned early,
    // which would let LLVM drop byte 0 from the SIMD check and split up the loads.
    let valid_start = match bytes {
        [first, ..] => !first.is_ascii_digit(),
        [] => return false,
    };
    if let Some(last) = bytes.last_chunk::<16>() {
        // 16+ bytes: 16-byte chunks, then the last 16 bytes for whatever is left over.
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
        valid_start & both_snake_case_characters(first, last)
    } else if let (Some(first), Some(last)) = (bytes.first_chunk::<4>(), bytes.last_chunk::<4>()) {
        // 4..=7 bytes: the first 4 and the last 4, which may overlap.
        valid_start & both_snake_case_characters(first, last)
    } else {
        // 1..=3 bytes: one at a time.
        let mut all_valid = true;
        let mut rest = bytes;
        while let [c, tail @ ..] = rest {
            all_valid &= is_snake_case_character(*c);
            rest = tail;
        }
        valid_start & all_valid
    }
}
}

// overlapping-chunks variant
mod overlap {
const fn valid_start(b: u8) -> bool { b == b'_' || b'a' <= b && b <= b'z' }
const fn ok(c: u8) -> bool { (c.wrapping_sub(b'a') < 26) | (c.wrapping_sub(b'0') < 10) | (c == b'_') }
const fn chunk_ok<const N: usize>(chunk: &[u8; N]) -> bool {
    let mut all_valid = true; let mut i = 0;
    while i < N { all_valid &= ok(chunk[i]); i += 1; }
    all_valid
}
pub const fn is_snake_case(s: &str) -> bool {
    let bytes = s.as_bytes();
    match bytes { [first, ..] if valid_start(*first) => {}, _ => return false }
    if let Some(last) = bytes.last_chunk::<16>() {
        let mut rest = bytes;
        while let Some((chunk, tail)) = rest.split_first_chunk::<16>() {
            if !chunk_ok(chunk) { return false; }
            rest = tail;
        }
        return chunk_ok(last);
    }
    if let (Some(first), Some(last)) = (bytes.first_chunk::<8>(), bytes.last_chunk::<8>()) {
        return chunk_ok(first) & chunk_ok(last);
    }
    if let (Some(first), Some(last)) = (bytes.first_chunk::<4>(), bytes.last_chunk::<4>()) {
        return chunk_ok(first) & chunk_ok(last);
    }
    let mut rest = bytes;
    while let [c, tail @ ..] = rest { if !ok(*c) { return false; } rest = tail; }
    true
}
}

// overlapping chunks; start check folded into the result instead of an early return
mod pairs {
const fn valid_start(b: u8) -> bool { b == b'_' || b'a' <= b && b <= b'z' }
const fn ok(c: u8) -> bool { (c.wrapping_sub(b'a') < 26) | (c.wrapping_sub(b'0') < 10) | (c == b'_') }
const fn chunk_ok<const N: usize>(chunk: &[u8; N]) -> bool {
    let mut all_valid = true; let mut i = 0;
    while i < N { all_valid &= ok(chunk[i]); i += 1; }
    all_valid
}
pub const fn is_snake_case(s: &str) -> bool {
    let bytes = s.as_bytes();
    // the chunks check every byte with ok(); the only extra rule for the first byte is "not a digit".
    let start = match bytes { [first, ..] => *first > b'9', [] => return false };
    if let Some(last) = bytes.last_chunk::<16>() {
        let mut rest = bytes;
        while let Some((chunk, tail)) = rest.split_first_chunk::<16>() {
            if !chunk_ok(chunk) { return false; }
            rest = tail;
        }
        return chunk_ok(last) & start;
    }
    if let (Some(first), Some(last)) = (bytes.first_chunk::<8>(), bytes.last_chunk::<8>()) {
        let mut both = [0u8; 16]; let mut i = 0;
        while i < 8 { both[i] = first[i]; both[i + 8] = last[i]; i += 1; }
        return chunk_ok(&both) & start;
    }
    if let (Some(first), Some(last)) = (bytes.first_chunk::<4>(), bytes.last_chunk::<4>()) {
        let mut both = [0u8; 8]; let mut i = 0;
        while i < 4 { both[i] = first[i]; both[i + 4] = last[i]; i += 1; }
        return chunk_ok(&both) & start;
    }
    let mut rest = bytes;
    let mut all_valid = start;
    while let [c, tail @ ..] = rest { all_valid &= ok(*c); rest = tail; }
    all_valid
}
}

fn t(f: fn(&str) -> bool, s: &str, it: usize) -> f64 {
    let mut best = f64::MAX;
    for _ in 0..7 {
        let t = Instant::now();
        for _ in 0..it { black_box(f(black_box(s))); }
        best = best.min(t.elapsed().as_nanos() as f64 / it as f64);
    }
    best
}

fn main() {
    let fs: [(&str, fn(&str) -> bool); 4] =
        [("master", master::is_snake_case), ("current", current::is_snake_case), ("overlap", overlap::is_snake_case), ("pairs", pairs::is_snake_case)];
    // correctness: valid strings, and a bad byte at every position
    for n in 1..80 {
        let s: String = "abc_def_0123456789_".chars().cycle().take(n).collect();
        for (name, f) in fs {
            assert!(f(&s), "{name} rejected {s:?}");
            for p in 0..n {
                let mut v = s.clone().into_bytes(); v[p] = b'X';
                let v = String::from_utf8(v).unwrap();
                assert!(!f(&v), "{name} accepted {v:?}");
            }
            assert!(!f("") && !f("1abc") && f("_1"), "{name} edge cases");
        }
    }
    println!("all variants agree on correctness\n");
    println!("  len   master  current  overlap    pairs   cur_x  over_x  pair_x");
    for n in [2usize, 3, 4, 5, 6, 7, 8, 9, 12, 15, 16, 17, 20, 24, 31, 32, 64, 256, 4096] {
        let s: String = "abc_def_0123456789_".chars().cycle().take(n).collect();
        let it = 20_000_000 / n.max(16);
        let r: Vec<f64> = fs.iter().map(|(_, f)| t(*f, &s, it)).collect();
        println!("{n:5} {:8.2} {:8.2} {:8.2} {:8.2} {:6.1}x {:6.1}x {:6.1}x", r[0], r[1], r[2], r[3], r[0] / r[1], r[0] / r[2], r[0] / r[3]);
    }
    // mixed lengths: random identifiers, so branch prediction can't learn one length
    println!("\nmixed lengths, ns per call   master  current  overlap    pairs   cur_x  over_x  pair_x");
    for (lo, hi) in [(2usize, 24usize), (2, 64)] {
        let mut seed = 0x2545F4914F6CDD1Du64;
        let mut rnd = move || { seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17; seed };
        let alphabet = b"abcdefghijklmnopqrstuvwxyz_0123456789";
        let ids: Vec<String> = (0..4096).map(|_| {
            let n = lo + (rnd() as usize) % (hi - lo + 1);
            (0..n).map(|k| alphabet[(rnd() as usize) % if k == 0 { 27 } else { alphabet.len() }] as char).collect()
        }).collect();
        let r: Vec<f64> = fs.iter().map(|(name, f)| {
            assert!(ids.iter().all(|s| f(s)), "{name} rejected a mixed id");
            let mut best = f64::MAX;
            for _ in 0..7 {
                let t = Instant::now();
                for _ in 0..500 { for s in &ids { black_box(f(black_box(s))); } }
                best = best.min(t.elapsed().as_nanos() as f64 / (500 * ids.len()) as f64);
            }
            best
        }).collect();
        println!("  len {lo:2}..={hi:2}                {:8.2} {:8.2} {:8.2} {:8.2} {:6.1}x {:6.1}x {:6.1}x", r[0], r[1], r[2], r[3], r[0] / r[1], r[0] / r[2], r[0] / r[3]);
    }
}
