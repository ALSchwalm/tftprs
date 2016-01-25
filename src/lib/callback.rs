/// A simple trait representing a callable that will be invoked after some
/// event has occurred.
pub trait Callback<T: ?Sized, U: ?Sized>: Sync + Send {
    fn call(&self, arg1: &T, arg2: &U);
}

/// A default implementation for Fn
impl<F, T: ?Sized, U: ?Sized> Callback<T, U> for F where F: Fn(&T, &U), F: Sync + Send {
    fn call(&self, arg1: &T, arg2: &U) {
        self(arg1, arg2)
    }
}
