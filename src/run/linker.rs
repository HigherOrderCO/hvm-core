use st3::lifo::Worker;

use super::*;

/// Stores extra data needed about the nodes when in lazy mode. (In strict mode,
/// this is unused.)
pub(super) struct Header {
  /// the principal port of this node
  pub(super) this: Port,
  /// the port connected to the principal port of this node
  pub(super) targ: Port,
}

pub type Redex = (Port, Port);

/// Manages linking ports and wires within the net.
///
/// When threads interfere, this uses the atomic linking algorithm described in
/// `paper/`.
///
/// Linking wires must be done atomically, but linking ports can be done
/// non-atomically (because they must be locked).
pub struct Linker<'h, M: Mode> {
  pub(super) allocator: Allocator<'h>,
  pub rwts: Rewrites,
  pub redexes: RedexQueue,
  headers: IntMap<Addr, Header>,
  _mode: PhantomData<M>,
}

pub const WORKER_QUEUE_CAPACITY: usize = 1 << 22;

deref!({<'h, M: Mode>} Linker<'h, M> => self.allocator: Allocator<'h>);

impl<'h, M: Mode> Linker<'h, M> {
  pub fn new(heap: &'h Heap) -> Self {
    Linker::new_with_worker(heap, Worker::new(WORKER_QUEUE_CAPACITY))
  }
  pub(super) fn new_with_worker(heap: &'h Heap, worker: Worker<Redex>) -> Self {
    Linker {
      allocator: Allocator::new(heap),
      redexes: RedexQueue { fast: Vec::new(), slow: worker },
      rwts: Default::default(),
      headers: Default::default(),
      _mode: PhantomData,
    }
  }

  /// Links two ports.
  #[inline(always)]
  pub fn link_port_port(&mut self, a_port: Port, b_port: Port) {
    trace!(self, a_port, b_port);
    if a_port.is_principal() && b_port.is_principal() {
      self.redux(a_port, b_port);
    } else {
      self.half_link_port_port(a_port.clone(), b_port.clone());
      self.half_link_port_port(b_port, a_port);
    }
  }

  /// Links two wires.
  #[inline(always)]
  pub fn link_wire_wire(&mut self, a_wire: Wire, b_wire: Wire) {
    trace!(self, a_wire, b_wire);
    let a_port = a_wire.lock_target();
    let b_port = b_wire.lock_target();
    trace!(self, a_port, b_port);
    if a_port.is_principal() && b_port.is_principal() {
      self.free_wire(a_wire);
      self.free_wire(b_wire);
      self.redux(a_port, b_port);
    } else {
      self.half_link_wire_port(a_port.clone(), a_wire, b_port.clone());
      self.half_link_wire_port(b_port, b_wire, a_port);
    }
  }

  /// Links a wire to a port.
  #[inline(always)]
  pub fn link_wire_port(&mut self, a_wire: Wire, b_port: Port) {
    trace!(self, a_wire, b_port);
    let a_port = a_wire.lock_target();
    trace!(self, a_port);
    if a_port.is_principal() && b_port.is_principal() {
      self.free_wire(a_wire);
      self.redux(a_port, b_port);
    } else {
      self.half_link_wire_port(a_port.clone(), a_wire, b_port.clone());
      self.half_link_port_port(b_port, a_port);
    }
  }

  /// Pushes an active pair to the redex queue; `a` and `b` must both be
  /// principal ports.
  #[inline(always)]
  pub fn redux(&mut self, a: Port, b: Port) {
    trace!(self, a, b);
    debug_assert!(!(a.is(Tag::Var) || a.is(Tag::Red) || b.is(Tag::Var) || b.is(Tag::Red)));
    if a.is_skippable() && b.is_skippable() {
      self.rwts.eras += 1;
    } else if !M::LAZY {
      // Prioritize redexes that do not allocate memory,
      // to prevent OOM errors that can be avoided
      // by reducing redexes in a different order (see #91)
      if redex_would_shrink(&a, &b) {
        self.redexes.fast.push((a, b));
      } else {
        self.redexes.slow.push((a, b)).expect("exceeded redex queue capacity");
      }
    } else {
      self.set_header(a.clone(), b.clone());
      self.set_header(b.clone(), a.clone());
    }
  }

  /// Half-links `a_port` to `b_port`, without linking `b_port` back to
  /// `a_port`.
  #[inline(always)]
  fn half_link_port_port(&mut self, a_port: Port, b_port: Port) {
    trace!(self, a_port, b_port);
    if a_port.is(Tag::Var) {
      a_port.wire().set_target(b_port);
    } else {
      if M::LAZY {
        self.set_header(a_port, b_port);
      }
    }
  }

  /// Half-links a foreign `a_port` (taken from `a_wire`) to `b_port`, without
  /// linking `b_port` back to `a_port`.
  #[inline(always)]
  fn half_link_wire_port(&mut self, a_port: Port, a_wire: Wire, b_port: Port) {
    trace!(self, a_port, a_wire, b_port);
    // If 'a_port' is a var...
    if a_port.is(Tag::Var) {
      let got = a_port.wire().cas_target(a_wire.as_var(), b_port.clone());
      // Attempts to link using a compare-and-swap.
      if got.is_ok() {
        trace!(self, "cas ok");
        self.free_wire(a_wire);
      // If the CAS failed, resolve by using redirections.
      } else {
        let got = got.unwrap_err();
        trace!(self, "cas fail", got);
        if b_port.is(Tag::Var) {
          let port = b_port.redirect();
          a_wire.set_target(port);
          //self.resolve_redirect_var(a_port, a_wire, b_port);
        } else if b_port.is_principal() {
          a_wire.set_target(b_port.clone());
          self.resolve_redirect_pri(a_port, a_wire, b_port);
        } else {
          unreachable!();
        }
      }
    } else {
      self.free_wire(a_wire);
      if M::LAZY {
        self.set_header(a_port, b_port);
      }
    }
  }

  /// Resolves redirects when 'b_port' is a principal port.
  fn resolve_redirect_pri(&mut self, mut a_port: Port, a_wire: Wire, b_port: Port) {
    trace!(self);
    loop {
      trace!(self, a_port, a_wire, b_port);
      // Peek the target, which may not be owned by us.
      let mut t_wire = a_port.wire();
      let mut t_port = t_wire.load_target();
      trace!(self, t_port);
      // If it is taken, we wait.
      if t_port == Port::LOCK {
        spin_loop();
        continue;
      }
      // If target is a redirection, we own it. Clear and move forward.
      if t_port.is(Tag::Red) {
        self.free_wire(t_wire);
        a_port = t_port;
        continue;
      }
      // If target is a variable, we don't own it. Try replacing it.
      if t_port.is(Tag::Var) {
        if t_wire.cas_target(t_port.clone(), b_port.clone()).is_ok() {
          trace!(self, "var cas ok");
          // Clear source location.
          // self.half_free(a_wire.addr());
          // Collect the orphaned backward path.
          t_wire = t_port.wire();
          t_port = t_wire.load_target();
          while t_port != Port::LOCK && t_port.is(Tag::Red) {
            trace!(self, t_wire, t_port);
            self.free_wire(t_wire);
            t_wire = t_port.wire();
            // if t_wire == a_wire {
            //   break;
            // }
            t_port = t_wire.load_target();
          }
          return;
        }
        trace!(self, "var cas fail");
        // If the CAS failed, the var changed, so we try again.
        continue;
      }

      // If it is a node, two threads will reach this branch.
      if t_port.is_principal() || t_port == Port::GONE {
        // Sort references, to avoid deadlocks.
        let x_wire = if a_wire < t_wire { a_wire.clone() } else { t_wire.clone() };
        let y_wire = if a_wire < t_wire { t_wire.clone() } else { a_wire.clone() };
        trace!(self, x_wire, y_wire);
        // Swap first reference by Ptr::GONE placeholder.
        let x_port = x_wire.swap_target(Port::GONE);
        // First to arrive creates a redex.
        if x_port != Port::GONE {
          let y_port = y_wire.swap_target(Port::GONE);
          trace!(self, "fst", x_wire, y_wire, x_port, y_port);
          self.redux(x_port, y_port);
          return;
        // Second to arrive clears up the memory.
        } else {
          trace!(self, "snd !!!", x_wire, y_wire);
          self.free_wire(x_wire);
          while y_wire.cas_target(Port::GONE, Port::LOCK).is_err() {
            spin_loop();
          }
          self.free_wire(y_wire);
          return;
        }
      }
      // Shouldn't be reached.
      trace!(self, t_port, a_wire, a_port, b_port);
      unreachable!()
    }
  }

  /// Resolves redirects when 'b_port' is an aux port.
  // TODO: this is currently broken
  #[allow(unused)]
  fn resolve_redirect_var(&mut self, _: Port, _: Wire, b_port: Port) {
    loop {
      let ste_wire = b_port.clone().wire();
      let ste_port = ste_wire.load_target();
      if ste_port.is(Tag::Var) {
        let trg_wire = ste_port.wire();
        let trg_port = trg_wire.load_target();
        if trg_port.is(Tag::Red) {
          let neo_port = trg_port.unredirect();
          if ste_wire.cas_target(ste_port, neo_port).is_ok() {
            self.free_wire(trg_wire);
            continue;
          }
        }
      }
      break;
    }
  }
}

/// Part of the net to link to; either a wire or a port.
///
/// To store this compactly, we reuse the [`Red`] tag to indicate if this is a
/// wire.
#[derive(Clone)]
pub struct Trg(pub(crate) Port);

impl fmt::Debug for Trg {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    if self.is_wire() { self.clone().as_wire().fmt(f) } else { self.0.fmt(f) }
  }
}

impl Trg {
  /// Creates a `Trg` from a port.
  #[inline(always)]
  pub fn port(port: Port) -> Self {
    Trg(port)
  }

  /// Creates a `Trg` from a wire.
  #[inline(always)]
  pub fn wire(wire: Wire) -> Self {
    Trg(Port(wire.0 as u64))
  }

  #[inline(always)]
  pub(super) fn is_wire(&self) -> bool {
    self.0.is(Tag::Red)
  }

  #[inline(always)]
  pub(super) fn as_wire(self) -> Wire {
    Wire(self.0.0 as _)
  }

  #[inline(always)]
  pub(super) fn as_port(self) -> Port {
    self.0
  }

  /// Access the target port; if this trg is already a port, this does nothing,
  /// but if it is a wire, it loads the target.
  ///
  /// The returned port is only normative if it is principal; if it is a var or
  /// a redirect, it must not be used for linking, and the original `Trg` should
  /// be used instead.
  #[inline(always)]
  pub fn target(&self) -> Port {
    if self.is_wire() { self.clone().as_wire().load_target() } else { self.0.clone() }
  }
}

impl<'h, M: Mode> Linker<'h, M> {
  /// Links a `Trg` to a port, delegating to the appropriate method based on the
  /// type of `a`.
  #[inline(always)]
  pub fn link_trg_port(&mut self, a: Trg, b: Port) {
    match a.is_wire() {
      true => self.link_wire_port(a.as_wire(), b),
      false => self.link_port_port(a.as_port(), b),
    }
  }

  /// Links two `Trg`s, delegating to the appropriate method based on the types
  /// of `a` and `b`.
  #[inline(always)]
  pub fn link_trg(&mut self, a: Trg, b: Trg) {
    match (a.is_wire(), b.is_wire()) {
      (true, true) => self.link_wire_wire(a.as_wire(), b.as_wire()),
      (true, false) => self.link_wire_port(a.as_wire(), b.as_port()),
      (false, true) => self.link_wire_port(b.as_wire(), a.as_port()),
      (false, false) => self.link_port_port(a.as_port(), b.as_port()),
    }
  }

  pub(super) fn get_header(&self, addr: Addr) -> &Header {
    assert!(M::LAZY);
    &self.headers[&addr]
  }

  pub(super) fn set_header(&mut self, ptr: Port, trg: Port) {
    assert!(M::LAZY);
    trace!(self, ptr, trg);
    if ptr.is_full_node() {
      self.headers.insert(ptr.addr(), Header { this: ptr, targ: trg });
    }
  }

  pub(super) fn get_target_full(&self, port: Port) -> Port {
    assert!(M::LAZY);
    if !port.is_principal() {
      return port.wire().load_target();
    }
    self.headers[&port.addr()].targ.clone()
  }
}

#[derive(Debug)]
pub struct RedexQueue {
  pub(super) fast: Vec<Redex>,
  pub(super) slow: Worker<Redex>,
}

impl RedexQueue {
  /// Returns the highest-priority redex in the queue, if any
  #[inline(always)]
  pub fn pop(&mut self) -> Option<(Port, Port)> {
    self.fast.pop().or_else(|| self.slow.pop())
  }
  #[inline(always)]
  pub fn is_empty(&self) -> bool {
    self.fast.is_empty() && self.slow.is_empty()
  }
  #[inline(always)]
  pub fn drain(&mut self) -> impl Iterator<Item = (Port, Port)> + '_ {
    self.fast.drain(..).chain(self.slow.drain(|x| x).unwrap())
  }
  #[inline(always)]
  pub fn iter(&self) -> impl Iterator<Item = &(Port, Port)> {
    // TODO
    // todo!();
    [].into_iter()
    // self.fast.iter().chain(self.slow.)
  }
  #[inline(always)]
  pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut (Port, Port)> {
    todo!();
    [].into_iter()
    // self.fast.iter_mut().chain(self.slow.iter_mut())
  }
  #[inline(always)]
  pub fn clear(&mut self) {
    todo!();
    // self.fast.clear();
    // self.slow.clear();
  }
}

// Returns whether a redex does not allocate memory
fn redex_would_shrink(a: &Port, b: &Port) -> bool {
  (*a == Port::ERA || *b == Port::ERA)
    || (a.tag() == Tag::Ctr && b.tag() == Tag::Ctr && a.lab() == b.lab())
    || (!(a.tag() == Tag::Ref || b.tag() == Tag::Ref) && (a.tag() == Tag::Num || b.tag() == Tag::Num))
}
