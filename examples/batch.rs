//! Developer tool: decodes the files listed in a TSV (`path<TAB>size`) and prints
//! `path<TAB>sha256` or `path<TAB>ERR` for each row. Used for differential tests.

use sha2::{Digest, Sha256};
use std::io::{BufRead, Write};

fn main() {
    let list = std::env::args().nth(1).expect("usage: batch <list.tsv>");
    let f = std::io::BufReader::new(std::fs::File::open(list).expect("open list"));
    let stdout = std::io::stdout();
    let mut o = stdout.lock();
    for line in f.lines() {
        let line = line.expect("read");
        let mut it = line.split('\t');
        let (Some(path), Some(size)) = (it.next(), it.next()) else {
            continue;
        };
        let size: usize = size.parse().expect("size");
        let data = std::fs::read(path).expect("read input");
        let mut out = vec![0u8; size];
        match lztans::decode(&data, &mut out) {
            Ok(_) => {
                let d: String = Sha256::digest(&out)
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect();
                writeln!(o, "{path}\t{d}").unwrap();
            }
            Err(e) => writeln!(o, "{path}\tERR\t{e}").unwrap(),
        }
    }
}
