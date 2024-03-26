use super::*;

/// A wire in the interaction net.
///
/// More accurately, this is a *directed view* of a wire. If ports `a` and `b`
/// are connected, then the wire leaving `a` and the wire leaving `b` are the
/// same wire, but viewed from opposite directions.
///
/// This is represented by a pointer to an `AtomicU64` storing the *target* of
/// the wire -- the port on the other side. (The target of the wire leaving `a`
/// is `b`.)
///
/// Changes to the target are handled by the linker.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[must_use]
pub struct Wire(pub u64);

impl fmt::Debug for Wire {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "{:012x?}", self.0 as usize)
  }
}

unsafe impl Send for Wire {}
unsafe impl Sync for Wire {}

impl Wire {
  #[inline(always)]
  pub fn addr(&self) -> Addr {
    Addr((self.0 & 0x0000_FFFF_FFFF_FFFF) as usize)
  }

  #[inline(always)]
  pub fn alloc_align(&self) -> Align {
    unsafe { Align::from_unchecked((self.0 >> 48) as u8) }
  }

  #[inline(always)]
  pub fn new(alloc_align: Align, addr: Addr) -> Wire {
    Wire(((alloc_align as u64) << 48) | (addr.0 as u64))
  }

  #[inline(always)]
  fn target<'a>(&self) -> &'a AtomicU64 {
    if cfg!(feature = "_fuzz") {
      assert_ne!(self.addr().0, 0xfffffffffff0u64 as usize);
      assert_ne!(self.0, 0);
    }
    unsafe { &*(self.addr().0 as *const _) }
  }

  #[inline(always)]
  pub fn load_target(&self) -> Port {
    let port = Port(self.target().load(Relaxed));
    if cfg!(feature = "_fuzz") {
      assert_ne!(port, Port::FREE_1);
    }
    port
  }

  #[inline(always)]
  pub fn set_target(&self, port: Port) {
    self.target().store(port.0, Relaxed);
  }

  #[inline(always)]
  pub fn cas_target(&self, expected: Port, value: Port) -> Result<Port, Port> {
    self.target().compare_exchange(expected.0, value.0, Relaxed, Relaxed).map(Port).map_err(Port)
  }

  #[inline(always)]
  pub fn swap_target(&self, value: Port) -> Port {
    let port = Port(self.target().swap(value.0, Relaxed));
    if cfg!(feature = "_fuzz") {
      assert_ne!(port, Port::FREE_1);
    }
    port
  }

  #[inline(always)]
  pub fn as_var(&self) -> Port {
    Port(self.0 | Tag::Var as u64)
  }

  // Takes a pointer's target.
  #[inline(always)]
  pub fn lock_target(&self) -> Port {
    loop {
      let got = self.swap_target(Port::LOCK);
      if cfg!(feature = "_fuzz") {
        assert_ne!(got, Port::FREE_1);
      }
      if got != Port::LOCK {
        return got;
      }
      spin_loop();
    }
  }
}
