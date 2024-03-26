use self::run::{AdtN, CtrN};

use super::*;
use crate::util::maybe_grow;

impl Host {
  /// Creates an ast tree from a wire in a runtime net.
  pub fn readback_tree(&self, wire: &run::Wire) -> Tree {
    ReadbackState { host: self, vars: Default::default(), var_id: 0 .. }.read_wire(wire.clone())
  }

  /// Creates an ast net from a runtime net.
  ///
  /// Note that vicious circles and disconnected subnets will not be in the
  /// resulting ast net, as it is impossible to read these back from the runtime
  /// net representation. In the case of vicious circles, this may result in
  /// unbound variables.
  pub fn readback<M: Mode>(&self, rt_net: &run::Net<M>) -> Net {
    let mut state = ReadbackState { host: self, vars: Default::default(), var_id: 0 .. };
    let mut net = Net::default();

    net.root = state.read_wire(rt_net.root.clone());
    for (a, b) in rt_net.redexes.iter() {
      net.redexes.push((state.read_port(a.clone(), None), state.read_port(b.clone(), None)))
    }

    net
  }
}

/// See [`Host::readback`].
struct ReadbackState<'a> {
  host: &'a Host,
  vars: HashMap<Addr, usize>,
  var_id: RangeFrom<usize>,
}

impl<'a> ReadbackState<'a> {
  /// Reads a tree out from a given `wire`.
  fn read_wire(&mut self, wire: Wire) -> Tree {
    let port = wire.load_target();
    self.read_port(port, Some(wire))
  }
  /// Reads a tree out from a given `port`. If this is a var port, the
  /// `wire` this port was reached from must be supplied to key into the
  /// `vars` map.
  fn read_port(&mut self, port: Port, wire: Option<Wire>) -> Tree {
    maybe_grow(move || match port.tag() {
      Tag::Var | Tag::Red => {
        // todo: resolve redirects
        let key = wire.unwrap().addr().min(port.addr());
        Tree::Var {
          nam: create_var(match self.vars.entry(key) {
            Entry::Occupied(e) => e.remove(),
            Entry::Vacant(e) => *e.insert(self.var_id.next().unwrap()),
          }),
        }
      }
      Tag::Ref if port == Port::ERA => Tree::Era,
      Tag::Ref => Tree::Ref { nam: self.host.back[&port.addr()].clone() },
      Tag::Num => Tree::Num { val: port.num() },
      Tag::Op => {
        let op = port.op();
        let node = port.traverse_node();
        Tree::Op { op, rhs: Box::new(self.read_wire(node.p1)), out: Box::new(self.read_wire(node.p2)) }
      }
      // Tag::Ctr2 => {
      //   let node = port.traverse_node();
      //   Tree::Ctr { lab: node.lab, ports: vec![self.read_wire(node.p1), self.read_wire(node.p2)] }
      // }
      Tag::Mat => {
        let zero = Box::new(self.read_wire(port.aux_port(0).wire()));
        let succ = Box::new(self.read_wire(port.aux_port(1).wire()));
        let out = Box::new(self.read_wire(port.aux_port(2).wire()));
        Tree::Mat { zero, succ, out }
      }
      CtrN!() => Tree::Ctr {
        lab: port.lab(),
        ports: (0 .. port.tag().width()).map(|i| self.read_wire(port.aux_port(i).wire())).collect(),
      },
      AdtN!() | Tag::AdtZ => {
        let adtz =
          if port.is(Tag::AdtZ) { port.clone() } else { port.aux_port(port.tag().arity()).wire().load_target() };
        Tree::Adt {
          lab: port.lab(),
          variant_index: adtz.variant_index() as usize,
          variant_count: adtz.variant_count() as usize,
          fields: (0 .. port.tag().arity()).map(|i| self.read_wire(port.aux_port(i).wire())).collect(),
        }
      }
    })
  }
}
