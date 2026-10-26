use std::fmt::Debug;

/// Capture drops this doc attribute and keeps the derive below.
#[derive(Debug, Clone)]
pub struct Point {
    pub x: i32,
    pub y: i32,
    label: String,
}

pub struct Pair(pub u8, u16);

pub struct Unit;

pub(crate) struct Borrowed<'a, T> {
    pub value: &'a T,
    count: usize,
}

pub struct Matrix<T, const ROWS: usize, const COLS: usize> {
    cells: [[T; COLS]; ROWS],
}

struct Bounded<'a, K, V = ()>
where
    K: Ord + Debug,
    V: 'a,
{
    key: K,
    value: &'a V,
}

pub struct Wrapped<T: Clone>(T)
where
    T: Default;
