use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use md5::Md5;
use serde::Serialize;
use sha1::Sha1;
use sha2::{Digest, Sha256};

const BUFFER_SIZE: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct Hashes {
    pub crc32: String,
    pub md5: String,
    pub sha1: String,
    pub sha256: String,
}

impl Hashes {
    pub fn of_file(path: &Path) -> io::Result<Self> {
        Hashes::of_reader(File::open(path)?)
    }

    pub fn of_reader(mut reader: impl Read) -> io::Result<Self> {
        let mut crc32 = crc32fast::Hasher::new();
        let mut md5 = Md5::new();
        let mut sha1 = Sha1::new();
        let mut sha256 = Sha256::new();
        let mut buffer = vec![0; BUFFER_SIZE];
        loop {
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            let chunk = &buffer[..read];
            crc32.update(chunk);
            md5.update(chunk);
            sha1.update(chunk);
            sha256.update(chunk);
        }
        Ok(Hashes {
            crc32: format!("{:x}", crc32.finalize()),
            md5: format!("{:x}", md5.finalize()),
            sha1: format!("{:x}", sha1.finalize()),
            sha256: format!("{:x}", sha256.finalize()),
        })
    }

    pub fn named(&self) -> [(&'static str, &str); 4] {
        [
            ("crc32", &self.crc32),
            ("md5", &self.md5),
            ("sha1", &self.sha1),
            ("sha256", &self.sha256),
        ]
    }
}
