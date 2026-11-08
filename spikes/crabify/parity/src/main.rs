//! Print what one Rust PDF backend sees in one file, in baseline.py's canonical form.
//!     parity hayro|lopdf FILE [--dump-ops PAGE]     object keys ("12.0") on stdin, one JSON record on stdout
mod hayro_b;
mod lopdf_b;
mod strict;
mod strict_b;

use sha1::{Digest, Sha1};
use std::io::Read;

pub const MAX_DEPTH: usize = 12;
pub type M = [f64; 6];
pub const ID: M = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

pub fn mul(a: &M, b: &M) -> M {
    [
        a[0] * b[0] + a[1] * b[2],
        a[0] * b[1] + a[1] * b[3],
        a[2] * b[0] + a[3] * b[2],
        a[2] * b[1] + a[3] * b[3],
        a[4] * b[0] + a[5] * b[2] + b[4],
        a[4] * b[1] + a[5] * b[3] + b[5],
    ]
}

pub fn short(data: &[u8]) -> String {
    let d = Sha1::digest(data);
    d.iter().take(6).map(|b| format!("{b:02x}")).collect()
}

pub fn hex(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02x}")).collect()
}

/// Hashes the canonical op lines of a page (and prints them when dumping).
pub struct Sink {
    pub h: Sha1,
    pub n: usize,
    pub dump: bool,
}

impl Sink {
    pub fn new(dump: bool) -> Self {
        Self { h: Sha1::new(), n: 0, dump }
    }
    pub fn line(&mut self, op: &str, depth: usize, ctm: &M, operands: &str) {
        let line = if op == "BI" {
            format!("BI|{depth}\n")
        } else {
            let c: Vec<String> = ctm.iter().map(|v| format!("{v:.3}")).collect();
            format!("{op}|{depth}|{}|{operands}\n", c.join(","))
        };
        if self.dump {
            print!("{line}");
        }
        self.h.update(line.as_bytes());
        self.n += 1;
    }
    pub fn finish(self) -> serde_json::Value {
        let d = self.h.finalize();
        serde_json::json!([self.n, d.iter().take(6).map(|b| format!("{b:02x}")).collect::<String>()])
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let dump = a.iter().position(|x| x == "--dump-ops").map(|i| a[i + 1].parse::<usize>().unwrap());
    let mut keys = String::new();
    if dump.is_none() {
        std::io::stdin().read_to_string(&mut keys).unwrap();
    }
    let keys: Vec<(u32, i32)> = keys
        .lines()
        .map(|l| {
            let (n, g) = l.split_once('.').unwrap();
            (n.parse().unwrap(), g.parse().unwrap())
        })
        .collect();
    let rec = match a[1].as_str() {
        "hayro" => hayro_b::record(&a[2], &keys, dump),
        "lopdf" => lopdf_b::record(&a[2], &keys, dump),
        "strict" => strict_b::record(&a[2], &keys, dump),
        other => panic!("no backend {other}"),
    };
    if dump.is_none() {
        println!("{rec}");
    }
}
