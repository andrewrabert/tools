//! Items that are none of the captured kinds.

use std::collections::HashMap;
use std::io::{self, Read as _};

extern crate alloc;

mod declared;

macro_rules! double {
    ($value:expr) => {
        $value * 2
    };
}

thread_local! {
    static COUNTER: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

double!(1);

pub const LIMIT: u32 = double!(21);

pub static NAME: &str = "ignored";

static mut COUNT: u32 = 0;

pub type Map = HashMap<String, u32>;

type Reader<'a> = &'a mut dyn io::Read;

pub union Bits {
    int: u32,
    float: f32,
}

pub trait Alias = Clone + Send;
