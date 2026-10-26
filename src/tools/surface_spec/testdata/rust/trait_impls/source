use std::fmt::{self, Display, Formatter};

pub struct Wrapper<T>(T);

pub struct Raw(*mut u8);

impl<T: Display> Display for Wrapper<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "[{}]", self.0)
    }
}

unsafe impl Send for Raw {}

impl !Sync for Raw {}

impl<T> From<T> for Wrapper<T>
where
    T: Clone,
{
    fn from(value: T) -> Self {
        Wrapper(value)
    }
}

impl<'a, T> Iterator for Wrapper<&'a [T]> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        let (first, rest) = self.0.split_first()?;
        self.0 = rest;
        Some(first)
    }
}

impl Default for Raw {
    fn default() -> Self {
        Raw(std::ptr::null_mut())
    }
}
