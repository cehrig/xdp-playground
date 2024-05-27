use crate::assert::{expect_or_error, ExpectNotZero, ExpectPositive};
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
    unsafe {
        expect_or_error(
            ExpectNotZero,
            if_nametoindex(name.as_ref() as *const _ as _),
            Error::InterfaceInvalid,
        )
    }
}

#[cfg(target_os = "linux")]
pub(crate) unsafe fn page_size() -> Result<u64> {
    Ok(expect_or_error(ExpectPositive, sysconf(_SC_PAGE_SIZE), Error::PageSize)? as u64)
}

#[cfg(not(target_os = "linux"))]
pub(crate) unsafe fn page_size() -> i64 {
    unimplemented!("Page-aligned ArrayUmem is only supported on Linux")
}
