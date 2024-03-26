use std::mem::MaybeUninit;

use super::*;

impl<'a, M: Mode> Net<'a, M> {
  /// Performs an interaction between two connected principal ports.
  #[inline(always)]
  pub fn interact(&mut self, a: Port, b: Port) {
    self.sync();
    trace!(self, a, b);
    use Tag::*;
    match (a.tag(), b.tag()) {
      // not actually an active pair
      (Var | Red, _) | (_, Var | Red) => unreachable!(),

      (Ref, _) if a != Port::ERA => self.call(a, b),
      (_, Ref) if b != Port::ERA => self.call(b, a),

      (Num | Ref | AdtZ, Num | Ref | AdtZ) => self.rwts.eras += 1,

      (CtrN!(), CtrN!()) if a.lab() == b.lab() => self.anni(a, b),

      (AdtN!() | AdtZ, CtrN!()) if a.lab() == b.lab() => self.adt_ctr(a, b),
      (CtrN!(), AdtN!() | AdtZ) if a.lab() == b.lab() => self.adt_ctr(b, a),
      (AdtN!() | AdtZ, AdtN!() | AdtZ) if a.lab() == b.lab() => todo!(),

      (CtrN!(), Mat) if a.lab() == 0 => todo!(),
      (Mat, CtrN!()) if b.lab() == 0 => todo!(),
      (Op, Op) if a.op() != b.op() => todo!(),

      (Mat, Mat) | (Op, Op) => self.anni(a, b),

      (Op, Num) => self.op_num(a, b),
      (Num, Op) => self.op_num(b, a),
      (Mat, Num) => self.mat_num(a, b),
      (Num, Mat) => self.mat_num(b, a),

      (_, _) => self.comm(a, b),
    }
  }

  /// Annihilates two binary agents.
  ///
  /// ```text
  ///  
  ///         a2 |   | a1
  ///           _|___|_
  ///           \     /
  ///         a  \   /
  ///             \ /
  ///              |
  ///             / \
  ///         b  /   \
  ///           /_____\
  ///            |   |
  ///         b1 |   | b2
  ///
  /// --------------------------- anni2
  ///
  ///         a2 |   | a1
  ///            |   |
  ///             \ /
  ///              X
  ///             / \
  ///            |   |
  ///         b1 |   | b2
  ///  
  /// ```
  #[inline(never)]
  pub fn anni2(&mut self, a: Port, b: Port) {
    trace!(self, a, b);
    self.rwts.anni += 1;
    let a = a.consume_node();
    let b = b.consume_node();
    self.link_wire_wire(a.p1, b.p1);
    self.link_wire_wire(a.p2, b.p2);
  }

  /// Commutes two binary agents.
  ///
  /// ```text
  ///  
  ///         a2 |   | a1
  ///           _|___|_
  ///           \     /
  ///         a  \   /
  ///             \ /
  ///              |
  ///             /#\
  ///         b  /###\
  ///           /#####\
  ///            |   |
  ///         b1 |   | b2
  ///
  /// --------------------------- comm22
  ///
  ///     a2 |         | a1
  ///        |         |
  ///       /#\       /#\
  ///  B2  /###\     /###\  B1
  ///     /#####\   /#####\
  ///      |   \     /   |
  ///   p1 | p2 \   / p1 | p2
  ///      |     \ /     |
  ///      |      X      |
  ///      |     / \     |
  ///   p2 | p1 /   \ p2 | p1
  ///     _|___/_   _\___|_
  ///     \     /   \     /
  ///  A1  \   /     \   /  A2
  ///       \ /       \ /
  ///        |         |
  ///     b1 |         | b2
  ///  
  /// ```
  #[inline(never)]
  pub fn comm22(&mut self, a: Port, b: Port) {
    trace!(self, a, b);
    self.rwts.comm += 1;

    let a = a.consume_node();
    let b = b.consume_node();

    let A1 = self.create_node(a.tag, a.lab);
    let A2 = self.create_node(a.tag, a.lab);
    let B1 = self.create_node(b.tag, b.lab);
    let B2 = self.create_node(b.tag, b.lab);

    trace!(self, A1.p0, A2.p0, B1.p0, B2.p0);
    self.link_port_port(A1.p1, B1.p1);
    self.link_port_port(A1.p2, B2.p1);
    self.link_port_port(A2.p1, B1.p2);
    self.link_port_port(A2.p2, B2.p2);

    trace!(self);
    self.link_wire_port(a.p1, B1.p0);
    self.link_wire_port(a.p2, B2.p0);
    self.link_wire_port(b.p1, A1.p0);
    self.link_wire_port(b.p2, A2.p0);
  }

  /// Commutes a nilary agent and a binary agent.
  ///
  /// ```text
  ///  
  ///         a  (---)
  ///              |
  ///              |
  ///             /#\
  ///         b  /###\
  ///           /#####\
  ///            |   |
  ///         b1 |   | b2
  ///
  /// --------------------------- comm02
  ///
  ///     a (---)   (---) a
  ///         |       |
  ///      b1 |       | b2
  ///  
  /// ```
  #[inline(never)]
  pub fn comm02(&mut self, a: Port, b: Port) {
    trace!(self, a, b);
    self.rwts.comm += 1;
    let b = b.consume_node();
    self.link_wire_port(b.p1, a.clone());
    self.link_wire_port(b.p2, a);
  }

  /// Interacts a number and a numeric match node.
  ///
  /// ```text
  ///                               |
  ///          b   (0)              |          b  (n+1)
  ///               |               |               |
  ///               |               |               |
  ///              / \              |              / \
  ///             /   \             |             /   \
  ///         a  / mat \            |         a  / mat \
  ///           /_______\           |           /_______\
  ///           /   |   \           |           /   |   \
  ///       a1 /    | a2 \ a3       |       a1 /    | a2 \ a3
  ///                               |
  /// ----------------------------- | ----------------------------- mat_num
  ///                               |                _ _ _        
  ///                               |              /       \
  ///                               |          x2 |  (n) x1 |
  ///                               |            _|___|_    |
  ///                               |            \     /    |
  ///             _ _ _             |          x  \   /     |
  ///           /       \           |              \ /     /
  ///          |   (*)   |          |         (*)   |     /
  ///       a1 |    | a2 | a3       |       a1 |    | a2 | a3       
  ///                               |
  /// ```
  #[inline(never)]
  pub fn mat_num(&mut self, a: Port, b: Port) {
    trace!(self, a, b);
    self.rwts.oper += 1;
    let a1 = a.aux_port(0).wire();
    let a2 = a.aux_port(1).wire();
    let a3 = a.aux_port(2).wire();
    let b = b.num();
    if b == 0 {
      self.link_wire_wire(a1, a3);
      self.link_wire_port(a2, Port::ERA);
    } else {
      let x = self.create_node(Tag::Ctr2, 0);
      trace!(self, x.p0);
      self.link_port_port(x.p1, Port::new_num(b - 1));
      self.link_wire_port(a1, Port::ERA);
      self.link_wire_port(a2, x.p0);
      self.link_wire_port(a3, x.p2);
    }
  }

  /// Interacts a number and a binary numeric operation node.
  ///
  /// ```text
  ///                             |  
  ///         b   (n)             |         b   (n)    
  ///              |              |              |      
  ///              |              |              |       
  ///             / \             |             / \       
  ///         a  /op \            |         a  /op \       
  ///           /_____\           |           /_____\       
  ///            |   |            |            |   |         
  ///           (m)  | a2         |         a1 |   | a2       
  ///                             |                            
  /// --------------------------- | --------------------------- op_num
  ///                             |           _ _ _
  ///                             |         /       \
  ///                             |        |  (n)    |   
  ///                             |       _|___|_    |   
  ///                             |       \     /    |   
  ///                             |     x  \op$/     |   
  ///            (n op m)         |         \ /      |   
  ///                |            |          |       |   
  ///                | a2         |       a1 |       | a2  
  ///                             |  
  /// ```
  #[inline(never)]
  pub fn op_num(&mut self, a: Port, b: Port) {
    trace!(self, a, b);
    let a = a.consume_node();
    let op = unsafe { Op::from_unchecked(a.lab) };
    let a1 = a.p1.load_target();
    if a1.is(Tag::Num) {
      self.rwts.oper += 1;
      self.free_wire(a.p1);
      let out = op.op(b.num(), a1.num());
      self.link_wire_port(a.p2, Port::new_num(out));
    } else {
      let op = op.swap();
      let x = self.create_node(Tag::Op, op as u16);
      trace!(self, x.p0);
      self.link_port_port(x.p1, b);
      self.link_wire_port(a.p2, x.p2);
      self.link_wire_port(a.p1, x.p0);
    }
  }

  fn adt_ctr(&mut self, adt: Port, ctr: Port) {
    self.rwts.anni += 1;
    let ctr_arity = ctr.tag().arity();
    let adtz = if adt.is(AdtZ) { adt.clone() } else { adt.aux_port(adt.tag().arity()).wire().swap_target(Port::LOCK) };
    if ctr_arity < adtz.variant_count() + 1 {
      if ctr_arity <= adtz.variant_index() + 1 {
        for i in 0 .. (ctr_arity - 1) {
          self.link_wire_port(ctr.aux_port(i).wire(), Port::ERA);
        }
        let new_adtz = Port::new_adtz(adtz.variant_index() - (ctr_arity - 1), adtz.variant_count() - (ctr_arity - 1));
        self.link_wire_port(
          ctr.aux_port(ctr_arity - 1).wire(),
          if adt.is(AdtZ) {
            new_adtz
          } else {
            adt.aux_port(adt.tag().arity()).wire().set_target(new_adtz);
            adt
          },
        );
        return;
      }
      dbg!(ctr_arity, adtz.variant_count(), adtz.variant_index());
      todo!();
    }
    let out = if ctr_arity == adtz.variant_count() + 1 {
      Trg::wire(ctr.aux_port(ctr_arity - 1).wire())
    } else {
      let w = ctr_arity - adtz.variant_count();
      let t = Tag::ctr_with_width(w);
      let port = Port::new(t, ctr.lab(), self.alloc(t.align()));
      for i in 0 .. w {
        self.link_wire_port(ctr.aux_port(adtz.variant_count() + i).wire(), port.aux_port(i));
      }
      Trg::port(port)
    };
    if adt.is(AdtZ) {
      self.link_trg(out, Trg::wire(ctr.aux_port(adtz.variant_index()).wire()));
    } else {
      self.link_trg_port(out, adt.aux_port(adt.tag().arity()));
      self.link_wire_port(ctr.aux_port(adtz.variant_index()).wire(), Port(adt.0 ^ 0b1000));
    }
    for i in 0 .. adtz.variant_index() {
      self.link_wire_port(ctr.aux_port(i).wire(), Port::ERA);
    }
    for i in adtz.variant_index() + 1 .. ctr_arity {
      self.link_wire_port(ctr.aux_port(i).wire(), Port::ERA);
    }
  }

  fn anni(&mut self, a: Port, b: Port) {
    // if a.tag().width() == 2 && b.tag().width() == 2 {
    //   return self.anni2(a, b);
    // }
    self.rwts.anni += 1;
    if a.tag() == b.tag() {
      for i in 0 .. a.tag().arity() {
        self.link_wire_wire(a.aux_port(i).wire(), b.aux_port(i).wire());
      }
      for i in a.tag().arity() .. a.tag().width() {
        self.free_wire(a.aux_port(i).wire());
        self.free_wire(b.aux_port(i).wire());
      }
    } else {
      let aw = a.tag().width();
      let bw = b.tag().width();
      let (a, b, aw, bw) = if aw > bw { (b, a, bw, aw) } else { (a, b, aw, bw) };
      let aws = aw - 1;
      let cw = bw - aws;
      let ct = Tag::ctr_with_width(cw);
      let c = Port::new(ct, a.lab(), self.alloc(ct.align()));
      for i in 0 .. aws {
        self.link_wire_wire(a.aux_port(i).wire(), b.aux_port(i).wire());
      }
      for i in 0 .. cw {
        self.link_wire_port(b.aux_port(aws + i).wire(), c.aux_port(i))
      }
      self.link_wire_port(a.aux_port(aws).wire(), c);
    }
  }

  fn comm(&mut self, a: Port, b: Port) {
    if a == Port::ERA || b == Port::ERA {
      self.rwts.eras += 1;
    } else {
      self.rwts.comm += 1;
    }
    trace!(self, a, b);
    let mut Bs = [const { MaybeUninit::<Port>::uninit() }; 8];
    let mut As = [const { MaybeUninit::<Port>::uninit() }; 8];
    let aa = a.tag().arity();
    let aw = a.tag().width();
    let ba = b.tag().arity();
    let bw = b.tag().width();
    let Bs = &mut Bs[0 .. aa as usize];
    let As = &mut As[0 .. ba as usize];
    if ba != 0 {
      for B in &mut *Bs {
        let addr = self.alloc(b.align());
        *B = MaybeUninit::new(b.with_addr(addr));
      }
    } else {
      Bs.fill_with(|| MaybeUninit::new(b.clone()));
    }
    if aa != 0 {
      for A in &mut *As {
        let addr = self.alloc(a.align());
        *A = MaybeUninit::new(a.with_addr(addr));
      }
    } else {
      As.fill_with(|| MaybeUninit::new(a.clone()));
    }
    for bi in 0 .. aa {
      for ai in 0 .. ba {
        unsafe {
          self.link_port_port(
            As.get_unchecked(ai as usize).assume_init_ref().aux_port(bi),
            Bs.get_unchecked(bi as usize).assume_init_ref().aux_port(ai),
          );
        }
      }
    }
    for i in aa .. aw {
      let t = a.aux_port(i).wire().load_target();
      for A in &*As {
        unsafe {
          A.assume_init_ref().aux_port(i).wire().set_target(t.clone());
        }
      }
    }
    for i in ba .. bw {
      let t = b.aux_port(i).wire().load_target();
      for B in &*Bs {
        unsafe {
          B.assume_init_ref().aux_port(i).wire().set_target(t.clone());
        }
      }
    }
    for i in 0 .. aa {
      unsafe {
        self.link_wire_port(a.aux_port(i).wire(), Bs.get_unchecked(i as usize).assume_init_read());
      }
    }
    for i in 0 .. ba {
      unsafe {
        self.link_wire_port(b.aux_port(i).wire(), As.get_unchecked(i as usize).assume_init_read());
      }
    }
  }
}
