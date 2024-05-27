use libc::{c_int, c_long, c_uint};

/// Represents an expected non-negative value
pub(crate) struct ExpectNonNegative;

pub(crate) struct ExpectNotZero;

pub(crate) struct ExpectPositive;

/// Represents an expected non-null pointer
pub(crate) struct ExpectNonNullPtr;

/// Represents a value equal to a type's default
pub(crate) struct ExpectDefault;

pub(crate) struct ExpectOk;

impl<T, E> AssertReturn<Result<T, E>> for ExpectOk {
    fn assert(_: &Result<T, E>) -> bool {
        todo!()
    }
}

pub(crate) trait AssertReturn<T> {
    fn assert(_: &T) -> bool;
}

impl<T> AssertReturn<T> for ExpectDefault
where
    T: Default + PartialEq,
{
    fn assert(ty: &T) -> bool {
        ty == &T::default()
    }
}

impl<T> AssertReturn<*mut T> for ExpectNonNullPtr {
    fn assert(ty: &*mut T) -> bool {
        !(*ty).is_null()
    }
}

impl<T> AssertReturn<*const T> for ExpectNonNullPtr {
    fn assert(ty: &*const T) -> bool {
        !(*ty).is_null()
    }
}

impl AssertReturn<c_int> for ExpectNonNegative {
    fn assert(ty: &c_int) -> bool {
        ty >= &0
    }
}

impl AssertReturn<c_uint> for ExpectNotZero {
    fn assert(ty: &c_uint) -> bool {
        ty != &0
    }
}

impl AssertReturn<c_long> for ExpectPositive {
    fn assert(ty: &c_long) -> bool {
        ty > &0
    }
}

pub(crate) fn expect_or_error<P, T, E>(_: P, res: T, ex: E) -> crate::Result<T>
where
    P: AssertReturn<T>,
    E: std::error::Error + 'static,
{
    if P::assert(&res) {
        Ok(res)
    } else {
        Err(ex.into())
    }
}
