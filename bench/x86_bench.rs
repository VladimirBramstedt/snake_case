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

// simd-check-and-deref (current)
mod current {
pub const fn is_snake_case(string: &str) -> bool {
    // we only care about ascii chars, which fit in a byte.
    // iterating over utf8 continuation bytes and the like will not count as valid snake case anyway.
    // slice patterns and fixed-size chunks instead of slice indexing: no bounds checks, no panic paths.
    const fn valid_start(b: u8) -> bool {
        b == b'_' || b'a' <= b && b <= b'z'
    }
    // branchless, so the chunk loop below auto-vectorizes.
    const fn ok(c: u8) -> bool {
        (c.wrapping_sub(b'a') < 26) | (c.wrapping_sub(b'0') < 10) | (c == b'_')
    }
    // non-empty and starts with a..z or _
    let mut rest = string.as_bytes();
    match rest {
        [first, ..] if valid_start(*first) => {}
        _ => return false,
    }
    // the first byte gets checked again below; cheaper than misaligning the chunks.
    // 16 bytes at a time, without short-circuiting inside a chunk, so LLVM emits SIMD.
    while let Some((chunk, tail)) = rest.split_first_chunk::<16>() {
        let mut all_valid = true;
        let mut i = 0;
        while i < 16 {
            all_valid &= ok(chunk[i]);
            i += 1;
        }
        if !all_valid {
            return false;
        }
        rest = tail;
    }
    // one 8-byte chunk, so typical identifier lengths take the SIMD path too.
    if let Some((chunk, tail)) = rest.split_first_chunk::<8>() {
        let mut all_valid = true;
        let mut i = 0;
        while i < 8 {
            all_valid &= ok(chunk[i]);
            i += 1;
        }
        if !all_valid {
            return false;
        }
        rest = tail;
    }
    while let [c, tail @ ..] = rest {
        if !ok(*c) {
            return false;
        }
        rest = tail;
    }
    true
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
    let fs: [(&str, fn(&str) -> bool); 3] =
        [("master", master::is_snake_case), ("current", current::is_snake_case), ("overlap", overlap::is_snake_case)];
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
    println!("  len   master  current  overlap   cur_x  over_x");
    for n in [2usize, 3, 4, 5, 6, 7, 8, 9, 12, 15, 16, 17, 20, 24, 31, 32, 64, 256, 4096] {
        let s: String = "abc_def_0123456789_".chars().cycle().take(n).collect();
        let it = 20_000_000 / n.max(16);
        let (m, c, o) = (t(fs[0].1, &s, it), t(fs[1].1, &s, it), t(fs[2].1, &s, it));
        println!("{n:5} {m:8.2} {c:8.2} {o:8.2} {:6.1}x {:6.1}x", m / c, m / o);
    }
}
