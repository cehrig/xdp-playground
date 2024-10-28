#![allow(path_statements)]
#![allow(clippy::no_effect)]

use crate::assert::{unsafe_no_panic, ExpectNotZero, ExpectPositive};
use crate::{Error, Result};
use libc::{if_nametoindex, sysconf, _SC_PAGE_SIZE};
use std::ffi::CString;

/// Aligns a value to a given bound
pub(crate) trait AlignUp {
    fn align_up(n: Self, bound: Self) -> Self;
}

impl AlignUp for u32 {
    fn align_up(n: Self, bound: Self) -> Self {
        ((n + (bound - 1)) / bound) * bound
    }
}

impl AlignUp for usize {
    fn align_up(n: Self, bound: Self) -> Self {
        ((n + (bound - 1)) / bound) * bound
    }
}

#[cfg(target_os = "linux")]
pub fn ifindex<I>(name: I) -> Result<u32>
where
    I: Into<String>,
{
    let name = CString::new(name.into())?;

    unsafe_no_panic!(if_nametoindex(name.as_ref() as *const _ as _))
        .expect(ExpectNotZero, Error::InterfaceInvalid)
}

#[cfg(target_os = "linux")]
pub(crate) fn page_size() -> Result<usize> {
    unsafe_no_panic!(sysconf(_SC_PAGE_SIZE))
        .expect(ExpectPositive, Error::PageSizeInvalid)
        .map(|ok| ok as usize)
}

#[cfg(not(target_os = "linux"))]
pub(crate) unsafe fn page_size() -> i64 {
    unimplemented!("Page-aligned ArrayUmem is only supported on Linux")
}

pub fn split_array<T, const N: usize, const L: usize, const R: usize>(
    input: [T; N],
) -> ([T; L], [T; R])
where
    for<'a> [T; L]: TryFrom<&'a [T]>,
    for<'a> [T; R]: TryFrom<&'a [T]>,
{
    struct Assert<const N: usize, const L: usize, const R: usize>;
    impl<const N: usize, const L: usize, const R: usize> Assert<N, L, R> {
        const OK: () = assert!(L + R == N);
    }

    Assert::<N, L, R>::OK;

    let (left, right) = input.split_at(L);

    (
        left.try_into().unwrap_or_else(|_| unreachable!()),
        right.try_into().unwrap_or_else(|_| unreachable!()),
    )
}
