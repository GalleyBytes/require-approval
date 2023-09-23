mod poll;
mod wait;

fn main() {
    std::thread::spawn(poll::poll);
    wait::wait();
}
