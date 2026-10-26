use std::fmt::Debug;

pub trait Named {
    fn name(&self) -> String;
}

/// Capture drops this doc attribute.
pub trait Shape: Named + Clone + Debug {
    type Unit;
    type Scaled: Shape<Unit = Self::Unit>;

    const SIDES: usize;
    const SCALE: f64 = 1.0;

    fn area(&self) -> f64;

    fn describe(&self) -> String {
        format!("{} with area {}", self.name(), self.area())
    }

    fn scale<F>(&self, factor: F) -> Self::Scaled
    where
        F: Into<f64> + Copy;

    fn largest<'a, I: IntoIterator<Item = &'a Self>>(shapes: I) -> Option<&'a Self>
    where
        Self: 'a,
    {
        shapes.into_iter().next()
    }
}

pub(crate) unsafe trait RawAccess<T: ?Sized> {
    unsafe fn read(&self, offset: usize) -> T;
}
