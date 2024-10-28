fn main() {
    let a = 1;
    let b = &a as *const i32;

    std::thread::spawn(|| {
        println!("{:?}", b);
    })
    .join();
}
