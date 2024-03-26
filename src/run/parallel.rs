use std::{
  sync::atomic::{AtomicIsize, Ordering},
  time::Duration,
};

use st3::{
  lifo::{Stealer, Worker},
  StealError,
};

use super::*;

impl<'h, M: Mode> Net<'h, M> {
  /// Forks the net into `tids` child nets, for parallel operation.
  pub fn fork(&mut self, tid: usize, tids: usize, worker: Worker<Redex>) -> Self {
    let heap_size = (self.heap.0.len() / tids) & !63; // round down to needed alignment
    let heap_start = heap_size * tid;
    let area = unsafe { std::mem::transmute(&self.heap.0[heap_start .. heap_start + heap_size]) };
    let mut net = Net::new_with_root_worker(area, self.root.clone(), worker);
    net.next = self.next.saturating_sub(heap_start);
    net.head = if tid == 0 { net.head } else { Addr::NULL };
    net.tid = tid;
    net.tids = tids;
    net.tracer.set_tid(tid);
    net
  }

  // Evaluates a term to normal form in parallel
  pub fn parallel_normal(&mut self) -> Duration {
    assert!(!M::LAZY);

    self.expand();

    const LOCAL_LIMIT: usize = 1 << 18; // max local rewrites per epoch

    // Local thread context
    struct ThreadContext<'a, M: Mode> {
      net: Net<'a, M>,                                // thread's own net object
      delta: &'a AtomicRewrites,                      // global delta rewrites
      workers_with_non_empty_queues: &'a AtomicIsize, // how many workers have non-empty queues
      barrier: &'a Barrier,                           // synchronization barrier
      stealers: Vec<Stealer<Redex>>,                  // stealers
      next_stealer_idx: usize,                        // next stealer to try
      last_frame_is_not_empty: bool,                  // redex count of this worker, the last time count() was called
    }

    // Initialize global objects
    let cores = std::thread::available_parallelism().unwrap().get() as usize;
    let tlog2 = cores.ilog2() as usize;
    let tids = 1 << tlog2;
    let delta = AtomicRewrites::default(); // delta rewrite counter
    let workers_with_non_empty_queues = AtomicIsize::new(0);
    let barrier = Arc::new(Barrier::new(tids)); // global barrier

    let worker_count = tids;
    let workers = (0 .. worker_count).map(|_| Worker::<Redex>::new(WORKER_QUEUE_CAPACITY)).collect::<Vec<_>>();
    let stealers = workers.iter().map(|w| w.stealer()).collect::<Vec<_>>();

    let worker_redex_counts = (0 .. worker_count).map(|_| std::cell::Cell::new(0)).collect::<Vec<_>>();

    // Initialize worker queues by distributing redexes evenly
    let mut workers_iter = workers.iter().zip(&worker_redex_counts).cycle();
    while let Some(redex) = self.redexes.pop() {
      let (worker, worker_redex_count) = workers_iter.next().unwrap();

      worker.push(redex).expect("capacity");
      worker_redex_count.set(worker_redex_count.get() + 1); // increment worker redex count
    }

    // Set `workers_with_non_empty_queues` to the number of workers with non-empty
    // queues
    for worker in &workers {
      let val = !worker.is_empty() as isize;
      workers_with_non_empty_queues.fetch_add(val, Ordering::Relaxed);
    }

    let thread_contexts = workers
      .into_iter()
      .enumerate()
      .map(|(tid, worker)| {
        ThreadContext {
          net: self.fork(tid, tids, worker),
          delta: &delta,
          workers_with_non_empty_queues: &workers_with_non_empty_queues,
          barrier: &barrier,
          stealers: {
            // TODO: Don't clone?
            // https://lemire.me/blog/2017/09/18/visiting-all-values-in-an-array-exactly-once-in-random-order
            let mut stealers = stealers.clone();
            // We never want to steal from ourselves
            stealers.remove(tid);
            // Every worker gets a randomized permutation of the stealers
            fastrand::shuffle(&mut stealers);
            stealers
          },
          next_stealer_idx: 0,
          last_frame_is_not_empty: worker_redex_counts[tid].get() > 0,
        }
      })
      .collect::<Vec<_>>();

    // Perform parallel reductions
    let start_time = std::time::Instant::now();
    std::thread::scope(move |s| {
      for mut ctx in thread_contexts.into_iter() {
        s.spawn(move || {
          main(&mut ctx);
        });
      }
    });
    let elapsed = start_time.elapsed();

    delta.add_to(&mut self.rwts);

    // Main reduction loop
    #[inline(always)]
    fn main<M: Mode>(ctx: &mut ThreadContext<M>) {
      loop {
        reduce(ctx);
        if count(ctx) == 0 {
          break;
        }
      }
      ctx.net.rwts.add_to(ctx.delta);
    }

    // Reduce redexes locally, then share with target
    #[inline(always)]
    fn reduce<M: Mode>(ctx: &mut ThreadContext<M>) {
      loop {
        let _reduced = ctx.net.reduce(LOCAL_LIMIT);
        //println!("[{:04x}] reduced {}", ctx.tid, reduced);
        if count(ctx) == 0 {
          break;
        }

        // Steal redexes from other workers
        while {
          let stealer = unsafe { ctx.stealers.get_unchecked(ctx.next_stealer_idx) };
          ctx.next_stealer_idx = (ctx.next_stealer_idx + 1) % ctx.stealers.len();
          let res = stealer.steal(&ctx.net.redexes.slow, |n| n / 2);
          matches!(res, Err(StealError::Busy))
        } {}
      }
    }

    // Count how many workers have non-empty queues
    #[inline(always)]
    fn count<M: Mode>(ctx: &mut ThreadContext<M>) -> usize {
      // // This was the original code, but it's not as fast as the code below.
      // ctx.barry.wait();
      // ctx.workers_with_non_empty_queues.store(0, Ordering::Relaxed);
      // ctx.barry.wait();
      // let cur_frame_val = !ctx.net.redex.is_empty() as isize;
      // ctx.rlens[ctx.tid].store(cur_frame_val as _, Ordering::Relaxed);
      // ctx.workers_with_non_empty_queues.fetch_add(cur_frame_val,
      // Ordering::Relaxed); ctx.barry.wait();
      // ctx.workers_with_non_empty_queues.load(Ordering::Relaxed) as usize

      // // This is a bit faster (2 barriers instead of 3), but we're iterating
      // // over the entire array of `rlens``, which could be slow on a many-core CPU.
      // ctx.barry.wait();
      // ctx.rlens[ctx.tid].store(!ctx.net.redex.is_empty() as _, Ordering::Relaxed);
      // ctx.barry.wait();
      // ctx.rlens.iter().map(|x| x.load(Ordering::Relaxed)).sum()

      // This is the fastest approach (2 barriers instead of 3), and without iteration
      let cur_frame_val = !ctx.net.redexes.is_empty();
      let difference = (cur_frame_val as isize) - (ctx.last_frame_is_not_empty as isize);
      ctx.last_frame_is_not_empty = cur_frame_val;
      ctx.barrier.wait();
      ctx.workers_with_non_empty_queues.fetch_add(difference, Ordering::Relaxed);
      ctx.barrier.wait();
      ctx.workers_with_non_empty_queues.load(Ordering::Relaxed) as usize
    }

    elapsed
  }
}
