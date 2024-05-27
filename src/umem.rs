use crate::assert::{expect_or_error, ExpectNonNullPtr};
use crate::utility::page_size;
use crate::{Error, Result};
use std::alloc::{alloc, Layout};

pub struct Umem<U> {
    area: U,
}

impl<const C: usize, const N: usize> Drop for ArrayUmem<C, N> {
    fn drop(&mut self) {
        println!("DROP");
    }
}

pub struct ArrayUmem<const C: usize, const N: usize> {
    mem: Box<[[u8; C]; N]>,
}

pub trait UmemStorage {
    fn alloc(size: usize) -> Result<Self>
    where
        Self: Sized;
    fn chunk_size(&self) -> usize;
    fn num_chunks(&self) -> usize;
}

impl<const C: usize, const N: usize> Umem<ArrayUmem<C, N>> {
    pub fn new() -> Result<Self> {
        Ok(Umem {
            area: ArrayUmem::<C, N>::alloc(0)?,
        })
    }
}

impl<A> Umem<A>
where
    A: UmemStorage,
{
    pub fn with_custom(size: usize) -> Result<Self> {
        Ok(Umem {
            area: A::alloc(size)?,
        })
    }

    pub fn chunk_size(&self) -> usize {
        self.area.chunk_size()
    }
}

impl<const C: usize, const N: usize> ArrayUmem<C, N> {
    /// Chunk size must be non-zero and a power of two
    const IS_VALID: bool = { C != 0 && C & (C - 1) == 0 };

    pub(crate) fn new() -> Result<Self> {
        if !Self::IS_VALID {
            return Err(Error::InvalidUmem)?;
        }

        unsafe {
            Ok(Self::from_raw(expect_or_error(
                ExpectNonNullPtr,
                alloc(Layout::from_size_align(C * N, page_size()? as _)?),
                Error::Allocate,
            )? as _))
        }
    }

    unsafe fn from_raw(ptr: *mut [[u8; C]; N]) -> Self {
        ArrayUmem {
            mem: Box::from_raw(ptr),
        }
    }
}

impl<const C: usize, const N: usize> UmemStorage for ArrayUmem<C, N> {
    fn alloc(_: usize) -> Result<Self> {
        Self::new()
    }

    fn chunk_size(&self) -> usize {
        C
    }

    fn num_chunks(&self) -> usize {
        N
    }
}

#[cfg(test)]
mod test {
    use crate::umem::{ArrayUmem, Umem};

    #[test]
    #[should_panic]
    fn umem_zero_chunk_size() {
        Umem::<ArrayUmem<0, 1>>::new().unwrap();
    }

    #[test]
    #[should_panic]
    fn umem_not_power_of_two_chunk_size() {
        Umem::<ArrayUmem<3, 1>>::new().unwrap();
    }

    #[test]
    fn umem_alloc() {
        Umem::<ArrayUmem<4, 1>>::new().unwrap();
    }
}
