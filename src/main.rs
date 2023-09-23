use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

use aligned::{Aligned, A64};

trait Queue {
    fn new(capacity: usize) -> Self;
    fn push(&mut self, val: i32) -> bool;
    fn pop(&mut self, val: &mut i32) -> bool;
}

#[derive(Debug, Default)]
struct RingBuffer {
    data_: Vec<i32>,
    read_idx_: Aligned<A64, AtomicUsize>,
    read_idx_cached_: usize,
    write_idx_: Aligned<A64, AtomicUsize>,
    write_idx_cached_: usize,
}
impl Queue for RingBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            data_: vec![0; capacity],
            read_idx_: Aligned(AtomicUsize::new(0)),
            read_idx_cached_: 0,
            write_idx_: Aligned(AtomicUsize::new(0)),
            write_idx_cached_: 0,
        }
    }

    fn push(&mut self, val: i32) -> bool {
        let write_idx = self.write_idx_.load(Ordering::Relaxed);
        let mut next_write_idx = write_idx + 1;

        if next_write_idx == self.data_.len() {
            next_write_idx = 0;
        }

        if next_write_idx == self.read_idx_cached_ {
            self.read_idx_cached_ = self.read_idx_.load(Ordering::Acquire);

            if next_write_idx == self.read_idx_cached_ {
                return false;
            }
        }

        self.data_[write_idx] = val;
        self.write_idx_.store(next_write_idx, Ordering::Relaxed);

        true
    }

    fn pop(&mut self, val: &mut i32) -> bool {
        let read_idx = self.read_idx_.load(Ordering::Relaxed);
        if read_idx == self.write_idx_cached_ {
            self.write_idx_cached_ = self.write_idx_.load(Ordering::Acquire);
            if read_idx == self.write_idx_cached_ {
                return false;
            }
        }
        *val = self.data_[read_idx];
        let mut next_read_idx = read_idx + 1;

        if next_read_idx == self.data_.len() {
            next_read_idx = 0;
        }
        self.read_idx_.store(next_read_idx, Ordering::Release);
        true
    }
}

fn pin_thread(cpu: usize) {
    unsafe {
        let mut cpuset = std::mem::zeroed();
        libc::CPU_ZERO(&mut cpuset);
        libc::CPU_SET(cpu, &mut cpuset);

        let result = libc::pthread_setaffinity_np(
            libc::pthread_self(),
            std::mem::size_of::<libc::cpu_set_t>(),
            &cpuset,
        );

        if result == -1 {
            eprintln!("Error: {}", std::io::Error::last_os_error());
            std::process::exit(1);
        }
    }
}

fn bench(cpu1: usize, cpu2: usize, iters: i32, buffer_rign: RingBuffer) {
    let q = Arc::new(Mutex::new(buffer_rign));
    let q2 = Arc::clone(&q);

    let t = std::thread::spawn(move || {
        pin_thread(cpu1);
        for i in 0..iters {
            let mut val = 0_i32;
            loop {
                if q.lock().unwrap().pop(&mut val) {
                    break;
                }
            }

            if val != i {
                println!("Error: expected {} got {}", i, val);
                std::process::exit(1);
            }
        }
    });

    pin_thread(cpu2);
    let start = std::time::Instant::now();

    for i in 0..iters {
        loop {
            if q2.lock().unwrap().push(i) {
                break;
            }
        }
    }

    loop {
        let q = q2.lock().unwrap();
        if q.read_idx_.load(Ordering::Relaxed) == q.write_idx_.load(Ordering::Relaxed) {
            break;
        }
    }

    let stop = start.elapsed();
    t.join().unwrap();

    let secs = stop.as_secs() as f64 + f64::from(stop.subsec_nanos()) * 1e-9;
    println!(
        "RingBuffer: {} iters in {} secs, {} ops/sec",
        iters,
        secs,
        iters as f64 / secs
    );
}

fn main() {
    let queue = 100000;
    let cpu1 = 0;
    let cpu2 = 1;
    let iters = 100000000;
    bench(cpu1, cpu2, iters, RingBuffer::new(queue));
    println!("Done");
}
