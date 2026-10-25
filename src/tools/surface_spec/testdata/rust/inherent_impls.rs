pub struct Stack<T> {
    items: Vec<T>,
}

impl<T: Clone> Stack<T> {
    pub const CAPACITY: usize = 16;

    pub fn new() -> Self {
        Self { items: Vec::with_capacity(Self::CAPACITY) }
    }

    pub fn push(&mut self, item: T) {
        self.items.push(item);
    }

    fn peek(&self) -> Option<&T> {
        self.items.last()
    }
}

impl Stack<u8> {
    pub fn sum(&self) -> u32 {
        self.items.iter().map(|&item| u32::from(item)).sum()
    }
}

impl<T> Stack<T>
where
    T: Default + Clone,
{
    pub(crate) fn fill(count: usize) -> Self {
        Self { items: vec![T::default(); count] }
    }

    pub fn len<'a>(&'a self) -> usize {
        self.items.len()
    }
}
