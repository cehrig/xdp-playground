use crate::assert::{expect_or_error, ExpectNonNullPtr};
use crate::utility::{page_size, AlignUp};
use crate::{Error, Result};
use futures::ready;
use libbpf_rs::{Map, MapType};
use libc::{mmap, MAP_SHARED, PROT_READ, PROT_WRITE};
use std::ffi::{c_ulong, c_void};
use std::io::ErrorKind;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd};
use std::pin::Pin;
use std::ptr::null_mut;
use std::slice;
use std::sync::atomic::{compiler_fence, Ordering};
use std::task::{Context, Poll};
use tokio::io::unix::AsyncFd;
use tokio::io::{AsyncRead, ReadBuf};

const BPF_RINGBUF_BUSY_BIT: u32 = 1 << 31;
const BPF_RINGBUF_DISCARD_BIT: u32 = 1 << 30;
const BPF_RINGBUF_HDR_SZ: u32 = 8;

#[derive(Debug)]
pub struct Ringbuf<'a> {
    fd: AsyncFd<BorrowedFd<'a>>,
    mask: usize,
    consumer: *mut c_void,
    producer: *mut c_void,
    data: *mut c_void,
}

unsafe impl<'a> Send for Ringbuf<'a> {}

impl<'a> Ringbuf<'a> {
    fn new(
        fd: BorrowedFd<'a>,
        mask: usize,
        consumer: *mut c_void,
        producer: *mut c_void,
        data: *mut c_void,
    ) -> Self {
        Ringbuf {
            fd: AsyncFd::with_interest(fd, tokio::io::Interest::READABLE).unwrap(),
            mask,
            consumer,
            producer,
            data,
        }
    }

    /// Returns a BPF ring buffer from a given Map
    pub fn from_map(map: &'a Map) -> Result<Self> {
        if map.map_type() != MapType::RingBuf {
            return Err(Error::WrongMapType)?;
        }

        let max_entries = map
            .info()
            .expect("failed getting map info")
            .info
            .max_entries;
        let mask = max_entries.checked_sub(1).expect("ring buf was empty") as usize;
        let page_size = unsafe { page_size()? as usize };
        let mmap_sz: usize = page_size + 2 * (max_entries as usize);

        let consumer = unsafe {
            expect_or_error(
                ExpectNonNullPtr,
                mmap(
                    null_mut(),
                    page_size,
                    PROT_READ | PROT_WRITE,
                    MAP_SHARED,
                    map.as_fd().as_raw_fd(),
                    0,
                ),
                Error::ConsumerMmap,
            )?
        };

        let producer = unsafe {
            expect_or_error(
                ExpectNonNullPtr,
                mmap(
                    null_mut(),
                    mmap_sz,
                    PROT_READ,
                    MAP_SHARED,
                    map.as_fd().as_raw_fd(),
                    page_size as _,
                ),
                Error::ProducerMmap,
            )?
        };

        Ok(Self::new(map.as_fd(), mask, consumer, producer, unsafe {
            producer.add(page_size)
        }))
    }

    /// Returns the file descriptor associated with the ring buffer
    pub fn fd(&self) -> BorrowedFd<'_> {
        self.fd.as_fd()
    }
}

/// Reads bytes from the given pointer and adds a memory barrier. This should be used with Acquire
#[inline(always)]
fn read_volatile_fence<T>(ptr: *const T, ordering: Ordering) -> T {
    let val = unsafe { std::ptr::read_volatile(ptr) };
    compiler_fence(ordering);

    val
}

/// Writes bytes to the given pointer and adds a memory barrier. This should be used with Release
#[inline(always)]
fn write_volatile_fence<T>(ptr: *mut T, val: T, ordering: Ordering) {
    compiler_fence(ordering);
    unsafe { std::ptr::write_volatile(ptr, val) };
}

/// Given a ring buffer header, removes the Busy and Discard bits, then adds the length of the BPF
/// header and aligns it to a byte boundary
#[inline(always)]
fn roundup_len(mut len: u32) -> u32 {
    len <<= 2;
    len >>= 2;
    len += BPF_RINGBUF_HDR_SZ;

    u32::align_up(len, 8)
}

impl<'a> AsyncRead for Ringbuf<'a> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        // Read consumer position
        let mut consumer_pos =
            read_volatile_fence(self.consumer as *const c_ulong, Ordering::Acquire);

        loop {
            // Read producer position
            let producer_pos =
                read_volatile_fence(self.producer as *const c_ulong, Ordering::Acquire);

            // Check if we read every element from the ring buffer
            if consumer_pos == producer_pos {
                // Poll for readiness.
                let mut guard = ready!(self.fd.poll_read_ready(cx))?;

                // If that's the case, we clear the readiness state and rely on the kernel to send
                // a notification once a new element is ready.
                guard.clear_ready();

                // We immediately continue to test if there are new elements in the ring buffer.
                // If this is the case, the kernel should have sent us a notification, so the next
                // producer position has increased.
                // If this is not the case, we will be getting Poll::Pending from the next poll_read_ready
                // and register the waker again with tokio
                continue;
            }

            // Get a pointer to the header of the next object
            let len_ptr = unsafe { self.data.add(consumer_pos as usize & self.mask) };

            // Read only the header byte
            let len = read_volatile_fence(len_ptr as *const u32, Ordering::Acquire);

            // If this element is currently being written by the kernel, we immediately continue
            // and try again
            if len & BPF_RINGBUF_BUSY_BIT != 0 {
                continue;
            }

            // Increase consumer position, but don't write it yet
            consumer_pos += roundup_len(len) as u64;

            // If the discard bit is set we write the consumer position and try to get the next
            // element
            if len & BPF_RINGBUF_DISCARD_BIT != 0 {
                write_volatile_fence(
                    self.consumer as *mut c_ulong,
                    consumer_pos,
                    Ordering::Release,
                );

                continue;
            }

            // We get a pointer to the actual message payload and create a slice to it
            let data = unsafe { len_ptr.add(BPF_RINGBUF_HDR_SZ as usize) };
            let slice: &[u8] = unsafe { slice::from_raw_parts(data as *const u8, len as usize) };

            // Write slice to buffer if we have enough capacity
            let result = match buf.capacity() >= slice.len() {
                true => {
                    buf.put_slice(slice);
                    Ok(())
                }
                false => Err(ErrorKind::WriteZero.into()),
            };

            // Write consumer position back to ring buffer
            write_volatile_fence(
                self.consumer as *mut c_ulong,
                consumer_pos,
                Ordering::Release,
            );

            // Hand data over to the consumer
            return Poll::Ready(result);
        }
    }
}
