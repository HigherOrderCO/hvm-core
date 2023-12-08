// An efficient Interaction Combinator runtime
// ===========================================
// This file implements an efficient interaction combinator runtime. Nodes are represented by 2 aux
// ports (P1, P2), with the main port (P1) omitted. A separate vector, 'rdex', holds main ports,
// and, thus, tracks active pairs that can be reduced in parallel. Pointers are unboxed, meaning
// that ERAs, NUMs and REFs don't use any additional space. REFs lazily expand to closed nets when
// they interact with nodes, and are cleared when they interact with ERAs, allowing for constant
// space evaluation of recursive functions on Scott encoded datatypes.

use st3::{lifo::{Stealer, Worker}, StealError};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering, AtomicBool, AtomicIsize};
use std::sync::{Arc, Barrier};
use std::thread;
use crate::u60;

pub const WORKER_QUEUE_CAPACITY : usize = 1 << 22;

type Redex = (Ptr, Ptr);

pub type Tag  = u8;
pub type Lab  = u32;
pub type Loc  = u32;
pub type Val  = u64;
pub type AVal = AtomicU64;

// Core terms.
pub const VR1: Tag = 0x0; // Variable to aux port 1
pub const VR2: Tag = 0x1; // Variable to aux port 2
pub const RD1: Tag = 0x2; // Redirect to aux port 1
pub const RD2: Tag = 0x3; // Redirect to aux port 2
pub const REF: Tag = 0x4; // Lazy closed net
pub const ERA: Tag = 0x5; // Unboxed eraser
pub const NUM: Tag = 0x6; // Unboxed number
pub const OP2: Tag = 0x7; // Binary numeric operation
pub const OP1: Tag = 0x8; // Unary numeric operation
pub const MAT: Tag = 0x9; // Numeric pattern-matching
pub const LAM: Tag = 0xA; // Main port of lam node
pub const TUP: Tag = 0xB; // Main port of tup node
pub const DUP: Tag = 0xC; // Main port of dup node
pub const END: Tag = 0xE; // Last pointer tag

// Numeric operations.
pub const ADD: Lab = 0x00; // addition
pub const SUB: Lab = 0x01; // subtraction
pub const MUL: Lab = 0x02; // multiplication
pub const DIV: Lab = 0x03; // division
pub const MOD: Lab = 0x04; // modulus
pub const EQ : Lab = 0x05; // equal-to
pub const NE : Lab = 0x06; // not-equal-to
pub const LT : Lab = 0x07; // less-than
pub const GT : Lab = 0x08; // greater-than
pub const LTE: Lab = 0x09; // less-than-or-equal
pub const GTE: Lab = 0x0A; // greater-than-or-equal
pub const AND: Lab = 0x0B; // logical-and
pub const OR : Lab = 0x0C; // logical-or
pub const XOR: Lab = 0x0D; // logical-xor
pub const LSH: Lab = 0x0E; // left-shift
pub const RSH: Lab = 0x0F; // right-shift
pub const NOT: Lab = 0x10; // logical-not

pub const ERAS: Ptr = Ptr::new(ERA, 0, 0);
pub const ROOT: Ptr = Ptr::new(VR2, 0, 0);
pub const NULL: Ptr = Ptr(0x0000_0000_0000_0000);
pub const GONE: Ptr = Ptr(0xFFFF_FFFF_FFFF_FFEF);
pub const LOCK: Ptr = Ptr(0xFFFF_FFFF_FFFF_FFFF); // if last digit is F it will be seen as a CTR

// An auxiliary port.
pub type Port = Val;
pub const P1: Port = 0;
pub const P2: Port = 1;

// A tagged pointer.
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Hash)]
pub struct Ptr(pub Val);

// An atomic tagged pointer.
pub struct APtr(pub AVal);

// A target pointer, with implied ownership.
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Hash)]
pub enum Trg {
  Dir(Ptr), // we don't own the pointer, so we point to its location
  Ptr(Ptr), // we own the pointer, so we store it directly
}

// The global node buffer.
pub type Data = [(APtr, APtr)];

// A handy wrapper around Data.
pub struct Heap<'a> {
  pub data: &'a Data,
}

// Rewrite counter.
pub struct Rewrites {
  pub anni: usize, // anni rewrites
  pub comm: usize, // comm rewrites
  pub eras: usize, // eras rewrites
  pub dref: usize, // dref rewrites
  pub oper: usize, // oper rewrites
}

// Rewrite counter, atomic.
pub struct AtomicRewrites {
  pub anni: AtomicUsize, // anni rewrites
  pub comm: AtomicUsize, // comm rewrites
  pub eras: AtomicUsize, // eras rewrites
  pub dref: AtomicUsize, // dref rewrites
  pub oper: AtomicUsize, // oper rewrites
}

// An allocation area delimiter
pub struct Area {
  pub init: usize, // first allocation index
  pub size: usize, // total nodes in area
}

// A interaction combinator net.
pub struct Net<'a> {
  pub tid : usize, // thread id
  pub tids: usize, // thread count
  pub heap: Heap<'a>, // nodes
  pub rdex: Worker<Redex>, // redexes
  pub locs: Vec<Loc>,
  pub area: Area, // allocation area
  pub next: usize, // next allocation index within area
  pub rwts: Rewrites, // rewrite count
}

// A compact closed net, used for dereferences.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Def {
  pub safe: bool,
  pub rdex: Vec<Redex>,
  pub node: Vec<(Ptr, Ptr)>,
}

// A map of id to definitions (closed nets).
pub struct Book {
  pub defs: HashMap<Val, Def, nohash_hasher::BuildNoHashHasher<Val>>,
}

impl Ptr {
  #[inline(always)]
  pub const fn new(tag: Tag, lab: Lab, loc: Loc) -> Self {
    Ptr(((loc as Val) << 32) | ((lab as Val) << 4) | (tag as Val))
  }

  #[inline(always)]
  pub const fn big(tag: Tag, val: Val) -> Self {
    Ptr((val << 4) | (tag as Val))
  }

  #[inline(always)]
  pub const fn tag(&self) -> Tag {
    (self.0 & 0xF) as Tag
  }

  #[inline(always)]
  pub const fn lab(&self) -> Lab {
    (self.0 as Lab) >> 4
  }

  #[inline(always)]
  pub const fn loc(&self) -> Loc {
    (self.0 >> 32) as Loc
  }

  #[inline(always)]
  pub const fn val(&self) -> Val {
    self.0 >> 4
  }

  #[inline(always)]
  pub fn is_nil(&self) -> bool {
    return self.0 == 0;
  }

  #[inline(always)]
  pub fn is_var(&self) -> bool {
    return matches!(self.tag(), VR1..=VR2) && !self.is_nil();
  }

  #[inline(always)]
  pub fn is_red(&self) -> bool {
    return matches!(self.tag(), RD1..=RD2) && !self.is_nil();
  }

  #[inline(always)]
  pub fn is_era(&self) -> bool {
    return matches!(self.tag(), ERA);
  }

  #[inline(always)]
  pub fn is_ctr(&self) -> bool {
    return matches!(self.tag(), LAM..=END);
  }

  #[inline(always)]
  pub fn is_dup(&self) -> bool {
    return matches!(self.tag(), DUP);
  }

  #[inline(always)]
  pub fn is_ref(&self) -> bool {
    return matches!(self.tag(), REF);
  }

  #[inline(always)]
  pub fn is_pri(&self) -> bool {
    return matches!(self.tag(), REF..=END);
  }

  #[inline(always)]
  pub fn is_num(&self) -> bool {
    return matches!(self.tag(), NUM);
  }

  #[inline(always)]
  pub fn is_op1(&self) -> bool {
    return matches!(self.tag(), OP1);
  }

  #[inline(always)]
  pub fn is_op2(&self) -> bool {
    return matches!(self.tag(), OP2);
  }

  #[inline(always)]
  pub fn is_skp(&self) -> bool {
    return matches!(self.tag(), ERA | NUM | REF);
  }

  #[inline(always)]
  pub fn is_mat(&self) -> bool {
    return matches!(self.tag(), MAT);
  }

  #[inline(always)]
  pub fn is_nod(&self) -> bool {
    return matches!(self.tag(), OP2..=END);
  }

  #[inline(always)]
  pub fn has_loc(&self) -> bool {
    return matches!(self.tag(), VR1..=VR2 | OP2..=END);
  }

  #[inline(always)]
  pub fn redirect(&self) -> Ptr {
    return Ptr::new(self.tag() + RD2 - VR2, 0, self.loc());
  }

  #[inline(always)]
  pub fn unredirect(&self) -> Ptr {
    return Ptr::new(self.tag() + RD2 - VR2, 0, self.loc());
  }

  #[inline(always)]
  pub fn can_skip(a: Ptr, b: Ptr) -> bool {
    return matches!(a.tag(), ERA | REF) && matches!(b.tag(), ERA | REF);
  }
}

impl APtr {
  pub fn new(ptr: Ptr) -> Self {
    APtr(AtomicU64::new(ptr.0))
  }

  pub fn load(&self) -> Ptr {
    Ptr(self.0.load(Ordering::Relaxed))
  }

  pub fn store(&self, ptr: Ptr) {
    self.0.store(ptr.0, Ordering::Relaxed);
  }
}


impl Book {
  #[inline(always)]
  pub fn new() -> Self {
    Book {
      defs: HashMap::with_hasher(std::hash::BuildHasherDefault::default()),
    }
  }

  #[inline(always)]
  pub fn def(&mut self, name: Val, def: Def) {
    self.defs.insert(name, def);
  }

  #[inline(always)]
  pub fn get(&self, name: Val) -> Option<&Def> {
    self.defs.get(&name)
  }
}

impl Def {
  pub fn new() -> Self {
    Def {
      safe: true,
      rdex: vec![],
      node: vec![],
    }
  }
}

impl<'a> Heap<'a> {
  pub fn init(size: usize) -> Box<[(APtr, APtr)]> {
    let mut data = vec![];
    for _ in 0..size {
      data.push((APtr::new(NULL), APtr::new(NULL)));
    }
    return data.into_boxed_slice();
  }

  pub fn new(data: &'a Data) -> Self {
    Heap { data }
  }

  #[inline(always)]
  pub fn get(&self, index: Loc, port: Port) -> Ptr {
    unsafe {
      let node = self.data.get_unchecked(index as usize);
      if port == P1 {
        return node.0.load();
      } else {
        return node.1.load();
      }
    }
  }

  #[inline(always)]
  pub fn set(&self, index: Loc, port: Port, value: Ptr) {
    unsafe {
      let node = self.data.get_unchecked(index as usize);
      if port == P1 {
        node.0.store(value);
      } else {
        node.1.store(value);
      }
    }
  }

  #[inline(always)]
  pub fn cas(&self, index: Loc, port: Port, expected: Ptr, value: Ptr) -> Result<Ptr,Ptr> {
    unsafe {
      let node = self.data.get_unchecked(index as usize);
      let data = if port == P1 { &node.0.0 } else { &node.1.0 };
      let done = data.compare_exchange_weak(expected.0, value.0, Ordering::Relaxed, Ordering::Relaxed);
      return done.map(Ptr).map_err(Ptr);
    }
  }

  #[inline(always)]
  pub fn swap(&self, index: Loc, port: Port, value: Ptr) -> Ptr {
    unsafe {
      let node = self.data.get_unchecked(index as usize);
      let data = if port == P1 { &node.0.0 } else { &node.1.0 };
      return Ptr(data.swap(value.0, Ordering::Relaxed));
    }
  }

  #[inline(always)]
  pub fn get_root(&self) -> Ptr {
    return self.get(ROOT.loc(), P2);
  }

  #[inline(always)]
  pub fn set_root(&self, value: Ptr) {
    self.set(ROOT.loc(), P2, value);
  }
}

impl Rewrites {
  pub fn new() -> Self {
    Rewrites {
      anni: 0,
      comm: 0,
      eras: 0,
      dref: 0,
      oper: 0,
    }
  }

  pub fn add_to(&self, target: &AtomicRewrites) {
    target.anni.fetch_add(self.anni, Ordering::Relaxed);
    target.comm.fetch_add(self.comm, Ordering::Relaxed);
    target.eras.fetch_add(self.eras, Ordering::Relaxed);
    target.dref.fetch_add(self.dref, Ordering::Relaxed);
    target.oper.fetch_add(self.oper, Ordering::Relaxed);
  }

}

impl AtomicRewrites {
  pub fn new() -> Self {
    AtomicRewrites {
      anni: AtomicUsize::new(0),
      comm: AtomicUsize::new(0),
      eras: AtomicUsize::new(0),
      dref: AtomicUsize::new(0),
      oper: AtomicUsize::new(0),
    }
  }

  pub fn add_to(&self, target: &mut Rewrites) {
    target.anni += self.anni.load(Ordering::Relaxed);
    target.comm += self.comm.load(Ordering::Relaxed);
    target.eras += self.eras.load(Ordering::Relaxed);
    target.dref += self.dref.load(Ordering::Relaxed);
    target.oper += self.oper.load(Ordering::Relaxed);
  }
}

impl<'a> Net<'a> {
  // Creates an empty net with given heap data
  pub fn new(data: &'a Data) -> Self {
    Net {
      tid : 0,
      tids: 1,
      heap: Heap { data },
      rdex: Worker::new(WORKER_QUEUE_CAPACITY),
      locs: vec![0; 1 << 16],
      area: Area { init: 0, size: data.len() },
      next: 0,
      rwts: Rewrites::new(),
    }
  }

  // Creates an empty net with given heap data and worker
  pub fn new_with_worker(data: &'a Data, worker: Worker<Redex>) -> Self {
    Net {
      tid : 0,
      tids: 1,
      heap: Heap { data },
      rdex: worker,
      locs: vec![0; 1 << 16],
      area: Area { init: 0, size: data.len() },
      next: 0,
      rwts: Rewrites::new(),
    }
  }

  // Creates a net and boots from a REF.
  pub fn boot(&mut self, root_id: Val) {
    self.heap.set_root(Ptr::big(REF, root_id));
  }

  // Total rewrite count.
  pub fn rewrites(&self) -> usize {
    return self.rwts.anni + self.rwts.comm + self.rwts.eras + self.rwts.dref + self.rwts.oper;
  }

  #[inline(always)]
  pub fn alloc(&mut self) -> Loc {
    // On the first pass, just alloc without checking.
    // Note: we add 1 to avoid overwritting root.
    let index = if self.next < self.area.size - 1 {
      self.next += 1;
      self.area.init as Loc + self.next as Loc
    // On later passes, search for an available slot.
    } else {
      let limit = self.next + self.area.size;
      loop {
        self.next += 1;
        if self.next >= limit {
          panic!("out of memory");
        }
        let index = (self.area.init + self.next % self.area.size) as Loc;
        if self.heap.get(index, P1).is_nil() && self.heap.get(index, P2).is_nil() {
          break index;
        }
      }
    };
    self.heap.set(index, P1, LOCK);
    self.heap.set(index, P2, LOCK);
    index
  }

  // Gets a pointer's target.
  #[inline(always)]
  pub fn get_target(&self, ptr: Ptr) -> Ptr {
    self.heap.get(ptr.loc(), ptr.0 & 1)
  }

  // Sets a pointer's target.
  #[inline(always)]
  pub fn set_target(&mut self, ptr: Ptr, val: Ptr) {
    self.heap.set(ptr.loc(), ptr.0 & 1, val)
  }

  // Takes a pointer's target.
  #[inline(always)]
  pub fn swap_target(&self, ptr: Ptr, value: Ptr) -> Ptr {
    self.heap.swap(ptr.loc(), ptr.0 & 1, value)
  }

  // Takes a pointer's target.
  #[inline(always)]
  pub fn take_target(&self, ptr: Ptr) -> Ptr {
    loop {
      let got = self.heap.swap(ptr.loc(), ptr.0 & 1, LOCK);
      if got != LOCK && got != NULL {
        return got;
      }
    }
  }

  // Sets a pointer's target, using CAS.
  #[inline(always)]
  pub fn cas_target(&self, ptr: Ptr, expected: Ptr, value: Ptr) -> Result<Ptr,Ptr> {
    self.heap.cas(ptr.loc(), ptr.0 & 1, expected, value)
  }

  #[inline(always)]
  pub fn redux(&mut self, a: Ptr, b: Ptr) {
    if Ptr::can_skip(a, b) {
      self.rwts.eras += 1;
    } else {
      self.rdex.push((a, b)).expect("capacity");
    }
  }

  #[inline(always)]
  pub fn get(&self, a: Trg) -> Ptr {
    match a {
      Trg::Dir(dir) => self.get_target(dir),
      Trg::Ptr(ptr) => ptr,
    }
  }

  #[inline(always)]
  pub fn swap(&self, a: Trg, val: Ptr) -> Ptr {
    match a {
      Trg::Dir(dir) => self.swap_target(dir, val),
      Trg::Ptr(ptr) => ptr,
    }
  }

  // Links two pointers, forming a new wire. Assumes ownership.
  #[inline(always)]
  pub fn link(&mut self, a_ptr: Ptr, b_ptr: Ptr) {
    if a_ptr.is_pri() && b_ptr.is_pri() {
      return self.redux(a_ptr, b_ptr);
    } else {
      self.linker(a_ptr, b_ptr);
      self.linker(b_ptr, a_ptr);
    }
  }

  // Given two locations, links both stored pointers, atomically.
  #[inline(always)]
  pub fn atomic_link(&mut self, a_dir: Ptr, b_dir: Ptr) {
    //println!("link {:016x} {:016x}", a_dir.0, b_dir.0);
    let a_ptr = self.take_target(a_dir);
    let b_ptr = self.take_target(b_dir);
    if a_ptr.is_pri() && b_ptr.is_pri() {
      self.set_target(a_dir, NULL);
      self.set_target(b_dir, NULL);
      return self.redux(a_ptr, b_ptr);
    } else {
      self.atomic_linker(a_ptr, a_dir, b_ptr);
      self.atomic_linker(b_ptr, b_dir, a_ptr);
    }
  }

  // Given a location, link the pointer stored to another pointer, atomically.
  #[inline(always)]
  pub fn half_atomic_link(&mut self, a_dir: Ptr, b_ptr: Ptr) {
    let a_ptr = self.take_target(a_dir);
    if a_ptr.is_pri() && b_ptr.is_pri() {
      self.set_target(a_dir, NULL);
      return self.redux(a_ptr, b_ptr);
    } else {
      self.atomic_linker(a_ptr, a_dir, b_ptr);
      self.linker(b_ptr, a_ptr);
    }
  }

  // When two threads interfere, uses the lock-free link algorithm described on the 'paper/'.
  #[inline(always)]
  pub fn linker(&mut self, a_ptr: Ptr, b_ptr: Ptr) {
    if a_ptr.is_var() {
      self.set_target(a_ptr, b_ptr);
    }
  }

  // When two threads interfere, uses the lock-free link algorithm described on the 'paper/'.
  #[inline(always)]
  pub fn atomic_linker(&mut self, a_ptr: Ptr, a_dir: Ptr, b_ptr: Ptr) {
    // If 'a_ptr' is a var...
    if a_ptr.is_var() {
      let got = self.cas_target(a_ptr, a_dir, b_ptr);
      // Attempts to link using a compare-and-swap.
      if got.is_ok() {
        self.set_target(a_dir, NULL);
      // If the CAS failed, resolve by using redirections.
      } else {
        //println!("[{:04x}] cas fail {:016x}", self.tid, got.unwrap_err().0);
        if b_ptr.is_var() {
          self.set_target(a_dir, b_ptr.redirect());
          //self.atomic_linker_var(a_ptr, a_dir, b_ptr);
        } else if b_ptr.is_pri() {
          self.set_target(a_dir, b_ptr);
          self.atomic_linker_pri(a_ptr, a_dir, b_ptr);
        } else {
          todo!();
        }
      }
    } else {
      self.set_target(a_dir, NULL);
    }
  }

  // Atomic linker for when 'b_ptr' is a principal port.
  pub fn atomic_linker_pri(&mut self, mut a_ptr: Ptr, a_dir: Ptr, b_ptr: Ptr) {
    loop {
      // Peek the target, which may not be owned by us.
      let mut t_dir = a_ptr;
      let mut t_ptr = self.get_target(t_dir);
      // If target is a redirection, we own it. Clear and move forward.
      if t_ptr.is_red() {
        self.set_target(t_dir, NULL);
        a_ptr = t_ptr;
        continue;
      }
      // If target is a variable, we don't own it. Try replacing it.
      if t_ptr.is_var() {
        if self.cas_target(t_dir, t_ptr, b_ptr).is_ok() {
          //println!("[{:04x}] var", self.tid);
          // Clear source location.
          self.set_target(a_dir, NULL);
          // Collect the orphaned backward path.
          t_dir = t_ptr;
          t_ptr = self.get_target(t_ptr);
          while t_ptr.is_red() {
            self.swap_target(t_dir, NULL);
            t_dir = t_ptr;
            t_ptr = self.get_target(t_dir);
          }
          return;
        }
        // If the CAS failed, the var changed, so we try again.
        continue;
      }
      // If it is a node, two threads will reach this branch.
      if t_ptr.is_pri() || t_ptr == GONE {
        // Sort references, to avoid deadlocks.
        let x_dir = if a_dir < t_dir { a_dir } else { t_dir };
        let y_dir = if a_dir < t_dir { t_dir } else { a_dir };
        // Swap first reference by GONE placeholder.
        let x_ptr = self.swap_target(x_dir, GONE);
        // First to arrive creates a redex.
        if x_ptr != GONE {
          //println!("[{:04x}] fst {:016x}", self.tid, x_ptr.0);
          let y_ptr = self.swap_target(y_dir, GONE);
          self.redux(x_ptr, y_ptr);
          return;
        // Second to arrive clears up the memory.
        } else {
          //println!("[{:04x}] snd", self.tid);
          self.swap_target(x_dir, NULL);
          while self.cas_target(y_dir, GONE, NULL).is_err() {};
          return;
        }
      }
      // If it is taken, we wait.
      if t_ptr == LOCK {
        continue;
      }
      if t_ptr == NULL {
        continue;
      }
      // Shouldn't be reached.
      //println!("[{:04x}] {:016x} | {:016x} {:016x} {:016x}", self.tid, t_ptr.0, a_dir.0, a_ptr.0, b_ptr.0);
      unreachable!()
    }
  }

  // Atomic linker for when 'b_ptr' is an aux port.
  pub fn atomic_linker_var(&mut self, _a_ptr: Ptr, _a_dir: Ptr, b_ptr: Ptr) {
    loop {
      let ste_dir = b_ptr;
      let ste_ptr = self.get_target(ste_dir);
      if ste_ptr.is_var() {
        let trg_dir = ste_ptr;
        let trg_ptr = self.get_target(trg_dir);
        if trg_ptr.is_red() {
          let neo_ptr = trg_ptr.unredirect();
          if self.cas_target(ste_dir, ste_ptr, neo_ptr).is_ok() {
            self.swap_target(trg_dir, NULL);
            continue;
          }
        }
      }
      break;
    }
  }

  // Links two targets, using atomics when necessary, based on implied ownership.
  #[inline(always)]
  pub fn safe_link(&mut self, a: Trg, b: Trg) {
    match (a, b) {
      (Trg::Dir(a_dir), Trg::Dir(b_dir)) => self.atomic_link(a_dir, b_dir),
      (Trg::Dir(a_dir), Trg::Ptr(b_ptr)) => self.half_atomic_link(a_dir, b_ptr),
      (Trg::Ptr(a_ptr), Trg::Dir(b_dir)) => self.half_atomic_link(b_dir, a_ptr),
      (Trg::Ptr(a_ptr), Trg::Ptr(b_ptr)) => self.link(a_ptr, b_ptr),
    }
  }

  // Performs an interaction over a redex.
  #[inline(always)]
  pub fn interact(&mut self, book: &Book, a: Ptr, b: Ptr) {
    match (a.tag(), b.tag()) {
      (REF   , OP2..) => self.call(book, a, b),
      (OP2.. , REF  ) => self.call(book, b, a),
      (LAM.. , LAM..) if a.lab() == b.lab() => self.anni(a, b),
      (LAM.. , LAM..) => self.comm(a, b),
      (LAM.. , ERA  ) => self.era2(a),
      (ERA   , LAM..) => self.era2(b),
      (REF   , ERA  ) => self.rwts.eras += 1,
      (ERA   , REF  ) => self.rwts.eras += 1,
      (REF   , NUM  ) => self.rwts.eras += 1,
      (NUM   , REF  ) => self.rwts.eras += 1,
      (ERA   , ERA  ) => self.rwts.eras += 1,
      (LAM.. , NUM  ) => self.copy(a, b),
      (NUM   , LAM..) => self.copy(b, a),
      (NUM   , ERA  ) => self.rwts.eras += 1,
      (ERA   , NUM  ) => self.rwts.eras += 1,
      (NUM   , NUM  ) => self.rwts.eras += 1,
      (OP2   , NUM  ) => self.op2n(a, b),
      (NUM   , OP2  ) => self.op2n(b, a),
      (OP1   , NUM  ) => self.op1n(a, b),
      (NUM   , OP1  ) => self.op1n(b, a),
      (OP2   , LAM..) => self.comm(a, b),
      (LAM.. , OP2  ) => self.comm(b, a),
      (OP1   , LAM..) => self.pass(a, b),
      (LAM.. , OP1  ) => self.pass(b, a),
      (OP2   , ERA  ) => self.era2(a),
      (ERA   , OP2  ) => self.era2(b),
      (OP1   , ERA  ) => self.era1(a),
      (ERA   , OP1  ) => self.era1(b),
      (MAT   , NUM  ) => self.mtch(a, b),
      (NUM   , MAT  ) => self.mtch(b, a),
      (MAT   , LAM..) => self.comm(a, b),
      (LAM.. , MAT  ) => self.comm(b, a),
      (MAT   , ERA  ) => self.era2(a),
      (ERA   , MAT  ) => self.era2(b),
      _               => { println!("{:016x} {:016x}", a.0, b.0); unreachable!() },
    };
  }

  pub fn anni(&mut self, a: Ptr, b: Ptr) {
    self.rwts.anni += 1;
    let a1 = Ptr::new(VR1, 0, a.loc());
    let b1 = Ptr::new(VR1, 0, b.loc());
    self.atomic_link(a1, b1);
    let a2 = Ptr::new(VR2, 0, a.loc());
    let b2 = Ptr::new(VR2, 0, b.loc());
    self.atomic_link(a2, b2);
  }

  pub fn comm(&mut self, a: Ptr, b: Ptr) {
    self.rwts.comm += 1;
    let loc0 = self.alloc();
    let loc1 = self.alloc();
    let loc2 = self.alloc();
    let loc3 = self.alloc();
    self.heap.set(loc0, P1, Ptr::new(VR1, 0, loc2));
    self.heap.set(loc0, P2, Ptr::new(VR1, 0, loc3));
    self.heap.set(loc1, P1, Ptr::new(VR2, 0, loc2));
    self.heap.set(loc1, P2, Ptr::new(VR2, 0, loc3));
    self.heap.set(loc2, P1, Ptr::new(VR1, 0, loc0));
    self.heap.set(loc2, P2, Ptr::new(VR1, 0, loc1));
    self.heap.set(loc3, P1, Ptr::new(VR2, 0, loc0));
    self.heap.set(loc3, P2, Ptr::new(VR2, 0, loc1));
    let a1 = Ptr::new(VR1, 0, a.loc());
    self.half_atomic_link(a1, Ptr::new(b.tag(), b.lab(), loc0));
    let b1 = Ptr::new(VR1, 0, b.loc());
    self.half_atomic_link(b1, Ptr::new(a.tag(), a.lab(), loc2));
    let a2 = Ptr::new(VR2, 0, a.loc());
    self.half_atomic_link(a2, Ptr::new(b.tag(), b.lab(), loc1));
    let b2 = Ptr::new(VR2, 0, b.loc());
    self.half_atomic_link(b2, Ptr::new(a.tag(), a.lab(), loc3));
  }

  pub fn era2(&mut self, a: Ptr) {
    self.rwts.eras += 1;
    let a1 = Ptr::new(VR1, 0, a.loc());
    self.half_atomic_link(a1, ERAS);
    let a2 = Ptr::new(VR2, 0, a.loc());
    self.half_atomic_link(a2, ERAS);
  }

  pub fn era1(&mut self, a: Ptr) {
    self.rwts.eras += 1;
    let a2 = Ptr::new(VR2, 0, a.loc());
    self.half_atomic_link(a2, ERAS);
  }

  pub fn pass(&mut self, a: Ptr, b: Ptr) {
    self.rwts.comm += 1;
    let loc0 = self.alloc();
    let loc1 = self.alloc();
    let loc2 = self.alloc();
    self.heap.set(loc0, P1, Ptr::new(VR2, 0, loc1));
    self.heap.set(loc0, P2, Ptr::new(VR2, 0, loc2));
    self.heap.set(loc1, P1, self.heap.get(a.loc(), P1));
    self.heap.set(loc1, P2, Ptr::new(VR1, 0, loc0));
    self.heap.set(loc2, P1, self.heap.get(a.loc(), P1));
    self.heap.set(loc2, P2, Ptr::new(VR2, 0, loc0));
    let a2 = Ptr::new(VR2, 0, a.loc());
    self.half_atomic_link(a2, Ptr::new(b.tag(), b.lab(), loc0));
    let b1 = Ptr::new(VR1, 0, b.loc());
    self.half_atomic_link(b1, Ptr::new(a.tag(), b.lab(), loc1));
    let b2 = Ptr::new(VR2, 0, b.loc());
    self.half_atomic_link(b2, Ptr::new(a.tag(), b.lab(), loc2));
  }

  pub fn copy(&mut self, a: Ptr, b: Ptr) {
    self.rwts.comm += 1;
    let a1 = Ptr::new(VR1, 0, a.loc());
    self.half_atomic_link(a1, b);
    let a2 = Ptr::new(VR2, 0, a.loc());
    self.half_atomic_link(a2, b);
  }

  pub fn mtch(&mut self, a: Ptr, b: Ptr) {
    self.rwts.oper += 1;
    let a1 = Ptr::new(VR1, 0, a.loc()); // branch
    let a2 = Ptr::new(VR2, 0, a.loc()); // return
    if b.val() == 0 {
      let loc0 = self.alloc();
      self.heap.set(loc0, P2, ERAS);
      self.half_atomic_link(a1, Ptr::new(LAM, 0, loc0));
      self.half_atomic_link(a2, Ptr::new(VR1, 0, loc0));
    } else {
      let loc0 = self.alloc();
      let loc1 = self.alloc();
      self.heap.set(loc0, P1, ERAS);
      self.heap.set(loc0, P2, Ptr::new(LAM, 0, loc1));
      self.heap.set(loc1, P1, Ptr::big(NUM, b.val() - 1));
      self.half_atomic_link(a1, Ptr::new(LAM, 0, loc0));
      self.half_atomic_link(a2, Ptr::new(VR2, 0, loc1));
    }
  }

  pub fn op2n(&mut self, a: Ptr, b: Ptr) {
    self.rwts.oper += 1;
    let loc0 = self.alloc();
    let a1 = Ptr::new(VR1, 0, a.loc());
    let a2 = Ptr::new(VR2, 0, a.loc());
    self.heap.set(loc0, P1, b);
    self.half_atomic_link(a2, Ptr::new(VR2, 0, loc0));
    self.half_atomic_link(a1, Ptr::new(OP1, a.lab(), loc0));
  }

  pub fn op1n(&mut self, a: Ptr, b: Ptr) {
    self.rwts.oper += 1;
    let op = a.lab();
    let v0 = self.heap.get(a.loc(), P1).val();
    let v1 = b.val();
    let v2 = self.op(op, v0, v1);
    let a2 = Ptr::new(VR2, 0, a.loc());
    self.half_atomic_link(a2, Ptr::big(NUM, v2));
  }

  #[inline(always)]
  pub fn op(&self, op: Lab, a: Val, b: Val) -> Val {
    match op {
      ADD => { u60::add(a, b) }
      SUB => { u60::sub(a, b) }
      MUL => { u60::mul(a, b) }
      DIV => { u60::div(a, b) }
      MOD => { u60::rem(a, b) }
      EQ  => { u60::eq(a, b) }
      NE  => { u60::ne(a, b) }
      LT  => { u60::lt(a, b) }
      GT  => { u60::gt(a, b) }
      LTE => { u60::lte(a, b) }
      GTE => { u60::gte(a, b) }
      AND => { u60::and(a, b) }
      OR  => { u60::or(a, b) }
      XOR => { u60::xor(a, b) }
      NOT => { u60::not(a) }
      LSH => { u60::lsh(a, b) }
      RSH => { u60::rsh(a, b) }
      _   => { unreachable!() }
    }
  }

  // Expands a closed net.
  #[inline(always)]
  pub fn call(&mut self, book: &Book, ptr: Ptr, trg: Ptr) {
    self.rwts.dref += 1;
    let mut ptr = ptr;
    // FIXME: change "while" to "if" once lang prevents refs from returning refs
    if ptr.is_ref() {
      // Intercepts with a native function, if available.
      if self.call_native(book, ptr, trg) {
        return;
      }
      // Load the closed net.
      //println!("{:08x?}", book.defs);
      //println!("{:08x} {}", ptr.0, crate::ast::val_to_name(ptr.val()));
      let got = book.get(ptr.val()).unwrap();
      if got.safe && trg.is_dup() {
        return self.copy(trg, ptr);
      } else if got.node.len() > 0 {
        let len = got.node.len() - 1;
        // Allocate space.
        for i in 0 .. len {
          *unsafe { self.locs.get_unchecked_mut(1 + i) } = self.alloc();
        }
        // Load nodes, adjusted.
        for i in 0 .. len {
          let p1 = self.adjust(unsafe { got.node.get_unchecked(1 + i) }.0);
          let p2 = self.adjust(unsafe { got.node.get_unchecked(1 + i) }.1);
          let lc = *unsafe { self.locs.get_unchecked(1 + i) };
          self.heap.set(lc, P1, p1);
          self.heap.set(lc, P2, p2);
        }
        // Load redexes, adjusted.
        for r in &got.rdex {
          let p1 = self.adjust(r.0);
          let p2 = self.adjust(r.1);
          self.rdex.push((p1, p2)).expect("capacity");
        }
        // Load root, adjusted.
        ptr = self.adjust(got.node[0].1);
      }
    }
    self.link(ptr, trg);
  }

  // Adjusts dereferenced pointer locations.
  #[inline(always)]
  fn adjust(&self, ptr: Ptr) -> Ptr {
    if ptr.has_loc() {
      return Ptr::new(ptr.tag(), ptr.lab(), *unsafe { self.locs.get_unchecked(ptr.loc() as usize) });
    } else {
      return ptr;
    }
  }

  // Reduces all redexes.
  #[inline(always)]
  pub fn reduce(&mut self, book: &Book, limit: usize) -> usize {
    let mut count = 0;
    while let Some((a, b)) = self.rdex.pop() {
      //if !a.is_nil() && !b.is_nil() {
        self.interact(book, a, b);

        count += 1;
        if count >= limit {
          break;
        }
      //}
    }
    return count;
  }

  // Expands heads.
  #[inline(always)]
  pub fn expand(&mut self, book: &Book) {
    fn go(net: &mut Net, book: &Book, dir: Ptr, len: usize, key: usize) {
      //println!("[{:04x}] expand dir: {:016x}", net.tid, dir.0);
      let ptr = net.get_target(dir);
      if ptr.is_ctr() {
        if len >= net.tids || key % 2 == 0 {
          go(net, book, Ptr::new(VR1, 0, ptr.loc()), len * 2, key / 2);
        }
        if len >= net.tids || key % 2 == 1 {
          go(net, book, Ptr::new(VR2, 0, ptr.loc()), len * 2, key / 2);
        }
      } else if ptr.is_ref() {
        let got = net.swap_target(dir, LOCK);
        if got != LOCK {
          //println!("[{:08x}] expand {:08x}", net.tid, dir.0);
          net.call(book, ptr, dir);
        }
      }
    }
    return go(self, book, ROOT, 1, self.tid);
  }

  // Reduce a net to normal form.
  pub fn normal(&mut self, book: &Book) -> std::time::Duration {
    let start_time = std::time::Instant::now();
    self.expand(book);
    while !self.rdex.is_empty() {
      self.reduce(book, usize::MAX);
      self.expand(book);
    }
    start_time.elapsed()
  }

  // Forks into child threads, returning a Net for the (tid/tids)'th thread.
  pub fn fork(&self, tid: usize, tids: usize, worker: Worker<Redex>) -> Self {
    let mut net = Net::new_with_worker(&self.heap.data, worker);
    net.tid  = tid;
    net.tids = tids;
    net.area = Area {
      init: self.heap.data.len() * tid / tids,
      size: self.heap.data.len() / tids,
    };
    if tid == 0 {
      net.next = self.next;
    }
    return net;
  }

  // Evaluates a term to normal form in parallel
  pub fn parallel_normal(&mut self, book: &Book) -> std::time::Duration {
    const LOCAL_LIMIT : usize = 1 << 18; // max local rewrites per epoch

    // Local thread context
    struct ThreadContext<'a> {
      tid: usize, // thread id
      tids: usize, // thread count
      net: Net<'a>, // thread's own net object
      book: &'a Book, // definition book
      delta: &'a AtomicRewrites, // global delta rewrites
      workers_with_non_empty_queues: &'a AtomicIsize, // how many workers have non-empty queues
      barrier: &'a Barrier, // synchronization barrier
      stealers: Vec<Stealer<Redex>>, // stealers
      next_stealer_idx: usize, // next stealer to try
      last_frame_is_not_empty: bool, // redex count of this worker, the last time count() was called
    }

    // Initialize global objects
    let cores = std::thread::available_parallelism().unwrap().get() as usize;
    let tlog2 = cores.ilog2() as usize;
    let tids  = 1 << tlog2;
    let delta = AtomicRewrites::new(); // delta rewrite counter
    let workers_with_non_empty_queues = AtomicIsize::new(0);
    let barrier = Arc::new(Barrier::new(tids)); // global barrier

    let worker_count = tids;
    let workers = (0 .. worker_count).map(|_| Worker::<Redex>::new(WORKER_QUEUE_CAPACITY)).collect::<Vec<_>>();
    let stealers = workers.iter().map(|w| w.stealer()).collect::<Vec<_>>();

    let worker_redex_counts = (0 .. worker_count).map(|_| std::cell::Cell::new(0)).collect::<Vec<_>>();

    // Initialize worker queues by distributing redexes evenly
    let mut workers_iter = workers.iter().zip(&worker_redex_counts).cycle();
    while let Some(redex) = self.rdex.pop() {
      let (worker, worker_redex_count) = workers_iter.next().unwrap();

      worker.push(redex).expect("capacity");
      worker_redex_count.set(worker_redex_count.get() + 1); // increment worker redex count
    }

    // Set `workers_with_non_empty_queues` to the number of workers with non-empty queues
    for worker in &workers {
      let val = !worker.is_empty() as isize;
      workers_with_non_empty_queues.fetch_add(val, Ordering::Relaxed);
    }

    let thread_contexts = workers.into_iter().enumerate().map(|(tid, worker)| {
      ThreadContext {
        tid,
        tids,
        net: self.fork(tid, tids, worker),
        book: &book,
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
    }).collect::<Vec<_>>();

    // Perform parallel reductions
    let start_time = std::time::Instant::now();
    std::thread::scope(move |s| {
      for (tid, mut ctx) in thread_contexts.into_iter().enumerate() {
        s.spawn(move || {
          main(&mut ctx);
        });
      }
    });
    let elapsed = start_time.elapsed();

    // Sum stats
    delta.add_to(&mut self.rwts);

    // Main reduction loop
    #[inline(always)]
    fn main(ctx: &mut ThreadContext) {
      loop {
        reduce(ctx);
        expand(ctx);
        if count(ctx) == 0 { break; }
      }
      ctx.net.rwts.add_to(ctx.delta);
    }

    // Reduce redexes locally, then share with target
    #[inline(always)]
    fn reduce(ctx: &mut ThreadContext) {
      loop {
        let _reduced = ctx.net.reduce(ctx.book, LOCAL_LIMIT);
        //println!("[{:04x}] reduced {}", ctx.tid, reduced);
        if count(ctx) == 0 {
          break;
        }

        // Steal redexes from other workers
        while {
          let stealer = unsafe { ctx.stealers.get_unchecked(ctx.next_stealer_idx) };
          ctx.next_stealer_idx = (ctx.next_stealer_idx + 1) % ctx.stealers.len();
          let res = stealer.steal(&ctx.net.rdex, |n| n / 2);
          matches!(res, Err(StealError::Busy))
        } {}
      }
    }

    // Expand head refs
    #[inline(always)]
    fn expand(ctx: &mut ThreadContext) {
      ctx.net.expand(ctx.book);
    }

    // Count how many workers have non-empty queues
    #[inline(always)]
    fn count(ctx: &mut ThreadContext) -> usize {
      // // This was the original code, but it's not as fast as the code below.
      // ctx.barry.wait();
      // ctx.workers_with_non_empty_queues.store(0, Ordering::Relaxed);
      // ctx.barry.wait();
      // let cur_frame_val = !ctx.net.rdex.is_empty() as isize;
      // ctx.rlens[ctx.tid].store(cur_frame_val as _, Ordering::Relaxed);
      // ctx.workers_with_non_empty_queues.fetch_add(cur_frame_val, Ordering::Relaxed);
      // ctx.barry.wait();
      // ctx.workers_with_non_empty_queues.load(Ordering::Relaxed) as usize

      // // This is a bit faster (2 barriers instead of 3), but we're iterating
      // // over the entire array of `rlens``, which could be slow on a manycore CPU.
      // ctx.barry.wait();
      // ctx.rlens[ctx.tid].store(!ctx.net.rdex.is_empty() as _, Ordering::Relaxed);
      // ctx.barry.wait();
      // ctx.rlens.iter().map(|x| x.load(Ordering::Relaxed)).sum()

      // This is the fastest approach (2 barriers instead of 3), and without iteration
      let cur_frame_val = !ctx.net.rdex.is_empty();
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
