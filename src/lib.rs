use std::fmt::{Display, Formatter};

pub(crate) mod assert;
pub mod ringbuf;
pub mod umem;
pub mod utility;

#[derive(Debug)]
enum Error {
    InterfaceInvalid,
    InvalidUmem,
    PageSize,
    Allocate,
    WrongMapType,
    ConsumerMmap,
    ProducerMmap,
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        todo!()
    }
}

impl std::error::Error for Error {}

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
