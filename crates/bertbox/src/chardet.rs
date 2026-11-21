use std::borrow::Cow;
use std::fmt;

use chardetng::{EncodingDetector, Iso2022JpDetection, Utf8Detection};

pub fn to_utf8(data: &[u8]) -> Result<Cow<'_, str>, MalformedError> {
    let mut detector = EncodingDetector::new(Iso2022JpDetection::Allow);
    detector.feed(data, true);
    let encoding = detector.guess(None, Utf8Detection::Allow);
    let (text, _, malformed) = encoding.decode(data);
    if malformed {
        return Err(MalformedError {
            encoding: encoding.name(),
        });
    }
    Ok(text)
}

#[derive(Debug)]
pub struct MalformedError {
    pub encoding: &'static str,
}

impl fmt::Display for MalformedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "not valid {}", self.encoding)
    }
}

impl std::error::Error for MalformedError {}
