// An efficient Interaction Combinator runtime
// ===========================================
// This file implements an efficient interaction combinator runtime. Nodes are represented by 2 aux
// ports (P1, P2), with the main port (P1) omitted. A separate vector, 'rdex', holds main ports,
// and, thus, tracks active pairs that can be reduced in parallel. Pointers are unboxed, meaning
// that ERAs, NUMs and REFs don't use any additional space. REFs lazily expand to closed nets when
// they interact with nodes, and are cleared when they interact with ERAs, allowing for constant
// space evaluation of recursive functions on Scott encoded datatypes.

pub type Tag = u8;
pub type Ari = u8;
pub type Lab = u32;
pub type Val = u32;
pub type Data = u64;

// Core terms.

/// Variable to aux port
pub const VAR: Tag = 0x0;
/// Redirect to aux port
pub const RDR: Tag = 0x1;
/// Lazy closed net
pub const REF: Tag = 0x2;
/// Unboxed eraser
pub const ERA: Tag = 0x3;
/// Unboxed number
pub const NUM: Tag = 0x4;
/// Binary numeric operation
pub const OP2: Tag = 0x5;
/// Unary numeric operation
pub const OP1: Tag = 0x6;
/// Numeric if-then-else(MATCH)
pub const MAT: Tag = 0x7;
/// Main port of con node
pub const CTR: Tag = 0x8;


// Numeric operations.
pub type NumericOp = u8;
pub const USE: NumericOp = 0x0; // set-next-op
pub const ADD: NumericOp = 0x1; // addition
pub const SUB: NumericOp = 0x2; // subtraction
pub const MUL: NumericOp = 0x3; // multiplication
pub const DIV: NumericOp = 0x4; // division
pub const MOD: NumericOp = 0x5; // modulus
pub const EQ : NumericOp = 0x6; // equal-to
pub const NE : NumericOp = 0x7; // not-equal-to
pub const LT : NumericOp = 0x8; // less-than
pub const GT : NumericOp = 0x9; // greater-than
pub const AND: NumericOp = 0xA; // logical-and
pub const OR : NumericOp = 0xB; // logical-or
pub const XOR: NumericOp = 0xC; // logical-xor
pub const NOT: NumericOp = 0xD; // logical-not
pub const LSH: NumericOp = 0xE; // left-shift
pub const RSH: NumericOp = 0xF; // right-shift

// Root address
pub const ROOT_PORT: Val = 0 + P2;
pub const FIRST_PORT: Val = 2 + P1;

// Root pointer.
pub const ERAS: Ptr = Ptr::new_val(ERA, 0x0000_0000);
pub const ROOT: Ptr = Ptr::new_val(VAR, ROOT_PORT);
pub const NULL: Ptr = Ptr(0x0000_0000_0000_0000);

// An auxiliary port.
pub type Port = Val;
pub const P1: Port = 0;
pub const P2: Port = 1;


// A tagged pointer.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct Ptr(pub Data);

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Heap {
  data: Vec<Ptr>,
  next: usize,
  used: usize,
  full: bool,
}

// A interaction combinator net.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Net {
  pub rdex: Vec<(Ptr,Ptr)>, // redexes
  pub heap: Heap, // nodes
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
  pub port: Vec<Ptr>,
}

// A map of id to definitions (closed nets).
pub struct Book {
  pub defs: Vec<Def>,
}

impl Ptr {
  #[inline(always)]
  pub const fn new(tag: Tag, ari: Ari, lab: Lab, val: Val) -> Self {
    Ptr((val as Data) << 32 | (lab as Data) << 8 | (ari as Data) << 4 | (tag as Data))
  }

  #[inline(always)]
  // Creates a Ptr containing an unboxed value (VAR, REF, NUM, etc)
  pub const fn new_val(tag: Tag, val: Val) -> Self {
    Ptr((val as Data) << 32 | (tag as Data))
  }

  #[inline(always)]
  pub fn copy(&self, val: Val) -> Self {
    Ptr(self.data() & 0xFFFF_FFFF | (val as Data) << 32)
  }

  #[inline(always)]
  pub fn data(&self) -> Data {
    return self.0;
  }

  #[inline(always)]
  pub fn tag(&self) -> Tag {
    (self.data() & 0xF) as Tag
  }

  #[inline(always)]
  pub fn ari(&self) -> Ari {
    (self.data() >> 4 & 0xF) as Ari
  }

  #[inline(always)]
  pub fn lab(&self) -> Lab {
    (self.data() >> 8 & 0xFF_FFFF) as Lab
  }

  #[inline(always)]
  pub fn val(&self) -> Val {
    (self.data() >> 32) as Val
  }

  #[inline(always)]
  pub fn is_nil(&self) -> bool {
    return self.data() == 0;
  }

  #[inline(always)]
  pub fn is_var(&self) -> bool {
    return matches!(self.tag(), VAR);
  }

  #[inline(always)]
  pub fn is_era(&self) -> bool {
    return matches!(self.tag(), ERA);
  }

  #[inline(always)]
  pub fn is_ctr(&self) -> bool {
    return matches!(self.tag(), CTR);
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
  pub fn has_loc(&self) -> bool {
    self.is_var() || (matches!(self.tag(), OP2 | OP1 | MAT | CTR) && self.ari() > 0)
  }

  #[inline(always)]
  // Move pointers from a Def to pointer to the main net.
  pub fn adjust(&self, loc: Val) -> Ptr {
    let offset = if self.has_loc() { loc - FIRST_PORT } else { 0 };
    return self.copy(self.val() + offset);
  }

  // Can this redex be skipped (as an optimization)?
  #[inline(always)]
  pub fn can_skip(a: Ptr, b: Ptr) -> bool {
    return matches!(a.tag(), ERA | REF) && matches!(b.tag(), ERA | REF);
  }
}

impl Book {
  #[inline(always)]
  pub fn new() -> Self {
    Book {
      defs: vec![Def::new(); 1 << 24],
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
      port: vec![],
    }
  }
}

impl Heap {
  pub fn new(size: usize) -> Heap {
    return Heap {
      data: vec![NULL; size],
      next: FIRST_PORT as usize,
      used: 0,
      full: false,
    };
  }

  #[inline(always)]
  // Allocs `size` ports in the heap
  pub fn alloc(&mut self, size: usize) -> Val {
    if size == 0 {
      return 0;
    } else if !self.full && self.next + size <= self.data.len() {
      self.used += size;
      self.next += size;
      return (self.next - size) as Val;
    } else {
      self.full = true;
      let mut space = 0;
      loop {
        if self.next >= self.data.len() {
          space = 0;
          self.next = FIRST_PORT as usize;
        }
        if self.get(self.next as Val).is_nil() {
          space += 1;
        } else {
          space = 0;
        }
        self.next += 1;
        if space == size {
          self.used += size;
          return (self.next - space) as Val;
        }
      }
    }
  }

  #[inline(always)]
  pub fn free(&mut self, index: Val, size: usize) {
    self.used -= size as usize;
    for i in 0..size {
      self.set(index + i as Val, NULL);
    }
  }

  #[inline(always)]
  pub fn lock(&self, index: Data) {
    return;
  }

  #[inline(always)]
  pub fn unlock(&self, index: Data) {
    return;
  }

  #[inline(always)]
  pub fn get(&self, index: Val) -> Ptr {
    unsafe {
      *self.data.get_unchecked(index as usize)
    }
  }

  #[inline(always)]
  pub fn set(&mut self, index: Val, value: Ptr) {
    unsafe {
      *self.data.get_unchecked_mut(index as usize) = value;
    }
  }

  #[inline(always)]
  pub fn get_root(&self) -> Ptr {
    return self.get(ROOT_PORT);
  }

  #[inline(always)]
  pub fn set_root(&mut self, value: Ptr) {
    self.set(ROOT_PORT, value);
  }

  #[inline(always)]
  pub fn compact(&self) -> Vec<Ptr> {
    let mut node = vec![self.data[0], self.data[1]];
    let mut i = 2;
    loop {
      let p = self.data[i];
      if p != NULL {
        node.push(p);
        i += 1;
      } else {
        break;
      }
    }
    return node;
  }
}

impl Net {
  // Creates an empty net with given size.
  pub fn new(size: usize) -> Self {
    Net {
      rdex: vec![],
      heap: Heap::new(size),
      anni: 0,
      comm: 0,
      eras: 0,
      dref: 0,
      oper: 0,
    }
  }

  // Creates a net and boots from a REF.
  pub fn boot(&mut self, root_id: Val) {
    self.heap.set_root(Ptr::new(REF, 0, 0, root_id));
  }

  // Converts to a def.
  pub fn to_def(self) -> Def {
    Def { rdex: self.rdex, port: self.heap.compact() }
  }

  // Reads back from a def.
  pub fn from_def(def: Def) -> Self {
    let mut net = Net::new(def.port.len());
    for (i, &p) in def.port.iter().enumerate() {
      net.heap.set(i as Val, p);
    }
    net.rdex = def.rdex;
    net
  }

  // Gets a pointer's target.
  #[inline(always)]
  pub fn get_target(&self, ptr: Ptr) -> Ptr {
    self.heap.get(ptr.val())
  }

  // Sets a pointer's target.
  #[inline(always)]
  pub fn set_target(&mut self, ptr: Ptr, val: Ptr) {
    self.heap.set(ptr.val(), val)
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
    let mut a = a;
    let mut b = b;
    // Dereference A
    if a.is_ref() && b.is_pri() && !b.is_skp() {
      a = self.deref(book, a, b);
    } else if b.is_ref() && a.is_pri() && !a.is_skp() {
      b = self.deref(book, b, a);
    }
    match (a.tag(), b.tag()) {
      (CTR, CTR) if a.tag() == b.tag() => self.anni(a, b),
      (CTR, CTR) => self.comm(a, b),
      (CTR, ERA) => self.era2(a),
      (ERA, CTR) => self.era2(b),
      (REF, ERA) => self.eras += 1,
      (ERA, REF) => self.eras += 1,
      (ERA, ERA) => self.eras += 1,
      (VAR,   _) => self.link(a, b),
      (_  , VAR) => self.link(b, a),
      (CTR, NUM) => self.copy(a, b),
      (NUM, CTR) => self.copy(b, a),
      (NUM, ERA) => self.eras += 1,
      (ERA, NUM) => self.eras += 1,
      (NUM, NUM) => self.eras += 1,
      (OP2, NUM) => self.op2n(a, b),
      (NUM, OP2) => self.op2n(b, a),
      (OP1, NUM) => self.op1n(a, b),
      (NUM, OP1) => self.op1n(b, a),
      (OP2, CTR) => self.comm(a, b),
      (CTR, OP2) => self.comm(b, a),
      (OP1, CTR) => self.pass(a, b),
      (CTR, OP1) => self.pass(b, a),
      (OP2, ERA) => self.era2(a),
      (ERA, OP2) => self.era2(b),
      (OP1, ERA) => self.era1(a),
      (ERA, OP1) => self.era1(b),
      (MAT, NUM) => self.mtch(a, b),
      (NUM, MAT) => self.mtch(b, a),
      (MAT, CTR) => self.comm(a, b),
      (CTR, MAT) => self.comm(b, a),
      (MAT, ERA) => self.era2(a),
      (ERA, MAT) => self.era2(b),

      // because of the deref above this match
      // we know that A and B are not REFs
      (REF, _) => unreachable!(),
      (_, REF) => unreachable!(),

      // undefined numerical interactions resulting from a sort of "type error"
      (OP2..=MAT, OP2..=MAT) => unreachable!(),

      // TODO: this will change when we implement the multi-threaded version
      (RDR, _) => unreachable!(),
      (_, RDR) => unreachable!(),
      
      // All the other unused numbers
      (_, _) => unreachable!(),
    };
  }

  pub fn anni(&mut self, a: Ptr, b: Ptr) {
    self.anni += 1;
    let (a, b) = if a.ari() >= b.ari() { (a, b) } else { (b, a) };
    let max = a.ari() as Val;
    let min = b.ari() as Val;

    for port in 0..min {
      self.link(self.heap.get(a.val() + port), self.heap.get(b.val() + port));
    }
    // If a is larger than b, insert an Era to the remaining ports
    let loc = self.heap.alloc(max as usize - min as usize);
    for port in min..max {
      self.link(self.heap.get(a.val() + port), ERAS);
    }
    self.heap.free(a.val(), max as usize);
    self.heap.free(b.val(), min as usize);
  }

  pub fn comm(&mut self, a: Ptr, b: Ptr) {
    self.comm += 1;
    let a_ari = a.ari() as Val;
    let b_ari = b.ari() as Val;
    let half = a_ari * b_ari;
    let loc = self.heap.alloc(2*half as usize);
    // Link main ports
    for i in 0..a_ari {
      self.link(self.heap.get(a.val() + i), b.copy(loc + i * b_ari));
    }
    for i in 0..b_ari {
      self.link(self.heap.get(b.val() + i), a.copy(loc + half + i * a_ari));
    }
    // Link aux ports
    for i in 0..a_ari {
      for j in 0..b_ari {
        let a = loc + i * b_ari + j;
        let b = loc + half + j * a_ari + i;
        self.heap.set(a, Ptr::new_val(VAR, b));
        self.heap.set(b, Ptr::new_val(VAR, a));
      }
    }
    self.heap.free(a.val(), a.ari() as usize);
    self.heap.free(b.val(), b.ari() as usize);
  }

  pub fn pass(&mut self, a: Ptr, b: Ptr) {
    // `a` goes through Ctr `b` and is copied once at each port.
    // `a` owns a value at port 1 and returns at port 2.
    self.comm += 1;
    let loc = self.heap.alloc(b.ari() as usize + 2*b.ari() as usize);

    // Link main port of b
    self.link(self.heap.get(a.val() + P2), b.copy(loc));

    let owned = self.heap.get(a.val() + P1);
    for i in 0..b.ari() as Val {
      // Link main port of copy of a
      self.link(self.heap.get(b.val() + i), a.copy(loc + b.ari() as Val + 2*i));
      // Link aux ports
      let a_p1 = loc + b.ari() as Val + 2*i + P1;
      let a_p2 = loc + b.ari() as Val + 2*i + P2;
      let b_p = loc + i;
      self.heap.set(a_p1, owned);
      self.heap.set(a_p2, Ptr::new_val(VAR, b_p));
      self.heap.set(b_p, Ptr::new_val(VAR, a_p2));
    }
    self.heap.free(a.val(), 2);
    self.heap.free(b.val(), b.ari() as usize);
  }

  pub fn copy(&mut self, a: Ptr, b: Ptr) {
    // `b` goes through Ctr `a` and is copied once at each port.
    // `b` has arity 0.
    self.comm += 1;
    for i in 0..a.ari() as Val {
      self.link(self.heap.get(a.val() + i), b);
    }
    self.heap.free(a.val(), a.ari() as usize);
  }

  pub fn era2(&mut self, a: Ptr) {
    // Erases a normal node with a.ari() aux ports.
    self.eras += 1;
    for i in 0..a.ari() as Val {
      self.link(self.heap.get(a.val() + i), ERAS);
    }
    self.heap.free(a.val(), a.ari() as usize);
  }

  pub fn era1(&mut self, a: Ptr) {
    // Erases a node with 2 aux ports where P1 holds an owned value.
    self.eras += 1;
    self.link(self.heap.get(a.val() + P2), ERAS);
    self.heap.free(a.val(), 2);
  }

  pub fn op2n(&mut self, a: Ptr, b: Ptr) {
    self.oper += 1;
    let mut p1 = self.heap.get(a.val() + P1);
    // Optimization: perform chained ops at once
    if p1.is_num() {
      let mut rt = b.val();
      let mut p2 = self.heap.get(a.val() + P2);
      loop {
        self.oper += 1;
        rt = self.prim(rt, p1.val());
        // If P2 is OP2, keep looping
        if p2.is_op2() {
          p1 = self.heap.get(p2.val() + P1);
          if p1.is_num() {
            p2 = self.heap.get(p2.val() + P2);
            self.oper += 1; // since OP1 is skipped
            continue;
          }
        }
        // If P2 is OP1, flip args and keep looping
        if p2.is_op1() {
          let tmp = rt;
          rt = self.heap.get(p2.val() + P1).val();
          p1 = Ptr::new_val(NUM, tmp);
          p2 = self.heap.get(p2.val() + P2);
          continue;
        }
        break;
      }
      self.link(Ptr::new_val(NUM, rt), p2);
      return;
    }
    self.heap.set(a.val() + P1, b);
    self.link(Ptr::new(OP1, 2, 0, a.val()), p1);
  }

  pub fn op1n(&mut self, a: Ptr, b: Ptr) {
    self.oper += 1;
    let p1 = self.heap.get(a.val() + P1);
    let p2 = self.heap.get(a.val() + P2);
    let v0 = p1.val() as Val;
    let v1 = b.val() as Val;
    let v2 = self.prim(v0, v1);
    self.link(Ptr::new_val(NUM, v2), p2);
    self.heap.free(a.val(), 2);
  }

  pub fn prim(&mut self, a: Val, b: Val) -> Val {
    let a_opr = (a >> 24) & 0xF;
    let b_opr = (b >> 24) & 0xF; // not used yet
    let a_val = a & 0xFFFFFF;
    let b_val = b & 0xFFFFFF;
    match a_opr as NumericOp {
      USE => { ((a_val & 0xF) << 24) | b_val }
      ADD => { (a_val.wrapping_add(b_val)) & 0xFFFFFF }
      SUB => { (a_val.wrapping_sub(b_val)) & 0xFFFFFF }
      MUL => { (a_val.wrapping_mul(b_val)) & 0xFFFFFF }
      DIV if b_val == 0 => { 0xFFFFFF }
      DIV => { (a_val.wrapping_div(b_val)) & 0xFFFFFF }
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
    let p1 = self.heap.get(a.val() + P1); // branch
    let p2 = self.heap.get(a.val() + P2); // return
    if b.val() == 0 {
      let loc = self.heap.alloc(2);
      self.heap.set(loc+0+P2, ERAS);
      self.link(p1, Ptr::new(CTR, 2, 0, loc+0));
      self.link(p2, Ptr::new_val(VAR, loc+0));
      self.heap.free(a.val(), 2);
    } else {
      let loc = self.heap.alloc(4);
      self.heap.set(loc+0+P1, ERAS);
      self.heap.set(loc+0+P2, Ptr::new(CTR, 2, 0, loc + 2));
      self.heap.set(loc+2+P1, Ptr::new_val(NUM, b.val() - 1));
      self.link(p1, Ptr::new(CTR, 2, 0, loc+0));
      self.link(p2, Ptr::new_val(VAR, loc+3));
      self.heap.free(a.val(), 2);
    }
  }

  // Expands a closed net.
  #[inline(always)]
  pub fn deref(&mut self, book: &Book, ptr: Ptr, parent: Ptr) -> Ptr {
    self.dref += 1;
    let mut ptr = ptr;
    // FIXME: change "while" to "if" once lang prevents refs from returning refs
    while ptr.is_ref() {
      // Load the closed net.
      let got = unsafe { book.defs.get_unchecked((ptr.val() as usize) & 0xFFFFFF) };
      if got.port.len() > 0 {
        let len = got.port.len() - FIRST_PORT as usize;
        let loc = self.heap.alloc(len);
        // Load nodes, adjusted.
        for i in 0..len as Val {
          unsafe {
            let p = got.port.get_unchecked((FIRST_PORT + i) as usize).adjust(loc);
            self.heap.set(loc + i, p);
          }
        }
        for r in &got.rdex {
          let p1 = r.0.adjust(loc);
          let p2 = r.1.adjust(loc);
          self.rdex.push((p1, p2));
        }
        // Load root, adjusted.
        ptr = got.port[ROOT_PORT as usize].adjust(loc);
        // Link root.
        if ptr.is_var() {
          self.set_target(ptr, parent);
        }
      }
    }
    return ptr;
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

  pub fn reduce_debug(&mut self, book: &Book) {
    while !self.rdex.is_empty() {
      let (a, b) = self.rdex.remove(0);
      self.interact(book, a, b);
      eprintln!("{}\n", crate::ast::show_runtime_net(&self));
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

  // Expands heads.
  pub fn expand(&mut self, book: &Book, dir: Ptr) {
    let ptr = self.get_target(dir);
    if ptr.is_ctr() {
      for i in 0..ptr.ari() as Val {
        self.expand(book, Ptr::new_val(VAR, ptr.val() + i));
      }
    } else if ptr.is_ref() {
      let exp = self.deref(book, ptr, dir);
      self.set_target(dir, exp);
    }
  }

  // Total rewrite count.
  pub fn rewrites(&self) -> usize {
    return self.anni + self.comm + self.eras + self.dref + self.oper;
  }
}
