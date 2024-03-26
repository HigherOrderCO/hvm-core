use arrayvec::ArrayVec;

use super::*;
use crate::{ops::Op, run::Lab, util::maybe_grow};

impl Host {
  /// Converts an ast net to a list of instructions to create the net.
  ///
  /// `get_def` must return the `Port` corresponding to a given `Ref` name.
  pub(crate) fn encode_def(&self, net: &Net) -> InterpretedDef {
    let mut def = InterpretedDef { instr: Vec::new(), trgs: 1 };
    let mut state = State { host: self, encoder: &mut def, scope: Default::default() };
    state.visit_net(net, TrgId::new(0));
    state.finish();
    def
  }

  /// Encode `tree` directly into `trg`, skipping the intermediate `Def`
  /// representation.
  pub fn encode_tree<M: Mode>(&self, net: &mut run::Net<M>, trg: run::Trg, tree: &Tree) {
    let mut state = State { host: self, encoder: net, scope: Default::default() };
    state.visit_tree(tree, trg);
    state.finish();
  }

  /// Encode the root of `ast_net` directly into `trg` and encode its redexes
  /// into `net` redex list.
  pub fn encode_net<M: Mode>(&self, net: &mut run::Net<M>, trg: run::Trg, ast_net: &Net) {
    let mut state = State { host: self, encoder: net, scope: Default::default() };
    state.visit_net(ast_net, trg);
    state.finish();
  }
}

struct State<'a, E: Encoder> {
  host: &'a Host,
  encoder: &'a mut E,
  scope: HashMap<&'a str, E::Trg>,
}

impl<'a, E: Encoder> State<'a, E> {
  fn finish(self) {
    assert!(self.scope.is_empty(), "unbound variables: {:?}", self.scope.keys());
  }
  fn visit_net(&mut self, net: &'a Net, trg: E::Trg) {
    self.visit_tree(&net.root, trg);
    net.redexes.iter().for_each(|(a, b)| self.visit_redex(a, b));
  }
  fn visit_redex(&mut self, a: &'a Tree, b: &'a Tree) {
    let (port, tree) = match (a, b) {
      (Tree::Era, t) | (t, Tree::Era) => (Port::ERA, t),
      (Tree::Ref { nam }, t) | (t, Tree::Ref { nam }) => (Port::new_ref(&self.host.defs[nam]), t),
      (Tree::Num { val }, t) | (t, Tree::Num { val }) => (Port::new_num(*val), t),
      (t, u) => {
        let (av, aw, bv, bw) = self.encoder.wires();
        self.visit_tree(t, av);
        self.visit_tree(u, bv);
        self.encoder.link(aw, bw);
        return;
      }
    };
    let trg = self.encoder.make_const(port);
    self.visit_tree(tree, trg);
  }
  fn visit_tree(&mut self, tree: &'a Tree, trg: E::Trg) {
    maybe_grow(move || match tree {
      Tree::Era => self.encoder.link_const(trg, Port::ERA),
      Tree::Num { val } => self.encoder.link_const(trg, Port::new_num(*val)),
      Tree::Ref { nam } => self.encoder.link_const(trg, Port::new_ref(&self.host.defs[nam])),
      Tree::Ctr { lab, ports } => {
        if ports.is_empty() {
          return self.visit_tree(&Tree::Era, trg);
        }
        if ports.len() == 1 {
          return self.visit_tree(&ports[0], trg);
        }
        if ports.len() == 2 {
          let (a, b) = self.encoder.ctr2(*lab, trg);
          self.visit_tree(&ports[0], a);
          self.visit_tree(&ports[1], b);
          return;
        }
        for (i, t) in self.encoder.ctrn(*lab, trg, ports.len() as u8).into_iter().enumerate() {
          self.visit_tree(&ports[i], t);
        }
      }
      Tree::Adt { lab, variant_index, variant_count, fields } => {
        for (i, t) in self
          .encoder
          .adtn(*lab, trg, *variant_index as u8, *variant_count as u8, fields.len() as u8)
          .into_iter()
          .enumerate()
        {
          self.visit_tree(&fields[i], t);
        }
      }
      Tree::Op { op, rhs: lft, out: rgt } => {
        let (l, r) = self.encoder.op(*op, trg);
        self.visit_tree(lft, l);
        self.visit_tree(rgt, r);
      }
      Tree::Mat { zero, succ, out } => {
        let (z, s, o) = self.encoder.mat(trg);
        self.visit_tree(zero, z);
        self.visit_tree(succ, s);
        self.visit_tree(out, o);
      }
      Tree::Var { nam } => match self.scope.entry(nam) {
        Entry::Occupied(e) => self.encoder.link(e.remove(), trg),
        Entry::Vacant(e) => {
          e.insert(trg);
        }
      },
    })
  }
}

trait Encoder {
  type Trg;
  fn link_const(&mut self, trg: Self::Trg, port: Port);
  fn link(&mut self, a: Self::Trg, b: Self::Trg);
  fn make_const(&mut self, port: Port) -> Self::Trg;
  fn ctr2(&mut self, lab: Lab, trg: Self::Trg) -> (Self::Trg, Self::Trg);
  fn ctrn(&mut self, lab: Lab, trg: Self::Trg, n: u8) -> ArrayVec<Self::Trg, 8>;
  fn adtn(
    &mut self,
    lab: Lab,
    trg: Self::Trg,
    variant_index: u8,
    variant_count: u8,
    arity: u8,
  ) -> ArrayVec<Self::Trg, 7>;
  fn op(&mut self, op: Op, trg: Self::Trg) -> (Self::Trg, Self::Trg);
  fn op_num(&mut self, op: Op, trg: Self::Trg, rhs: u64) -> Self::Trg;
  fn mat(&mut self, trg: Self::Trg) -> (Self::Trg, Self::Trg, Self::Trg);
  fn wires(&mut self) -> (Self::Trg, Self::Trg, Self::Trg, Self::Trg);
}

impl InterpretedDef {
  fn new_trg_id(&mut self) -> TrgId {
    let index = self.trgs;
    self.trgs += 1;
    TrgId::new(index)
  }
}

impl Encoder for InterpretedDef {
  type Trg = TrgId;
  fn link_const(&mut self, trg: Self::Trg, port: Port) {
    self.instr.push(Instruction::LinkConst { trg, port });
  }
  fn link(&mut self, a: Self::Trg, b: Self::Trg) {
    self.instr.push(Instruction::Link { a, b });
  }
  fn make_const(&mut self, port: Port) -> Self::Trg {
    let trg = self.new_trg_id();
    self.instr.push(Instruction::Const { trg, port });
    trg
  }
  fn ctr2(&mut self, lab: Lab, trg: Self::Trg) -> (Self::Trg, Self::Trg) {
    let lft = self.new_trg_id();
    let rgt = self.new_trg_id();
    self.instr.push(Instruction::Ctr2 { lab, trg, lft, rgt });
    (lft, rgt)
  }
  fn ctrn(&mut self, lab: Lab, trg: Self::Trg, n: u8) -> ArrayVec<Self::Trg, 8> {
    let mut ports = ArrayVec::new();
    for _ in 0 .. n {
      ports.push(self.new_trg_id());
    }
    self.instr.push(Instruction::CtrN { lab, trg, ports: ports.clone() });
    ports
  }
  fn adtn(
    &mut self,
    lab: Lab,
    trg: Self::Trg,
    variant_index: u8,
    variant_count: u8,
    arity: u8,
  ) -> ArrayVec<Self::Trg, 7> {
    let mut fields = ArrayVec::new();
    for _ in 0 .. arity {
      fields.push(self.new_trg_id());
    }
    self.instr.push(Instruction::AdtN { lab, trg, variant_index, variant_count, fields: fields.clone() });
    fields
  }
  fn op(&mut self, op: Op, trg: Self::Trg) -> (Self::Trg, Self::Trg) {
    let rhs = self.new_trg_id();
    let out = self.new_trg_id();
    self.instr.push(Instruction::Op { op, trg, rhs, out });
    (rhs, out)
  }
  fn op_num(&mut self, op: Op, trg: Self::Trg, rhs: u64) -> Self::Trg {
    let out = self.new_trg_id();
    self.instr.push(Instruction::OpNum { op, trg, rhs, out });
    out
  }
  fn mat(&mut self, trg: Self::Trg) -> (Self::Trg, Self::Trg, Self::Trg) {
    let zero = self.new_trg_id();
    let succ = self.new_trg_id();
    let out = self.new_trg_id();
    self.instr.push(Instruction::Mat { trg, zero, succ, out });
    (zero, succ, out)
  }
  fn wires(&mut self) -> (Self::Trg, Self::Trg, Self::Trg, Self::Trg) {
    let av = self.new_trg_id();
    let aw = self.new_trg_id();
    let bv = self.new_trg_id();
    let bw = self.new_trg_id();
    self.instr.push(Instruction::Wires { av, aw, bv, bw });
    (av, aw, bv, bw)
  }
}

impl<'a, M: Mode> Encoder for run::Net<'a, M> {
  type Trg = run::Trg;
  fn link_const(&mut self, trg: Self::Trg, port: Port) {
    self.link_trg_port(trg, port)
  }
  fn link(&mut self, a: Self::Trg, b: Self::Trg) {
    self.link_trg(a, b)
  }
  fn make_const(&mut self, port: Port) -> Self::Trg {
    run::Trg::port(port)
  }
  fn ctr2(&mut self, lab: Lab, trg: Self::Trg) -> (Self::Trg, Self::Trg) {
    self.do_ctr2(lab, trg)
  }
  fn ctrn(&mut self, lab: Lab, trg: Self::Trg, n: u8) -> ArrayVec<Self::Trg, 8> {
    self.do_ctrn(lab, trg, n)
  }
  fn adtn(
    &mut self,
    lab: Lab,
    trg: Self::Trg,
    variant_index: u8,
    variant_count: u8,
    arity: u8,
  ) -> ArrayVec<Self::Trg, 7> {
    self.do_adtn(lab, trg, variant_index, variant_count, arity)
  }
  fn op(&mut self, op: Op, trg: Self::Trg) -> (Self::Trg, Self::Trg) {
    self.do_op(op, trg)
  }
  fn op_num(&mut self, op: Op, trg: Self::Trg, rhs: u64) -> Self::Trg {
    self.do_op_num(op, trg, rhs)
  }
  fn mat(&mut self, trg: Self::Trg) -> (Self::Trg, Self::Trg, Self::Trg) {
    self.do_mat(trg)
  }
  fn wires(&mut self) -> (Self::Trg, Self::Trg, Self::Trg, Self::Trg) {
    self.do_wires()
  }
}
