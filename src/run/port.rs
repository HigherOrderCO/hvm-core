use super::*;

/// A port in the interaction net.
///
/// The type of a port is determined by its *tag*, which is stored in the bottom
/// three bits.
///
/// TODO: update
///
/// All tags other than [`Num`] divide the bits of the port as follows:
/// - the top 16 bits are the *label*, accessible with [`Port::lab`]
/// - the middle 45 bits are the non-alignment bits of the *address*, an
///   8-byte-aligned pointer accessible with [`Port::addr`]
/// - the bottom 3 bits are the tag, as always
///
/// The semantics of these fields depend upon the tag; see the documentation for
/// each [`Tag`] variant.
#[derive(Clone, Eq, PartialEq, PartialOrd, Hash, Default)]
#[repr(transparent)]
#[must_use]
pub struct Port(pub u64);

impl fmt::Debug for Port {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "{:016x?} ", self.0)?;
    match *self {
      Port::ERA => write!(f, "[ERA]"),
      Port::FREE_1 => write!(f, "[FREE 1]"),
      Port::FREE_2 => write!(f, "[FREE 2]"),
      Port::FREE_4 => write!(f, "[FREE 4]"),
      Port::FREE_8 => write!(f, "[FREE 8]"),
      Port::GONE => write!(f, "[GONE]"),
      Port::LOCK => write!(f, "[LOCK]"),
      _ => match self.tag() {
        Tag::Num => write!(f, "[Num {}]", self.num()),
        Tag::Var | Tag::Red | Tag::Mat => write!(f, "[{:?} {:?}]", self.tag(), self.addr()),
        Tag::Op | CtrN!() | AdtN!() | Tag::AdtZ | Tag::Ref => {
          write!(f, "[{:?} {:?} {:?}]", self.tag(), self.lab(), self.addr())
        }
      },
    }
  }
}

impl Port {
  /// The principal port of an eraser node.
  pub const ERA: Port = Port(Tag::Ref as _);
  /// A sentinel value used to lock a wire; see the linking algorithm for more
  /// details.
  pub const LOCK: Port = Port(0xFFFF_FFFF_FFFF_FFF0);
  /// A sentinel value used in the atomic linking algorithm; see it for more
  /// details.
  pub const GONE: Port = Port(0xFFFF_FFFF_FFFF_FFFF);

  /// Creates a new port with a given tag, label, and addr.
  #[inline(always)]
  pub fn new(tag: Tag, lab: Lab, addr: Addr) -> Self {
    Port(((lab as u64) << 48) | (addr.0 as u64) | (tag as u64))
  }

  /// Creates a new [`Var`] port with a given addr.
  #[inline(always)]
  pub fn new_var(alloc_align: Align, addr: Addr) -> Self {
    Port::new(Tag::Var, alloc_align as u16, addr)
  }

  /// Creates a new [`Red`] port with a given addr.
  #[inline(always)]
  pub fn new_red(alloc_align: Align, addr: Addr) -> Self {
    Port::new(Tag::Red, alloc_align as u16, addr)
  }

  /// Creates a new [`Num`] port with a given 60-bit numeric value.
  #[inline(always)]
  pub const fn new_num(val: u64) -> Self {
    Port((val << 4) | (Tag::Num as u64))
  }

  /// Creates a new [`Ref`] port corresponding to a given definition.
  #[inline(always)]
  pub fn new_ref(def: &Def) -> Port {
    Port::new(Tag::Ref, def.labs.min_safe, Addr(def as *const _ as _))
  }

  /// TODO
  #[inline(always)]
  pub const fn new_adtz(variant_index: u8, variant_count: u8) -> Self {
    Port(Tag::AdtZ as u64 | ((variant_index as u64) << 16) | ((variant_count as u64) << 8))
  }

  /// TODO
  #[inline(always)]
  pub fn variant_index(&self) -> u8 {
    (self.0 >> 16) as u8
  }

  /// TODO
  #[inline(always)]
  pub fn variant_count(&self) -> u8 {
    (self.0 >> 8) as u8
  }

  /// TODO
  #[inline(always)]
  pub fn align(&self) -> Align {
    unsafe { Align::from_unchecked((self.0 & 0b11) as u8) }
  }

  /// Accesses the tag of this port; this is valid for all ports.
  #[inline(always)]
  pub fn tag(&self) -> Tag {
    unsafe { Tag::from_unchecked((self.0 & ((1 << self.align().tag_bits()) - 1)) as u8) }
  }

  /// Checks if this port is of the given `tag`.
  #[inline(always)]
  pub fn is(&self, tag: Tag) -> bool {
    // This could be `self.tag() == tag`, but this is more efficient when `tag`
    // is a constant.
    (self.0 & ((1 << tag.align().tag_bits()) - 1)) as u8 == tag as u8
  }

  /// Accesses the label of this port; this is valid for all non-`Num` ports.
  #[inline(always)]
  pub const fn lab(&self) -> Lab {
    (self.0 >> 48) as Lab
  }

  /// Accesses the addr of this port; this is valid for all non-`Num` ports.
  #[inline(always)]
  pub fn addr(&self) -> Addr {
    Addr((self.0 & self.align().addr_mask()) as usize)
  }

  /// Accesses the operation of this port; this is valid for [`Opr`] ports.
  #[inline(always)]
  pub fn op(&self) -> Op {
    unsafe { Op::from_unchecked(self.lab()) }
  }

  /// Accesses the numeric value of this port; this is valid for [`Num`] ports.
  #[inline(always)]
  pub const fn num(&self) -> u64 {
    self.0 >> 4
  }

  /// Accesses the wire leaving this port; this is valid for [`Var`] ports and
  /// non-sentinel [`Red`] ports.
  #[inline(always)]
  pub fn wire(&self) -> Wire {
    Wire::new(self.alloc_align(), self.addr())
  }

  #[inline(always)]
  pub fn is_principal(&self) -> bool {
    self.align() != Align1
  }

  /// Given a principal port, returns whether this principal port may be part of
  /// a skippable active pair -- an active pair like `ERA-ERA` that does not
  /// need to be added to the redex list.
  #[inline(always)]
  pub fn is_skippable(&self) -> bool {
    self.is(Tag::AdtZ) || self.is(Tag::Num) || self.is(Tag::Ref) && self.lab() != u16::MAX
  }

  /// Converts a [`Var`] port into a [`Red`] port with the same address.
  #[inline(always)]
  pub(super) fn redirect(&self) -> Port {
    Port(self.0 ^ (Tag::Red as u64 ^ Tag::Var as u64))
  }

  /// Converts a [`Red`] port into a [`Var`] port with the same address.
  #[inline(always)]
  pub(super) fn unredirect(&self) -> Port {
    self.redirect()
  }

  pub(super) fn is_full_node(&self) -> bool {
    match self.tag() {
      Tag::Op | Tag::Mat | CtrN!() | AdtN!() => true,
      Tag::Red | Tag::Var | Tag::Num | Tag::Ref | Tag::AdtZ => false,
    }
  }

  /// TODO
  #[inline(always)]
  pub(super) fn alloc_align(&self) -> Align {
    unsafe { Align::from_unchecked(self.lab() as u8) }
  }

  /// TODO
  #[inline(always)]
  pub(super) fn is_ctr_ish(&self) -> bool {
    (self.0 & 0b111) > 0b100
  }

  #[inline(always)]
  pub(crate) fn aux_port(&self, i: u8) -> Port {
    Port::new_var(self.align(), self.addr().offset(i as usize))
  }

  #[inline(always)]
  pub(super) fn with_addr(&self, addr: Addr) -> Port {
    Port(self.0 & !self.align().addr_mask() | addr.0 as u64)
  }
}
