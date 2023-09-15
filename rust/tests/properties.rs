mod fuzzer;

use fuzzer::NetWrapper as Net;
use hvm_core::core::Book;
use quickcheck_macros::quickcheck;

const MAX_STEPS: usize = 1000;

#[quickcheck]
fn prop_reduces_or_stops(Net(net): Net) -> bool {
    let mut net = net;
    net.normal(&Book::new(), Some(MAX_STEPS));
    net.rwts >= MAX_STEPS || net.acts.is_empty()
}

#[ignore]
#[quickcheck]
fn prop_confluence(Net(unchanged): Net) -> bool {
    use rand::seq::SliceRandom;
    use rand::thread_rng;

    let mut reduced = unchanged.clone();
    reduced.normal(&Book::new(), Some(MAX_STEPS));

    for _ in 0..20 {
        let mut net = unchanged.clone();
        net.acts.shuffle(&mut thread_rng());
        net.normal(&Book::new(), Some(MAX_STEPS));
        if net.rwts != reduced.rwts {
            println!("rwts: {} != {}", net.rwts, reduced.rwts);
            return false;
        }
        // TODO: check that the result is the same
    }
    true
}
