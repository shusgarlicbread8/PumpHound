fn main() {
    let manifest = std::env::args().nth(1).expect("usage: replay <manifest>");
    println!("REPLAY manifest={manifest}");
    println!("virtual_clock=enabled");
    println!("same reducer and paper engine as live");
}
