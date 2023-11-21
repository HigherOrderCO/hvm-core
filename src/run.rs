// An efficient Interaction Combinator runtime
// ===========================================
// This file implements an efficient interaction combinator runtime. Nodes are represented by 2 aux
// ports (P1, P2), with the main port (P1) omitted. A separate vector, 'rdex', holds main ports,
// and, thus, tracks active pairs that can be reduced in parallel. Pointers are unboxed, meaning
// that ERAs, NUMs and REFs don't use any additional space. REFs lazily expand to closed nets when
// they interact with nodes, and are cleared when they interact with ERAs, allowing for constant
// space evaluation of recursive functions on Scott encoded datatypes.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

pub type Tag  = u8;
pub type Val  = u32;
pub type AVal = AtomicU32;

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
pub const CT0: Tag = 0xA; // Main port of con node, label 0
pub const CT1: Tag = 0xB; // Main port of con node, label 1
pub const CT2: Tag = 0xC; // Main port of con node, label 2
pub const CT3: Tag = 0xD; // Main port of con node, label 3
pub const CT4: Tag = 0xE; // Main port of con node, label 4
pub const CT5: Tag = 0xF; // Main port of con node, label 5

// Numeric operations.
pub const USE: Tag = 0x0; // set-next-op
pub const ADD: Tag = 0x1; // addition
pub const SUB: Tag = 0x2; // subtraction
pub const MUL: Tag = 0x3; // multiplication
pub const DIV: Tag = 0x4; // division
pub const MOD: Tag = 0x5; // modulus
pub const EQ : Tag = 0x6; // equal-to
pub const NE : Tag = 0x7; // not-equal-to
pub const LT : Tag = 0x8; // less-than
pub const GT : Tag = 0x9; // greater-than
pub const AND: Tag = 0xA; // logical-and
pub const OR : Tag = 0xB; // logical-or
pub const XOR: Tag = 0xC; // logical-xor
pub const NOT: Tag = 0xD; // logical-not
pub const LSH: Tag = 0xE; // left-shift
pub const RSH: Tag = 0xF; // right-shift

pub const ERAS: Ptr   = Ptr::new(ERA, 0);
pub const ROOT: Ptr   = Ptr::new(VR2, 0);
pub const NULL: Ptr   = Ptr(0x0000_0000);

// An auxiliary port.
pub type Port = Val;
pub const P1: Port = 0;
pub const P2: Port = 1;

// A tagged pointer.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct Ptr(pub Val);

// An atomic tagged pointer.
#[repr(transparent)]
pub struct APtr(pub AVal);

pub struct Heap {
  pub data: Vec<(APtr, APtr)>,
}

// A interaction combinator net.
pub struct Net {
  pub rdex: Vec<(Ptr,Ptr)>, // redexes
  pub heap: Heap, // nodes
  pub locs: Vec<Val>,
  pub next: usize,
  pub anni: usize, // anni rewrites
  pub comm: usize, // comm rewrites
  pub eras: usize, // eras rewrites
  pub dref: usize, // dref rewrites
  pub oper: usize, // oper rewrites
}

// A compact closed net, used for dereferences.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Def {
  pub rdex: Vec<(Ptr, Ptr)>,
  pub node: Vec<(Ptr, Ptr)>,
}

pub type CallNative = Arc<dyn Fn(&Net, &Book, Ptr, Ptr) -> bool>;

// A map of id to definitions (closed nets).
pub struct Book {
  pub defs: Vec<Def>,
  pub call_native: CallNative,
}

pub fn call_native() -> CallNative {
  Arc::new(|_, _, _, _| false)
}

impl Ptr {
  #[inline(always)]
  pub const fn new(tag: Tag, val: Val) -> Self {
    Ptr((val << 4) | (tag as Val))
  }

  #[inline(always)]
  pub const fn data(&self) -> Val {
    return self.0;
  }

  #[inline(always)]
  pub const fn tag(&self) -> Tag {
    (self.data() & 0xF) as Tag
  }

  #[inline(always)]
  pub const fn val(&self) -> Val {
    (self.data() >> 4) as Val
  }

  #[inline(always)]
  pub fn is_nil(&self) -> bool {
    return self.data() == 0;
  }

  #[inline(always)]
  pub fn is_var(&self) -> bool {
    return matches!(self.tag(), VR1..=VR2);
  }

  #[inline(always)]
  pub fn is_era(&self) -> bool {
    return matches!(self.tag(), ERA);
  }

  #[inline(always)]
  pub fn is_ctr(&self) -> bool {
    return matches!(self.tag(), CT0..);
  }

  #[inline(always)]
  pub fn is_ref(&self) -> bool {
    return matches!(self.tag(), REF);
  }

  #[inline(always)]
  pub fn is_pri(&self) -> bool {
    return matches!(self.tag(), REF..);
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
    return matches!(self.tag(), OP2..);
  }

  #[inline(always)]
  pub fn has_loc(&self) -> bool {
    return matches!(self.tag(), VR1..=VR2 | OP2..);
  }

  //#[inline(always)]
  //pub fn adjust(&self, loc: Val) -> Ptr {
    //return Ptr::new(self.tag(), self.val() + if self.has_loc() { loc - 1 } else { 0 });
  //}

  // Can this redex be skipped (as an optimization)?
  #[inline(always)]
  pub fn can_skip(a: Ptr, b: Ptr) -> bool {
    return matches!(a.tag(), ERA | REF) && matches!(b.tag(), ERA | REF);
  }
}

impl APtr {
  pub fn new(ptr: Ptr) -> Self {
    APtr(AtomicU32::new(ptr.0))
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
      defs: vec![Def::new(); 1 << 24],
      call_native: call_native(),
    }
  }

  #[inline(always)]
  pub fn def(&mut self, id: Val, def: Def) {
    self.defs[id as usize] = def;
  }

  #[inline(always)]
  pub fn get(&self, id: Val) -> Option<&Def> {
    self.defs.get(id as usize)
  }
}

impl Def {
  pub fn new() -> Self {
    Def {
      rdex: vec![],
      node: vec![],
    }
  }
}

impl Heap {
  pub fn new(size: usize) -> Heap {
    let mut data = vec![];
    for _ in 0..size {
      data.push((APtr::new(NULL), APtr::new(NULL)));
    }
    return Heap { data };
  }

  #[inline(always)]
  pub fn lock(&self, index: Val) {
    return;
  }

  #[inline(always)]
  pub fn unlock(&self, index: Val) {
    return;
  }

  #[inline(always)]
  pub fn get(&self, index: Val, port: Port) -> Ptr {
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
  pub fn set(&self, index: Val, port: Port, value: Ptr) {
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
  pub fn get_root(&self) -> Ptr {
    return self.get(ROOT.val(), P2);
  }

  #[inline(always)]
  pub fn set_root(&self, value: Ptr) {
    self.set(ROOT.val(), P2, value);
  }
}

impl Net {
  // Creates an empty net with given size.
  pub fn new(size: usize) -> Self {
    Net {
      rdex: vec![],
      heap: Heap::new(size),
      locs: vec![0; 1 << 16],
      next: 1,
      anni: 0,
      comm: 0,
      eras: 0,
      dref: 0,
      oper: 0,
    }
  }

  // Creates a net and boots from a REF.
  pub fn boot(&mut self, root_id: Val) {
    self.heap.set_root(Ptr::new(REF, root_id));
  }

  // Total rewrite count.
  pub fn rewrites(&self) -> usize {
    return self.anni + self.comm + self.eras + self.dref + self.oper;
  }

  // Converts to a def.
  pub fn to_def(self) -> Def {
    let mut node = vec![];
    let mut rdex = vec![];
    for i in 0 .. self.heap.data.len() {
      let p1 = self.heap.get(node.len() as Val, P1);
      let p2 = self.heap.get(node.len() as Val, P2);
      if p1 != NULL || p2 != NULL {
        node.push((p1, p2));
      } else {
        break;
      }
    }
    for i in 0 .. self.rdex.len() {
      let p1 = self.rdex[i].0;
      let p2 = self.rdex[i].1;
      rdex.push((p1, p2));
    }
    return Def { rdex, node };
  }

  // Reads back from a def.
  pub fn from_def(def: Def) -> Self {
    let mut net = Net::new(def.node.len());
    for (i, &(p1, p2)) in def.node.iter().enumerate() {
      net.heap.set(i as Val, P1, p1);
      net.heap.set(i as Val, P2, p2);
    }
    net.rdex = def.rdex;
    net
  }

  #[inline(always)]
  pub fn alloc(&mut self, size: usize) -> Val {
    let len = self.heap.data.len();
    // On the first pass, just alloc without looking.
    if self.next < len {
      let index = self.next as Val;
      self.next += 1;
      return index;
    // On later passes, search for an available slot.
    } else {
      loop {
        self.next += 1;
        let index = (self.next % self.heap.data.len()) as Val;
        if self.heap.get(index, P2).is_nil() {
          return index;
        }
      }
    }
  }

  #[inline(always)]
  pub fn free(&self, index: Val) {
    unsafe { self.heap.data.get_unchecked(index as usize) }.0.store(NULL);
    unsafe { self.heap.data.get_unchecked(index as usize) }.1.store(NULL);
  }


  // Gets a pointer's target.
  #[inline(always)]
  pub fn get_target(&self, ptr: Ptr) -> Ptr {
    self.heap.get(ptr.val(), ptr.0 & 1)
  }

  // Sets a pointer's target.
  #[inline(always)]
  pub fn set_target(&mut self, ptr: Ptr, val: Ptr) {
    self.heap.set(ptr.val(), ptr.0 & 1, val)
  }

  // Links two pointers, forming a new wire.
  pub fn link(&mut self, a: Ptr, b: Ptr) {
    // Creates redex A-B
    if a.is_pri() && b.is_pri() {
      if Ptr::can_skip(a, b) {
        self.eras += 1;
      } else {
        self.rdex.push((a, b));
      }
      return;
    }
    // Substitutes A
    if a.is_var() {
      self.set_target(a, b);
    }
    // Substitutes B
    if b.is_var() {
      self.set_target(b, a);
    }
  }

  // Performs an interaction over a redex.
  pub fn interact(&mut self, book: &Book, a: Ptr, b: Ptr) {
    match (a.tag(), b.tag()) {
      (REF   , OP2..) => self.call(book, a, b),
      (OP2.. , REF  ) => self.call(book, b, a),
      (CT0.. , CT0..) if a.tag() == b.tag() => self.anni(a, b),
      (CT0.. , CT0..) => self.comm(a, b),
      (CT0.. , ERA  ) => self.era2(a),
      (ERA   , CT0..) => self.era2(b),
      (REF   , ERA  ) => self.eras += 1,
      (ERA   , REF  ) => self.eras += 1,
      (ERA   , ERA  ) => self.eras += 1,
      (VR1   , _    ) => self.link(a, b),
      (VR2   , _    ) => self.link(a, b),
      (_     , VR1  ) => self.link(b, a),
      (_     , VR2  ) => self.link(b, a),
      (CT0.. , NUM  ) => self.copy(a, b),
      (NUM   , CT0..) => self.copy(b, a),
      (NUM   , ERA  ) => self.eras += 1,
      (ERA   , NUM  ) => self.eras += 1,
      (NUM   , NUM  ) => self.eras += 1,
      (OP2   , NUM  ) => self.op2n(a, b),
      (NUM   , OP2  ) => self.op2n(b, a),
      (OP1   , NUM  ) => self.op1n(a, b),
      (NUM   , OP1  ) => self.op1n(b, a),
      (OP2   , CT0..) => self.comm(a, b),
      (CT0.. , OP2  ) => self.comm(b, a),
      (OP1   , CT0..) => self.pass(a, b),
      (CT0.. , OP1  ) => self.pass(b, a),
      (OP2   , ERA  ) => self.era2(a),
      (ERA   , OP2  ) => self.era2(b),
      (OP1   , ERA  ) => self.era1(a),
      (ERA   , OP1  ) => self.era1(b),
      (MAT   , NUM  ) => self.mtch(a, b),
      (NUM   , MAT  ) => self.mtch(b, a),
      (MAT   , CT0..) => self.comm(a, b),
      (CT0.. , MAT  ) => self.comm(b, a),
      (MAT   , ERA  ) => self.era2(a),
      (ERA   , MAT  ) => self.era2(b),
      _               => unreachable!(),
    };
  }

  pub fn conn(&mut self, a: Ptr, b: Ptr) {
    self.anni += 1;
    self.link(self.heap.get(a.val(), P2), self.heap.get(b.val(), P2));
    self.free(a.val());
    self.free(b.val());
  }

  pub fn anni(&mut self, a: Ptr, b: Ptr) {
    self.anni += 1;
    self.link(self.heap.get(a.val(), P1), self.heap.get(b.val(), P1));
    self.link(self.heap.get(a.val(), P2), self.heap.get(b.val(), P2));
    self.free(a.val());
    self.free(b.val());
  }

  pub fn comm(&mut self, a: Ptr, b: Ptr) {
    self.comm += 1;
    let loc0 = self.alloc(1);
    let loc1 = self.alloc(1);
    let loc2 = self.alloc(1);
    let loc3 = self.alloc(1);
    self.link(self.heap.get(a.val(), P1), Ptr::new(b.tag(), loc0));
    self.link(self.heap.get(b.val(), P1), Ptr::new(a.tag(), loc2));
    self.link(self.heap.get(a.val(), P2), Ptr::new(b.tag(), loc1));
    self.link(self.heap.get(b.val(), P2), Ptr::new(a.tag(), loc3));
    self.heap.set(loc0, P1, Ptr::new(VR1, loc2));
    self.heap.set(loc0, P2, Ptr::new(VR1, loc3));
    self.heap.set(loc1, P1, Ptr::new(VR2, loc2));
    self.heap.set(loc1, P2, Ptr::new(VR2, loc3));
    self.heap.set(loc2, P1, Ptr::new(VR1, loc0));
    self.heap.set(loc2, P2, Ptr::new(VR1, loc1));
    self.heap.set(loc3, P1, Ptr::new(VR2, loc0));
    self.heap.set(loc3, P2, Ptr::new(VR2, loc1));
    self.free(a.val());
    self.free(b.val());
  }

  pub fn pass(&mut self, a: Ptr, b: Ptr) {
    self.comm += 1;
    let loc0 = self.alloc(1);
    let loc1 = self.alloc(1);
    let loc2 = self.alloc(1);
    self.link(self.heap.get(a.val(), P2), Ptr::new(b.tag(), loc0));
    self.link(self.heap.get(b.val(), P1), Ptr::new(a.tag(), loc1));
    self.link(self.heap.get(b.val(), P2), Ptr::new(a.tag(), loc2));
    self.heap.set(loc0, P1, Ptr::new(VR2, loc1));
    self.heap.set(loc0, P2, Ptr::new(VR2, loc2));
    self.heap.set(loc1, P1, self.heap.get(a.val(), P1));
    self.heap.set(loc1, P2, Ptr::new(VR1, loc0));
    self.heap.set(loc2, P1, self.heap.get(a.val(), P1));
    self.heap.set(loc2, P2, Ptr::new(VR2, loc0));
    self.free(a.val());
    self.free(b.val());
  }

  pub fn copy(&mut self, a: Ptr, b: Ptr) {
    self.comm += 1;
    self.link(self.heap.get(a.val(), P1), b);
    self.link(self.heap.get(a.val(), P2), b);
    self.free(a.val());
  }

  pub fn era2(&mut self, a: Ptr) {
    self.eras += 1;
    self.link(self.heap.get(a.val(), P1), ERAS);
    self.link(self.heap.get(a.val(), P2), ERAS);
    self.free(a.val());
  }

  pub fn era1(&mut self, a: Ptr) {
    self.eras += 1;
    self.link(self.heap.get(a.val(), P2), ERAS);
    self.free(a.val());
  }

  pub fn op2n(&mut self, a: Ptr, b: Ptr) {
    self.oper += 1;
    let mut p1 = self.heap.get(a.val(), P1);
    // Optimization: perform chained ops at once
    if p1.is_num() {
      let mut rt = b.val();
      let mut p2 = self.heap.get(a.val(), P2);
      loop {
        self.oper += 1;
        rt = self.op(rt, p1.val());
        // If P2 is OP2, keep looping
        if p2.is_op2() {
          p1 = self.heap.get(p2.val(), P1);
          if p1.is_num() {
            p2 = self.heap.get(p2.val(), P2);
            self.oper += 1; // since OP1 is skipped
            continue;
          }
        }
        // If P2 is OP1, flip args and keep looping
        if p2.is_op1() {
          let tmp = rt;
          rt = self.heap.get(p2.val(), P1).val();
          p1 = Ptr::new(NUM, tmp);
          p2 = self.heap.get(p2.val(), P2);
          continue;
        }
        break;
      }
      self.link(Ptr::new(NUM, rt), p2);
      return;
    }
    self.heap.set(a.val(), P1, b);
    self.link(Ptr::new(OP1, a.val()), p1);
  }

  pub fn op1n(&mut self, a: Ptr, b: Ptr) {
    self.oper += 1;
    let p1 = self.heap.get(a.val(), P1);
    let p2 = self.heap.get(a.val(), P2);
    let v0 = p1.val() as Val;
    let v1 = b.val() as Val;
    let v2 = self.op(v0, v1);
    self.link(Ptr::new(NUM, v2), p2);
    self.free(a.val());
  }

  #[inline(always)]
  pub fn op(&self, a: Val, b: Val) -> Val {
    let a_opr = (a >> 24) & 0xF;
    let b_opr = (b >> 24) & 0xF; // not used yet
    let a_val = a & 0xFFFFFF;
    let b_val = b & 0xFFFFFF;
    match a_opr as Tag {
      USE => { ((a_val & 0xF) << 24) | b_val }
      ADD => { (a_val.wrapping_add(b_val)) & 0xFFFFFF }
      SUB => { (a_val.wrapping_sub(b_val)) & 0xFFFFFF }
      MUL => { (a_val.wrapping_mul(b_val)) & 0xFFFFFF }
      DIV => { if b_val == 0 { 0xFFFFFF } else { (a_val.wrapping_div(b_val)) & 0xFFFFFF } }
      MOD => { (a_val.wrapping_rem(b_val)) & 0xFFFFFF }
      EQ  => { ((a_val == b_val) as Val) & 0xFFFFFF }
      NE  => { ((a_val != b_val) as Val) & 0xFFFFFF }
      LT  => { ((a_val < b_val) as Val) & 0xFFFFFF }
      GT  => { ((a_val > b_val) as Val) & 0xFFFFFF }
      AND => { (a_val & b_val) & 0xFFFFFF }
      OR  => { (a_val | b_val) & 0xFFFFFF }
      XOR => { (a_val ^ b_val) & 0xFFFFFF }
      NOT => { (!b_val) & 0xFFFFFF }
      LSH => { (a_val << b_val) & 0xFFFFFF }
      RSH => { (a_val >> b_val) & 0xFFFFFF }
      _   => { unreachable!() }
    }
  }

  pub fn mtch(&mut self, a: Ptr, b: Ptr) {
    self.oper += 1;
    let p1 = self.heap.get(a.val(), P1); // branch
    let p2 = self.heap.get(a.val(), P2); // return
    if b.val() == 0 {
      let loc0 = self.alloc(1);
      self.heap.set(loc0, P2, ERAS);
      self.link(p1, Ptr::new(CT0, loc0));
      self.link(p2, Ptr::new(VR1, loc0));
      self.free(a.val());
    } else {
      let loc0 = self.alloc(1);
      let loc1 = self.alloc(1);
      self.heap.set(loc0, P1, ERAS);
      self.heap.set(loc0, P2, Ptr::new(CT0, loc1));
      self.heap.set(loc1, P1, Ptr::new(NUM, b.val() - 1));
      self.link(p1, Ptr::new(CT0, loc0));
      self.link(p2, Ptr::new(VR2, loc1));
      self.free(a.val());
    }
  }

  // Expands a closed net.
  #[inline(always)]
  pub fn call(&mut self, book: &Book, ptr: Ptr, par: Ptr) {
    self.dref += 1;
    let mut ptr = ptr;
    // FIXME: change "while" to "if" once lang prevents refs from returning refs
    if ptr.is_ref() {
      // Intercepts with a native function, if available.
      if self.call_native(book, ptr, par) {
        return;
      }
      // Load the closed net.
      let got = unsafe { book.defs.get_unchecked((ptr.val() as usize) & 0xFFFFFF) };
      if got.node.len() > 0 {
        let len = got.node.len() - 1;
        // Allocates space.
        for i in 0 .. len {
          *unsafe { self.locs.get_unchecked_mut(1 + i) } = self.alloc(1);
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
          self.rdex.push((p1, p2));
        }
        // Load root, adjusted.
        ptr = self.adjust(got.node[0].1);
      }
    }
    self.link(ptr, par);
  }

  fn adjust(&self, ptr: Ptr) -> Ptr {
    if ptr.has_loc() {
      return Ptr::new(ptr.tag(), *unsafe { self.locs.get_unchecked(ptr.val() as usize) });
    } else {
      return ptr;
    }
  }

  // Reduces all redexes.
  pub fn reduce(&mut self, book: &Book) {
    let mut rdex: Vec<(Ptr, Ptr)> = vec![];
    std::mem::swap(&mut self.rdex, &mut rdex);
    while rdex.len() > 0 {
      for (a, b) in &rdex {
        self.interact(book, *a, *b);
      }
      rdex.clear();
      std::mem::swap(&mut self.rdex, &mut rdex);
    }
  }

  // Expands heads.
  pub fn expand(&mut self, book: &Book, dir: Ptr) {
    let ptr = self.get_target(dir);
    if ptr.is_ctr() {
      self.expand(book, Ptr::new(VR1, ptr.val()));
      self.expand(book, Ptr::new(VR2, ptr.val()));
    } else if ptr.is_ref() {
      self.call(book, ptr, dir);
      //self.set_target(dir, exp);
    }
  }

  // Reduce a net to normal form.
  pub fn normal(&mut self, book: &Book) {
    self.expand(book, ROOT);
    while self.rdex.len() > 0 {
      self.reduce(book);
      self.expand(book, ROOT);
    }
  }

}
