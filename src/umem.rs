#![allow(path_statements)]
#![allow(clippy::no_effect)]

use std::alloc::{alloc, Layout};
use std::borrow::Borrow;
use std::ops::Deref;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::ptr::NonNull;

use libc::{setsockopt, socket, AF_XDP, SOCK_RAW, SOL_XDP, XDP_UMEM_REG};

use crate::assert::{unsafe_no_panic, ExpectDefault, ExpectNonNullPtr, ExpectNotMax};
use crate::utility::page_size;
use crate::{Error, Result};

const XSK_UMEM_DEFAULT_FRAME_HEADROOM: u32 = 0;
const XSK_UMEM_DEFAULT_FLAGS: u32 = 0;

const XSK_UMEM_DEFAULT_FILL_SIZE: u32 = 2048;
const XSK_UMEM_DEFAULT_COMP_SIZE: u32 = 2048;

#[derive(Default)]
pub struct UmemBuilder {
    config: UmemConfig,
    fd: Option<OwnedFd>,
}

pub struct Umem<U> {
    area: U,
    config: UmemConfig,
    fd: OwnedFd,
}

#[repr(C)]
pub struct UmemConfig {
    fill_size: u32,
    comp_size: u32,
    frame_headroom: u32,
    flags: u32,
}

#[repr(C)]
struct UmemReg {
    address: u64,
    length: u64,
    chunk_size: u64,
    headroom: u32,
    flags: u32,
}

struct Ring {
    kind: RingKind,
    def: RingDef,
}

enum RingKind {
    Fill,
    Completion,
    Rx,
    Tx,
}

#[repr(C)]
struct RingDef {
    cached_prod: u32,
    cached_cons: u32,
    mask: u32,
    size: u32,
    producer: *const u32,
    consumer: *const u32,
    ring: *const u8,
    flags: *const u32,
}

pub struct ArrayUmem<const C: usize, const N: usize> {
    mem: Box<[[u8; C]; N]>,
}

pub trait UmemStorage {
    fn chunk_size(&self) -> usize;
    fn num_chunks(&self) -> usize;
    fn start(&self) -> NonNull<u8>;

    fn length(&self) -> Result<usize> {
        Ok(self
            .chunk_size()
            .checked_mul(self.num_chunks())
            .ok_or(Error::Overflow)?)
    }
}

impl UmemBuilder {
    pub fn new() -> Self {
        UmemBuilder::default()
    }

    pub fn with_config(mut self, config: UmemConfig) -> Self {
        self.config = config;
        self
    }

    pub fn with_fd(mut self, fd: OwnedFd) -> Self {
        self.fd = Some(fd);
        self
    }

    pub fn with_default_area<const C: usize, const N: usize>(
        self,
    ) -> Result<Umem<ArrayUmem<C, N>>> {
        Umem::with_area(ArrayUmem::<C, N>::new()?, self.fd, self.config)
    }

    pub fn with_area<U>(self, area: U) -> Result<Umem<U>>
    where
        U: UmemStorage,
    {
        Umem::with_area(area, self.fd, self.config)
    }
}

impl UmemReg {
    fn new<A>(area: &A, config: &UmemConfig) -> Result<UmemReg>
    where
        A: UmemStorage,
    {
        let address = (area.start().as_ptr() as usize).try_into()?;
        let length = area.length()?.try_into()?;
        let chunk_size = area.chunk_size().try_into()?;
        let headroom = config.frame_headroom;
        let flags = config.flags;

        let reg = UmemReg {
            address,
            length,
            chunk_size,
            headroom,
            flags,
        };

        Ok(reg)
    }
}

impl<A> Umem<A>
where
    A: UmemStorage,
{
    fn with_area(area: A, fd: Option<OwnedFd>, config: UmemConfig) -> Result<Self> {
        let page_size = page_size()?;

        if area.start().as_ptr().align_offset(page_size) != 0 {
            return Err(Error::UnalignedUmem)?;
        }

        let fd = match fd {
            None => {
                let socket: RawFd = unsafe_no_panic!(socket(AF_XDP, SOCK_RAW, 0))
                    .expect(ExpectNotMax, Error::SocketFdInvalid)?;

                // SAFETY: File Descriptor was properly checked
                unsafe { OwnedFd::from_raw_fd(socket) }
            }
            Some(fd) => fd,
        };

        let reg = UmemReg::new(&area, &config)?;

        unsafe_no_panic!(setsockopt(
            fd.as_raw_fd(),
            SOL_XDP,
            XDP_UMEM_REG,
            &reg as *const _ as _,
            size_of::<UmemReg>() as _
        ))
        .expect(ExpectDefault, Error::UmemReg)?;

        Ok(Umem { area, fd, config })
    }

    pub fn chunk_size(&self) -> usize {
        self.area.chunk_size()
    }
}

impl<const C: usize, const N: usize> ArrayUmem<C, N> {
    pub fn new() -> Result<Self> {
        // Chunk size must be non-zero and a power of two, additionally we require more than 0 chunks
        const {
            assert!(C != 0, "must not be zero");
            assert!(C & (C - 1) == 0, "must be power of two");
            assert!(N > 0, "must be greater than zero");
        }

        let page_size = page_size()?;
        let layout = Layout::from_size_align(C * N, page_size)?;

        Self::from_raw(
            unsafe_no_panic!(alloc(layout)).expect(ExpectNonNullPtr, Error::Allocate)? as _,
        )
    }

    fn from_raw(ptr: *mut [[u8; C]; N]) -> Result<Self> {
        let page_size = page_size()?;

        if ptr.is_null() {
            return Err(Error::InvalidUmem)?;
        }

        if ptr.align_offset(page_size) != 0 {
            return Err(Error::UnalignedUmem)?;
        }

        Ok(ArrayUmem {
            // SAFETY: We made sure the pointer is not null and properly aligned
            mem: unsafe { Box::from_raw(ptr) },
        })
    }
}

impl Default for UmemConfig {
    fn default() -> Self {
        UmemConfig {
            fill_size: XSK_UMEM_DEFAULT_FILL_SIZE,
            comp_size: XSK_UMEM_DEFAULT_COMP_SIZE,
            frame_headroom: XSK_UMEM_DEFAULT_FRAME_HEADROOM,
            flags: XSK_UMEM_DEFAULT_FLAGS,
        }
    }
}

impl<const C: usize, const N: usize> Drop for ArrayUmem<C, N> {
    fn drop(&mut self) {
        println!("DROP");
    }
}

impl<const C: usize, const N: usize> UmemStorage for ArrayUmem<C, N> {
    fn chunk_size(&self) -> usize {
        C
    }

    fn num_chunks(&self) -> usize {
        N
    }

    fn start(&self) -> NonNull<u8> {
        // SAFETY: ArrayUmem was created by us, and we made sure that C + N > 0 gives us a
        // non-null pointer.
        unsafe { NonNull::new_unchecked(self.mem.as_ptr() as *mut _) }
    }
}

#[cfg(test)]
mod test {
    use crate::umem::{ArrayUmem, Umem};

    #[test]
    fn umem_alloc() {
        Umem::with_area(ArrayUmem::<4, 1>::new().unwrap(), None, Default::default()).unwrap();
    }
}
