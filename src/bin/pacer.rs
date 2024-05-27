use clap::Parser;
use futures::future::select_all;
use libbpf_rs::{Link, Map, Object};
use prometheus::{Encoder, IntCounter, Opts, Registry};
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::convert::Infallible;
use std::fmt::{Debug, Display, Formatter};
use std::future::Future;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::sync::Mutex;
use warp::Filter;
use xdp::ringbuf::Ringbuf;
use xdp::utility::ifindex;

type Interface = u32;

#[repr(C)]
#[derive(Debug)]
enum AddrType {
    IPV4 = 0,
    IPv6,
}

#[derive(Debug, Hash, PartialEq, Eq)]
enum AddressType {
    Ipv4(Ipv4Addr),
    Ipv6(Ipv6Addr),
}

#[derive(Debug, Hash, PartialEq, Eq)]
struct Address {
    ifindex: Interface,
    address: AddressType,
}

impl Display for AddressType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            AddressType::Ipv4(v4) => f.write_str(&format!("{}", v4)),
            AddressType::Ipv6(v6) => f.write_str(&format!("{}", v6)),
        }
    }
}

impl Address {
    fn from_octets(octets: [u8; 24]) -> Self {
        let ifindex = u32::from_le_bytes(octets[0..4].try_into().unwrap());
        let kind = u32::from_le_bytes(octets[4..8].try_into().unwrap());

        let kind = match kind {
            0 => AddrType::IPV4,
            1 => AddrType::IPv6,
            _ => unreachable!("unknown address type"),
        };

        let address = match kind {
            AddrType::IPV4 => AddressType::Ipv4(<Ipv4Addr as From<[u8; 4]>>::from(
                octets[8..12].try_into().unwrap(),
            )),
            AddrType::IPv6 => AddressType::Ipv6(<Ipv6Addr as From<[u8; 16]>>::from(
                octets[8..24].try_into().unwrap(),
            )),
        };

        Address { ifindex, address }
    }
}

#[derive(Parser, Debug)]
struct Args {
    #[arg(default_value = "bpf/pacer_kern.o")]
    bpf_obj: String,

    #[arg(default_value = "bpf/pacer_kern.o")]
    bpf_prog: String,

    #[arg(long)]
    interfaces: Vec<String>,
}

struct Bpf {
    object: Object,
    links: Vec<Link>,
}

impl Drop for Bpf {
    fn drop(&mut self) {
        for link in &self.links {
            link.detach().unwrap()
        }
    }
}

impl Bpf {
    fn new<P>(path: P) -> Self
    where
        P: AsRef<Path>,
    {
        let mut builder = libbpf_rs::ObjectBuilder::default();
        let open = builder.open_file(path).expect("unable to open object");
        let object = open.load().expect("unable to load object");

        Bpf {
            object,
            links: vec![],
        }
    }

    fn attach(&mut self, interfaces: Vec<String>) {
        let prog = self
            .object
            .prog_mut("xdp_pacer")
            .expect("unable to load prog");

        for interface in interfaces {
            self.links.push(
                prog.attach_xdp(ifindex(interface).expect("no interface found") as i32)
                    .expect("unable to attach program"),
            )
        }
    }

    fn ringbuf(&self) -> &Map {
        self.object.map("packets").expect("unable to load map")
    }
}

#[derive(Default, Debug)]
struct Log {
    inner: Mutex<HashMap<Address, u64>>,
}

impl Log {
    async fn tick(&self, address: Address) {
        let mut lock = self.inner.lock().await;

        match lock.entry(address) {
            Entry::Occupied(mut addr) => {
                *addr.get_mut() += 1;
            }
            Entry::Vacant(addr) => {
                addr.insert(1);
            }
        }
    }

    async fn registry(&self) -> Registry {
        let registry = Registry::new();

        let lock = self.inner.lock().await;

        for (address, packets) in lock.iter() {
            let opts = Opts::new("packets", "Number of packets")
                .const_label("address", format!("{}", address.address))
                .const_label("ifindex", format!("{}", address.ifindex));

            let counter = IntCounter::with_opts(opts).unwrap();
            counter.inc_by(*packets);

            registry.register(Box::new(counter)).unwrap();
        }

        registry
    }
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let log = Arc::new(Log::default());

    select_all(vec![
        Box::pin(ringbuffer(&args, Arc::clone(&log))) as Pin<Box<dyn Future<Output = ()>>>,
        Box::pin(prometheus(&args, Arc::clone(&log))),
    ])
    .await;
}

async fn prometheus(_: &Args, log: Arc<Log>) {
    let hello = warp::any().and_then(move || {
        let log = log.clone();
        async move {
            let registry = log.registry().await;
            let mut buffer = Vec::<u8>::new();

            let encoder = prometheus::TextEncoder::new();
            encoder.encode(&registry.gather(), &mut buffer).unwrap();

            let text = String::from_utf8(buffer).unwrap();

            Ok::<String, Infallible>(text)
        }
    });

    warp::serve(hello).run(([0, 0, 0, 0], 3030)).await;
}

async fn ringbuffer(args: &Args, log: Arc<Log>) {
    let mut bpf = Bpf::new(args.bpf_obj.clone());

    bpf.attach(args.interfaces.clone());

    let mut ringbuf = Ringbuf::from_map(bpf.ringbuf()).expect("can't load ringbuffer");

    loop {
        let mut data = [0u8; 24];
        let _ = ringbuf
            .read(&mut data)
            .await
            .expect("can't read from ringbuf");

        let addr = Address::from_octets(data);
        log.tick(addr).await;
    }
}
