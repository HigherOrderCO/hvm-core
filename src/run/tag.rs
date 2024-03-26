use super::*;

bi_enum! {
  #[repr(u8)]
  #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
  pub enum Tag {
    //-- Align1 --\\
    Red  =    0b0_00,
    Var  =    0b1_00,

    //-- Align2 --\\
    Ctr2 =   0b01_01,
    Adt2 =   0b11_01,
    Num  =   0b00_01,
    Op   =   0b10_01,

    //-- Align4 --\\
    Ctr3 =  0b001_10,
    Ctr4 =  0b101_10,
    Adt3 =  0b011_10,
    Adt4 =  0b111_10,
    Ref  =  0b000_10,
    Mat  =  0b010_10,
    AdtZ =  0b100_10,
    //   =  0b110_10,

    //-- Align8 --\\
    Ctr5 = 0b0001_11,
    Ctr6 = 0b0101_11,
    Ctr7 = 0b1001_11,
    Ctr8 = 0b1101_11,
    Adt5 = 0b0011_11,
    Adt6 = 0b0111_11,
    Adt7 = 0b1011_11,
    Adt8 = 0b1111_11,
    //   = 0b0000_11,
    //   = 0b0010_11,
    //   = 0b0100_11,
    //   = 0b0110_11,
    //   = 0b1000_11,
    //   = 0b1010_11,
    //   = 0b1100_11,
    //   = 0b1110_11,
  }
}

impl Tag {
  #[inline(always)]
  pub(super) fn align(self) -> Align {
    unsafe { Align::from_unchecked(self as u8 & 0b11) }
  }

  /// Returns the width -- the size of the allocation -- of nodes of this tag.
  #[inline]
  pub(crate) fn width(self) -> u8 {
    match self {
      Tag::Num | Tag::Ref | Tag::AdtZ => 0,
      Tag::Red | Tag::Var => 1,
      Tag::Op => 2,
      Tag::Mat => 3,
      CtrN!() | AdtN!() => (1 << (self.align() as u8 - 1)) + 1 + (self as u8 >> 4),
    }
  }

  /// Returns the arity -- the number of auxiliary ports -- of nodes of this
  /// tag.
  #[inline]
  pub(crate) fn arity(self) -> u8 {
    match self {
      AdtN!() => self.width() - 1,
      _ => self.width(),
    }
  }

  pub(super) fn ctr_with_width(width: u8) -> Tag {
    let sub = width - 1;
    let ilog = unsafe { sub.checked_ilog2().unwrap_unchecked() as u8 };
    let ext = sub - (1 << ilog);
    let tag = (ilog + 1) | 0b100 | (ext << 4);
    unsafe { Tag::from_unchecked(tag) }
  }

  pub(super) fn adt_with_width(width: u8) -> Tag {
    unsafe { Tag::from_unchecked(Tag::ctr_with_width(width) as u8 | 0b1000) }
  }
}

/// Matches any `Ctr` tag.
macro_rules! CtrN {
  () => {
    Tag::Ctr2 | Tag::Ctr3 | Tag::Ctr4 | Tag::Ctr5 | Tag::Ctr6 | Tag::Ctr7 | Tag::Ctr8
  };
}

/// Matches any `Adt` tag except `AdtZ` (which is handled quite differently).
macro_rules! AdtN {
  () => {
    Tag::Adt2 | Tag::Adt3 | Tag::Adt4 | Tag::Adt5 | Tag::Adt6 | Tag::Adt7 | Tag::Adt8
  };
}

pub(crate) use AdtN;
pub(crate) use CtrN;

bi_enum! {
  #[repr(u8)]
  /// The alignment of an [`Addr`], measured in words.
  ///
  /// The numeric representation of the alignment is `log2(align.width())`.
  #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
  pub(crate) enum Align {
    Align1 = 0b00,
    Align2 = 0b01,
    Align4 = 0b10,
    Align8 = 0b11,
  }
}

pub(crate) use Align::*;

impl Align {
  /// The number of bits available for tagging in addresses of this alignment.
  #[inline(always)]
  pub(super) fn tag_bits(self) -> u8 {
    self as u8 + 3
  }

  pub(super) fn addr_mask(self) -> u64 {
    0x0000_FFFF_FFFF_FFFF & (u64::MAX << self.tag_bits())
  }

  #[inline(always)]
  pub(super) fn width(self) -> u8 {
    1 << self as u8
  }

  /// Returns the next largest alignment, if it exists.
  #[inline(always)]
  pub(super) fn next(self) -> Option<Self> {
    match self {
      Align1 => Some(Align2),
      Align2 => Some(Align4),
      Align4 => Some(Align8),
      Align8 => None,
    }
  }

  pub(super) fn of_width(width: u8) -> Self {
    unsafe { Align::from_unchecked((width - 1).checked_ilog2().unwrap_unchecked() as u8 + 1) }
  }
}

#[test]
fn test_tag_width() {
  use Tag::*;
  assert_eq!([Ctr2, Ctr3, Ctr4, Ctr5, Ctr6, Ctr7, Ctr8].map(Tag::width), [2, 3, 4, 5, 6, 7, 8]);
  assert_eq!([Adt2, Adt3, Adt4, Adt5, Adt6, Adt7, Adt8].map(Tag::width), [2, 3, 4, 5, 6, 7, 8]);
  for t in [Ctr2, Ctr3, Ctr4, Ctr5, Ctr6, Ctr7, Ctr8] {
    assert_eq!(Tag::ctr_with_width(t.width()), t);
    assert_eq!(Align::of_width(t.width()), t.align());
  }
}
