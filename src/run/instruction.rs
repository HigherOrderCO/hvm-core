use arrayvec::ArrayVec;

use super::*;

/// Each instruction corresponds to a fragment of a net that has a native
/// implementation.
///
/// These net fragments may have several free ports, which are each represented
/// with [`TrgId`]s.
///
/// Each `TrgId` of an instruction has an associated polarity -- it can either
/// be an input or an output. Because the underlying interaction net model we're
/// using does not have polarity, we also need instructions for linking out-out
/// or in-in.
///
/// Linking two outputs can be done with [`Instruction::Link`], which creates a
/// "cup" wire.
///
/// Linking two inputs is more complicated, due to the way locking works. It can
/// be done with [`Instruction::Wires`], which creates two "cap" wires. One half
/// of each cap can be used for each input. Once those inputs have been fully
/// unlocked, the other halves of each cap can be linked with
/// [`Instruction::Link`]. For example:
/// ```rust,ignore
/// let (av, aw, bv, bw) = net.do_wires();
/// some_subnet(net, av, bv);
/// net.link(aw, bw);
/// ```
///
/// ```js
/// let a  =5;
/// ```
///
/// Each instruction documents both the native implementation and the polarity
/// of each `TrgId`.
///
/// Some instructions take a [`Port`]; these must never be statically-valid
/// ports -- that is, [`Ref`] or [`Num`] ports.
#[derive(Debug, Clone)]
pub enum Instruction {
  /// ```rust,ignore
  /// let trg = Trg::port(port);
  /// ```
  Const { trg: TrgId, port: Port },
  /// ```rust,ignore
  /// net.link_trg(a, b);
  /// ```
  Link { a: TrgId, b: TrgId },
  /// ```rust,ignore
  /// net.link_trg(trg, Trg::port(port));
  /// ```
  LinkConst { trg: TrgId, port: Port },
  /// See [`Net::do_ctr2`].
  /// ```rust,ignore
  /// let (lft, rgt) = net.do_ctr2(lab, trg);
  /// ```
  Ctr2 { lab: Lab, trg: TrgId, lft: TrgId, rgt: TrgId },
  /// See [`Net::do_ctrn`].
  /// ```rust,ignore
  /// let ports = net.do_ctrn(lab, trg, ports.len());
  /// ```
  CtrN { lab: Lab, trg: TrgId, ports: ArrayVec<TrgId, 8> },
  /// See [`Net::do_ctrn`].
  /// ```rust,ignore
  /// let ports = net.do_ctrn(lab, trg, variant_index, variant_count, fields.len() + 1);
  /// ```
  AdtN { lab: Lab, trg: TrgId, variant_index: u8, variant_count: u8, fields: ArrayVec<TrgId, 7> },
  /// See [`Net::do_op`].
  /// ```rust,ignore
  /// let (rhs, out) = net.do_op(lab, trg);
  /// ```
  Op { op: Op, trg: TrgId, rhs: TrgId, out: TrgId },
  /// See [`Net::do_op_num`].
  /// ```rust,ignore
  /// let out = net.do_op_num(lab, trg, rhs);
  /// ```
  OpNum { op: Op, trg: TrgId, rhs: u64, out: TrgId },
  /// See [`Net::do_mat`].
  /// ```rust,ignore
  /// let (zero, succ, out) = net.do_mat(trg);
  /// ```
  Mat { trg: TrgId, zero: TrgId, succ: TrgId, out: TrgId },
  /// See [`Net::do_wires`].
  /// ```rust,ignore
  /// let (av, aw, bv, bw) = net.do_wires();
  /// ```
  Wires { av: TrgId, aw: TrgId, bv: TrgId, bw: TrgId },
}

/// An index to a [`Trg`] in an [`Instruction`]. These essentially serve the
/// function of registers.
///
/// When compiled, each `TrgId` will be compiled to a variable.
///
/// When interpreted, the `TrgId` serves as an index into a vector.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TrgId {
  /// Instead of storing the index directly, we store the byte offset, to save a
  /// shift instruction when indexing into the `Trg` vector in interpreted mode.
  ///
  /// This is never `index * size_of::<Trg>()`.
  pub(super) byte_offset: usize,
}

impl TrgId {
  pub fn new(index: usize) -> Self {
    TrgId { byte_offset: index * size_of::<Trg>() }
  }
  pub fn index(&self) -> usize {
    self.byte_offset / size_of::<Trg>()
  }
}

impl fmt::Display for TrgId {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "t{}", self.index())
  }
}

impl fmt::Debug for TrgId {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "TrgId({})", self.index())
  }
}

impl<'a, M: Mode> Net<'a, M> {
  /// `trg ~ {#lab x y}`
  #[inline(never)]
  pub(crate) fn do_ctr2(&mut self, lab: Lab, trg: Trg) -> (Trg, Trg) {
    let port = trg.target();
    if !M::LAZY && port.is(Tag::Ctr2) && port.lab() == lab {
      trace!(self, "fast");
      self.free_trg(trg);
      let node = port.consume_node();
      self.rwts.anni += 1;
      (Trg::wire(node.p1), Trg::wire(node.p2))
    // TODO: fast copy?
    } else if false && !M::LAZY && port.is(Tag::Num) || port.is(Tag::Ref) && lab >= port.lab() {
      self.rwts.comm += 1;
      self.free_trg(trg);
      (Trg::port(port.clone()), Trg::port(port))
    } else {
      let n = self.create_node(Ctr2, lab);
      self.link_trg_port(trg, n.p0);
      (Trg::port(n.p1), Trg::port(n.p2))
    }
  }

  /// `trg ~ {#lab ...}`
  #[inline(never)]
  pub(crate) fn do_ctrn(&mut self, lab: Lab, trg: Trg, n: u8) -> ArrayVec<Trg, 8> {
    let tag = Tag::ctr_with_width(n);
    let align = tag.align();
    let addr = self.alloc(align);
    let mut out = ArrayVec::new();
    self.link_trg_port(trg, Port::new(tag, lab, addr));
    for i in 0 .. n {
      unsafe { out.push_unchecked(Trg::port(Port::new_var(align, addr.offset(i as usize)))) }
    }
    out
  }

  /// `trg ~ {lab:idx:count ...}`
  #[inline(never)]
  pub(crate) fn do_adtn(
    &mut self,
    lab: Lab,
    trg: Trg,
    variant_index: u8,
    variant_count: u8,
    arity: u8,
  ) -> ArrayVec<Trg, 7> {
    let adtz = Port::new_adtz(variant_index, variant_count);
    let mut out = ArrayVec::new();
    if arity == 0 {
      self.link_trg_port(trg, adtz);
    } else {
      let width = arity + 1;
      let tag = Tag::adt_with_width(width);
      let align = tag.align();
      let addr = self.alloc(align);
      self.link_trg_port(trg, Port::new(tag, lab, addr));
      for i in 0 .. arity {
        unsafe { out.push_unchecked(Trg::port(Port::new_var(align, addr.offset(i as usize)))) }
      }
      Wire::new(align, addr.offset(arity as usize)).set_target(adtz);
    }
    out
  }

  /// `trg ~ <op x y>`
  #[inline(never)]
  pub(crate) fn do_op(&mut self, op: Op, trg: Trg) -> (Trg, Trg) {
    trace!(self.tracer, op, trg);
    let port = trg.target();
    if !M::LAZY && port.tag() == Num {
      self.free_trg(trg);
      let n = self.create_node(Op, op.swap() as Lab);
      n.p1.wire().set_target(Port::new_num(port.num()));
      (Trg::port(n.p0), Trg::port(n.p2))
    } else if !M::LAZY && port == Port::ERA {
      self.free_trg(trg);
      (Trg::port(Port::ERA), Trg::port(Port::ERA))
    } else {
      let n = self.create_node(Op, op as Lab);
      self.link_trg_port(trg, n.p0);
      (Trg::port(n.p1), Trg::port(n.p2))
    }
  }

  /// `trg ~ <op #b x>`
  #[inline(never)]
  pub(crate) fn do_op_num(&mut self, op: Op, trg: Trg, rhs: u64) -> Trg {
    let port = trg.target();
    if !M::LAZY && port.tag() == Num {
      self.rwts.oper += 1;
      self.free_trg(trg);
      Trg::port(Port::new_num(op.op(port.num(), rhs)))
    } else if !M::LAZY && port == Port::ERA {
      self.free_trg(trg);
      Trg::port(Port::ERA)
    } else {
      let n = self.create_node(Op, op as Lab);
      self.link_trg_port(trg, n.p0);
      n.p1.wire().set_target(Port::new_num(rhs));
      Trg::port(n.p2)
    }
  }

  /// `trg ~ ?<x y z>`
  #[inline(never)]
  pub(crate) fn do_mat(&mut self, trg: Trg) -> (Trg, Trg, Trg) {
    let m = self.alloc(Align4);
    let m0 = Port::new(Mat, 0, m);
    let m1 = m0.aux_port(0);
    let m2 = m0.aux_port(1);
    let m3 = m0.aux_port(2);
    self.link_trg_port(trg, m0);
    (Trg::port(m1), Trg::port(m2), Trg::port(m3))
  }

  #[cfg(todo)]
  /// `trg ~ ?<x y out>`
  #[inline(never)]
  pub(crate) fn do_mat(&mut self, trg: Trg, out: Trg) -> (Trg, Trg) {
    let port = trg.target();
    if trg.target().is(Tag::Num) {
      self.rwts.oper += 1;
      self.free_trg(trg);
      let num = port.num();
      if num == 0 {
        (out, Trg::port(Port::ERA))
      } else {
        let c2 = self.create_node(Ctr2, 0);
        c2.p1.wire().set_target(Port::new_num(num - 1));
        self.link_trg_port(out, c2.p2);
        (Trg::port(Port::ERA), Trg::port(c2.p0))
      }
    } else if port == Port::ERA {
      self.link_trg_port(out, Port::ERA);
      (Trg::port(Port::ERA), Trg::port(Port::ERA))
    } else {
      let m = self.alloc(Align4);
      let m0 = Port::new(Mat, 0, m);
      let m1 = m0.aux_port(0);
      let m2 = m0.aux_port(1);
      let m3 = m0.aux_port(2);
      self.link_trg_port(out, m3);
      self.link_trg_port(trg, m0);
      (Trg::port(m1), Trg::port(m2))
    }
  }

  #[inline(never)]
  pub(crate) fn do_wires(&mut self) -> (Trg, Trg, Trg, Trg) {
    let a = self.alloc(Align2);
    let b = a.offset(1);
    (
      Trg::port(Port::new_var(Align2, a.clone())),
      Trg::wire(Wire::new(Align2, a)),
      Trg::port(Port::new_var(Align2, b.clone())),
      Trg::wire(Wire::new(Align2, b)),
    )
  }

  #[cfg(todo)]
  /// `trg ~ ?<(x (y z)) out>`
  #[inline(never)]
  #[allow(unused)] // TODO: emit this instruction
  pub(crate) fn do_mat_con_con(&mut self, trg: Trg, out: Trg) -> (Trg, Trg, Trg) {
    let port = trg.target();
    if !M::LAZY && trg.target().is(Tag::Num) {
      self.rwts.oper += 1;
      self.free_trg(trg);
      let num = port.num();
      if num == 0 {
        (out, Trg::port(Port::ERA), Trg::port(Port::ERA))
      } else {
        (Trg::port(Port::ERA), Trg::port(Port::new_num(num - 1)), out)
      }
    } else if !M::LAZY && port == Port::ERA {
      self.link_trg_port(out, Port::ERA);
      (Trg::port(Port::ERA), Trg::port(Port::ERA), Trg::port(Port::ERA))
    } else {
      let m = self.create_node(Mat, 0);
      let c1 = self.create_node(Ctr, 0);
      let c2 = self.create_node(Ctr, 0);
      self.link_port_port(m.p1, c1.p0);
      self.link_port_port(c1.p2, c2.p0);
      self.link_trg_port(out, m.p2);
      (Trg::port(c1.p1), Trg::port(c2.p1), Trg::port(c2.p2))
    }
  }
}
