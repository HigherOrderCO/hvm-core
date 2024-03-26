use super::*;

impl<'h, M: Mode> Net<'h, M> {
  /// Forks the net into `tids` child nets, for parallel operation.
  pub fn fork(&mut self, tids: usize) -> impl Iterator<Item = Self> + '_ {
    let redexes_len = self.linker.redexes.len();
    let mut redexes = self.linker.redexes.drain();
    let heap = &self.linker.allocator.heap;
    let next = &self.linker.allocator.next;
    let heads = self.linker.allocator.heads;
    let root = &self.root;
    (0 .. tids).map(move |tid| {
      let heap_size = (heap.0.len() / tids) & !63; // round down to needed alignment
      let heap_start = heap_size * tid;
      let area = unsafe { std::mem::transmute(&heap.0[heap_start .. heap_start + heap_size]) };
      let mut net = Net::new_with_root(area, root.clone());
      net.next = next.saturating_sub(heap_start);
      net.heads = if tid == 0 { heads } else { [Addr::NULL; 4] };
      net.tid = tid;
      net.tids = tids;
      net.tracer.set_tid(tid);
      let count = redexes_len / (tids - tid);
      (&mut redexes).take(count).for_each(|i| net.redux(i.0, i.1));
      net
    })
  }

  // Evaluates a term to normal form in parallel
  pub fn parallel_normal(&mut self) {
    assert!(!M::LAZY);

    // todo
    // self.expand();

    const SHARE_LIMIT: usize = 1 << 12; // max share redexes per split
    const LOCAL_LIMIT: usize = 1 << 18; // max local rewrites per epoch

    // Local thread context
    struct ThreadContext<'a, M: Mode> {
      tid: usize,                             // thread id
      tlog2: usize,                           // log2 of thread count
      tick: usize,                            // current tick
      net: Net<'a, M>,                        // thread's own net object
      delta: &'a AtomicRewrites,              // global delta rewrites
      share: &'a Vec<(AtomicU64, AtomicU64)>, // global share buffer
      rlens: &'a Vec<AtomicUsize>,            // global redex lengths (only counting shareable ones)
      total: &'a AtomicUsize,                 // total redex length
      barry: Arc<Barrier>,                    // synchronization barrier
    }

    // Initialize global objects
    let cores = std::thread::available_parallelism().unwrap().get() as usize;
    let tlog2 = cores.ilog2() as usize;
    let tids = 1 << tlog2;
    let delta = AtomicRewrites::default(); // delta rewrite counter
    let rlens = (0 .. tids).map(|_| AtomicUsize::new(0)).collect::<Vec<_>>();
    let share = (0 .. SHARE_LIMIT * tids).map(|_| Default::default()).collect::<Vec<_>>();
    let total = AtomicUsize::new(0); // sum of redex bag length
    let barry = Arc::new(Barrier::new(tids)); // global barrier

    // Perform parallel reductions
    std::thread::scope(|s| {
      for net in self.fork(tids) {
        let mut ctx = ThreadContext {
          tid: net.tid,
          tick: 0,
          net,
          tlog2,
          delta: &delta,
          share: &share,
          rlens: &rlens,
          total: &total,
          barry: Arc::clone(&barry),
        };
        thread::Builder::new().name(format!("t{:02x?}", ctx.net.tid)).spawn_scoped(s, move || main(&mut ctx)).unwrap();
      }
    });

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
        ctx.net.reduce(LOCAL_LIMIT);
        if count(ctx) == 0 {
          break;
        }
        let tlog2 = ctx.tlog2;
        split(ctx, tlog2);
        ctx.tick += 1;
      }
    }

    // Count total redexes (and populate 'rlens')
    #[inline(always)]
    fn count<M: Mode>(ctx: &mut ThreadContext<M>) -> usize {
      ctx.barry.wait();
      ctx.total.store(0, Relaxed);
      ctx.barry.wait();
      ctx.rlens[ctx.tid].store(ctx.net.redexes.slow.len(), Relaxed);
      ctx.total.fetch_add(ctx.net.redexes.len(), Relaxed);
      ctx.barry.wait();
      ctx.total.load(Relaxed)
    }

    // Share redexes with target thread
    #[inline(always)]
    fn split<M: Mode>(ctx: &mut ThreadContext<M>, plog2: usize) {
      unsafe {
        let side = (ctx.tid >> (plog2 - 1 - (ctx.tick % plog2))) & 1;
        let shift = (1 << (plog2 - 1)) >> (ctx.tick % plog2);
        let a_tid = ctx.tid;
        let b_tid = if side == 1 { a_tid - shift } else { a_tid + shift };
        let a_len = ctx.net.redexes.slow.len();
        let b_len = ctx.rlens[b_tid].load(Relaxed);
        let send = if a_len > b_len { (a_len - b_len) / 2 } else { 0 };
        let recv = if b_len > a_len { (b_len - a_len) / 2 } else { 0 };
        let send = std::cmp::min(send, SHARE_LIMIT);
        let recv = std::cmp::min(recv, SHARE_LIMIT);
        for i in 0 .. send {
          let init = a_len - send * 2;
          let rdx0 = ctx.net.redexes.slow[init + i * 2 + 0].clone();
          let rdx1 = ctx.net.redexes.slow[init + i * 2 + 1].clone();
          //let init = 0;
          //let ref0 = ctx.net.redexes.get_unchecked_mut(init + i * 2 + 0);
          //let rdx0 = *ref0;
          //*ref0    = (Ptr(0), Ptr(0));
          //let ref1 = ctx.net.redexes.get_unchecked_mut(init + i * 2 + 1);
          //let rdx1 = *ref1;
          //*ref1    = (Ptr(0), Ptr(0));
          let targ = ctx.share.get_unchecked(b_tid * SHARE_LIMIT + i);
          ctx.net.redexes.slow[init + i] = rdx0;
          targ.0.store(rdx1.0.0, Relaxed);
          targ.1.store(rdx1.1.0, Relaxed);
        }
        ctx.net.redexes.slow.truncate(a_len - send);
        ctx.barry.wait();
        for i in 0 .. recv {
          let got = ctx.share.get_unchecked(a_tid * SHARE_LIMIT + i);
          ctx.net.redexes.slow.push((Port(got.0.load(Relaxed)), Port(got.1.load(Relaxed))));
        }
      }
    }
  }
}
