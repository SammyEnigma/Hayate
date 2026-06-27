//! CodSpeed benchmarks for hash algorithm throughput comparison.
//!
//! Hayate uses blake3 for transfer integrity and rapidhash for fast
//! non-cryptographic lookups. This file benchmarks both alongside SHA-256
//! at representative payload sizes to track throughput and inform algorithm
//! selection.

use divan::Bencher;
use sha2::{Digest, Sha256};

fn main() {
    divan::main();
}

/// Payload sizes from a small fragment up to a large multi-MiB block.
const SIZES: &[usize] = &[1024, 64 * 1024, 1024 * 1024, 16 * 1024 * 1024];

#[divan::bench(args = SIZES)]
fn hash_blake3(bencher: Bencher, size: usize) {
    let data = vec![0u8; size];
    bencher.bench(|| divan::black_box(blake3::hash(divan::black_box(&data))));
}

#[divan::bench(args = SIZES)]
fn hash_sha256(bencher: Bencher, size: usize) {
    let data = vec![0u8; size];
    bencher.bench(|| {
        let mut hasher = Sha256::new();
        hasher.update(divan::black_box(&data));
        divan::black_box(hasher.finalize())
    });
}

#[divan::bench(args = SIZES)]
fn hash_rapidhash(bencher: Bencher, size: usize) {
    let data = vec![0u8; size];
    bencher.bench(|| {
        let mut hasher =
            rapidhash::v3::RapidStreamHasherV3::new(&rapidhash::v3::DEFAULT_RAPID_SECRETS);
        hasher.write(divan::black_box(&data));
        divan::black_box(hasher.finish())
    });
}
