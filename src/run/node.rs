use super::*;

/// See [`Port::traverse_node`].
pub struct TraverseNode {
  pub tag: Tag,
  pub lab: Lab,
  pub p1: Wire,
  pub p2: Wire,
}

impl Port {
  #[inline(always)]
  pub fn consume_node(self) -> TraverseNode {
    self.traverse_node()
  }

  #[inline(always)]
  pub fn traverse_node(self) -> TraverseNode {
    TraverseNode {
      tag: self.tag(),
      lab: self.lab(),
      p1: Wire::new(self.align(), self.addr()),
      p2: Wire::new(self.align(), self.addr().other(Align1)),
    }
  }
}

pub struct CreatedNode {
  pub p0: Port,
  pub p1: Port,
  pub p2: Port,
}

impl<'a, M: Mode> Net<'a, M> {
  #[inline(always)]
  pub fn create_node(&mut self, tag: Tag, lab: Lab) -> CreatedNode {
    assert_eq!(tag.width(), 2);
    let addr = self.alloc(tag.align());
    CreatedNode {
      p0: Port::new(tag, lab, addr.clone()),
      p1: Port::new_var(Align2, addr),
      p2: Port::new_var(Align2, addr.other(Align1)),
    }
  }

  /// Creates a wire an aux port pair.
  #[inline(always)]
  pub fn create_wire(&mut self) -> (Wire, Port) {
    let addr = self.alloc(Align2);
    (Wire::new(Align1, addr), Port::new_var(Align1, addr))
  }

  /// Creates a wire pointing to a given port; sometimes necessary to avoid
  /// deadlock.
  #[inline(always)]
  pub fn create_wire_to(&mut self, port: Port) -> Wire {
    let addr = self.alloc(Align1);
    let wire = Wire::new(Align1, addr);
    self.link_port_port(port, wire.as_var());
    wire
  }
}
