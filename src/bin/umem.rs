use std::sync::Arc;
use xdp::umem::{Umem, UmemBuilder};

fn main() {
    let umem = UmemBuilder::new()
        .with_default_area::<2048, 2048>()
        .expect("can't build umem");
}

fn bla(help: Arc<Umem<_>>) {}
