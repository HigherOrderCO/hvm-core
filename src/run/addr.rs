use super::*;

/// A memory address to be used in a [`Port`] or a [`Wire`].
///
/// The bottom three bits must be zero; i.e. this address must be at least
/// 8-byte-aligned.
///
/// Additionally, all bits other than the lowest 48 must be zero. On a 32-bit
/// system, this has no effect, but on a 64-bit system, this means that the top
/// 16 bits much be zero.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[must_use]
pub struct Addr(pub usize);

impl IsEnabled for Addr {}

impl fmt::Debug for Addr {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "{:012x?}", self.0)
  }
}

impl Addr {
  pub const NULL: Addr = Addr(0);

  /// Casts this address into an `&AtomicU64`, which may or may not be valid.
  #[inline(always)]
  pub fn val<'a>(&self) -> &'a AtomicU64 {
    unsafe { &*(self.0 as *const _) }
  }

  /// Casts this address into an `&Def`, which may or may not be valid.
  #[inline(always)]
  pub fn def<'a>(&self) -> &'a Def {
    unsafe { &*(self.0 as *const _) }
  }

  /// Rounds this address down to be aligned to `align`.
  #[inline(always)]
  pub(super) fn floor(&self, align: Align) -> Self {
    Addr(self.0 & (usize::MAX << align.tag_bits()))
  }

  /// Returns the other address of alignment `align` within alignment
  /// `align.next()`.
  #[inline(always)]
  pub(super) fn other(&self, align: Align) -> Self {
    Addr(self.0 ^ (1 << align.tag_bits()))
  }

  /// Offsets the address by a specified number of words.
  #[inline(always)]
  pub(super) fn offset(&self, words: usize) -> Self {
    Addr(self.0 + (words << 3))
  }
}
