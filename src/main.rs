//! Bitcoin CPU miner
//!
//! A Bitcoin CPU miner written in rust to show how the mining process actually works at the low level. This should demonstrate that mining is actually quite a simple process as the mine function is only 20 lines of code with comments.
use bitcoin::{
    blockdata::constants::genesis_block,
    consensus::encode::serialize,
    hashes::{sha256d::Hash as H256, Hash},
    network::constants::Network,
};
use std::{
    borrow::BorrowMut,
    io::Write,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc, Arc,
    },
    thread,
    time::SystemTime,
};

// process in smaller intervals to allow threads to stop and send status updates
const INTERVAL: u32 = 0xffffffff / 1500 | 0;
/// Mine the genesis block
pub fn main() {
    let genesis_block = genesis_block(Network::Bitcoin);

    // the raw header we'll be hashing
    let block_header = serialize(&genesis_block.header);

    // channel to send status updates
    let (tx, rx) = mpsc::channel();

    // the hash of the block header must be below the target
    let target = serialize(&genesis_block.header.target());

    // keep track of the current min and max nonces
    let min = Arc::new(AtomicU32::default());
    let max = Arc::new(AtomicU32::new(INTERVAL));

    // keep track of if we've found the right nonce to tell the other threads to stop
    let found = Arc::new(AtomicBool::default());

    // so we can calculate the hashrate
    let start = SystemTime::now();

    // so we can wait for all of the threads to finish before exiting
    let mut handles = vec![];

    // hashing performed worse for me with multithreading
    let threads = num_cpus::get_physical();

    for _ in 0..threads {
        // clone the necessary values so they can be used in multiple threads
        let min = min.clone();
        let max = max.clone();
        let found = found.clone();
        let target = target.clone();
        let mut block_header = block_header.clone();
        let tx = tx.clone();
        handles.push(thread::spawn(move || loop {
            // increase the min and max for the next run,
            // returing the old values to be used for the current run
            let curr_min = min.fetch_add(INTERVAL, Ordering::SeqCst);
            let curr_max = max.fetch_add(INTERVAL, Ordering::SeqCst);

            // stop if we've reached the max u32 value
            if curr_max == u32::max_value() {
                break;
            }

            match mine(&mut block_header, &target, curr_min, curr_max) {
                // if mining returned a value, this is the valid nonce
                Some(nonce) => {
                    // send a status update with the valid nonce
                    let _ = tx.send((nonce, true));
                    // other threads will see a nonce has been found and stop
                    found.store(true, Ordering::SeqCst);
                    break;
                }
                None => {
                    // stop if another thread found a valid nonce
                    if found.load(Ordering::SeqCst) {
                        break;
                    }
                    // send a status update with the maximum nonce we've tried
                    if let Err(_) = tx.send((curr_max, false)) {
                        break;
                    }
                }
            }
        }));
    }

    thread::spawn(move || {
        let mut max = 0;
        while !found.load(Ordering::SeqCst) {
            match rx.recv() {
                Ok((nonce, found)) if found => {
                    println!("Found nonce {}", nonce);
                    break;
                }
                Ok((hashes, found)) if !found && hashes > max => {
                    max = hashes;
                    let hashrate = hashes as f32 / start.elapsed().unwrap().as_secs_f32();
                    println!(
                        "Status: hashrate={:.2}MH/s hashes={}",
                        hashrate / 1_000_000.0,
                        hashes
                    );
                }
                Err(_) => break,
                _ => (),
            }
        }
    });

    for handle in handles {
        let _ = handle.join();
    }
}

/// Hash until the nonce overflows
pub fn mine(block_header: &mut [u8], target: &[u8], min: u32, max: u32) -> Option<u32> {
    let mut nonce = min;
    loop {
        // no valid proof of work found for nonces between max and min
        if nonce > max {
            break None;
        }
        // bytes 76, 77, 78 and 79 contain the nonce which is a 4 byte number
        block_header[76..80]
            .borrow_mut()
            .write(&nonce.to_le_bytes())
            .unwrap();
        // if hash of the block header is smaller than the target then return the nonce
        if valid_pow_fast(&H256::hash(block_header), &target) {
            break Some(nonce);
        }
        // otherwise increase the nonce and try again
        nonce += 1;
    }
}

/// Calculate whether the hash is smaller than the target quickly
fn valid_pow_fast(hash: &[u8], target: &[u8]) -> bool {
    // compare byte by byte from right to left
    for i in (0..hash.len()).rev() {
        // the current byte in the hash is smaller than the same byte in
        // the target so the hash is smaller and the proof of work is valid
        if hash[i] < target[i] {
            return true;
        }
        // the current byte in the hash is larger than the same byte in
        // the target so the hash is larger and the proof of work is NOT valid
        if hash[i] > target[i] {
            return false;
        }
    }
    // if we have compared every byte and each one is not
    // greater than or less than the same byte in the target
    // then the hash and target are equal and the proof of work is valid
    true
}
