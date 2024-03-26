use super::*;

/// The memory buffer backing a [`Net`].
#[repr(align(64))]
pub struct Heap(pub(super) [AtomicU64]);

impl Heap {
  /// Allocates a new heap with the given size in bytes, defaulting to the
  /// largest power-of-two allocation the system will allow.
  pub fn new(bytes: Option<usize>) -> Option<Box<Self>> {
    if let Some(bytes) = bytes {
      return Self::new_exact(bytes / 8);
    }
    let mut size = if cfg!(target_pointer_width = "64") {
      1 << 40 // 1 TiB
    } else {
      1 << 30 // 1 GiB
    } / 8;
    while size != 0 {
      if let Some(heap) = Self::new_exact(size) {
        return Some(heap);
      }
      size /= 2;
    }
    None
  }
  /// Allocates a new heap with exactly the given size in words.
  #[inline]
  pub fn new_exact(words: usize) -> Option<Box<Self>> {
    if words == 0 {
      return None;
    }
    unsafe {
      let ptr = alloc::alloc(Layout::array::<AtomicU64>(words).unwrap().align_to(64).unwrap()) as *mut AtomicU64;
      if ptr.is_null() {
        return None;
      }
      Some(Box::from_raw(core::ptr::slice_from_raw_parts_mut(ptr, words) as *mut _))
    }
  }
}

/// Manages allocating and freeing nodes within the net.
pub struct Allocator<'h> {
  pub(super) tracer: Tracer,
  pub(super) heap: &'h Heap,
  pub(super) next: usize,
  pub(super) heads: [Addr; 4],
}

deref!({<'h>} Allocator<'h> => self.tracer: Tracer);

impl Align {
  #[inline(always)]
  const fn free(self) -> u64 {
    (1 << self as u64) << 60
  }
}

/// Sentinel values used to indicate free memory.
impl Port {
  pub(super) const FREE_1: Port = Port(Align1.free());
  pub(super) const FREE_2: Port = Port(Align2.free());
  pub(super) const FREE_4: Port = Port(Align4.free());
  pub(super) const FREE_8: Port = Port(Align8.free());
}

impl<'h> Allocator<'h> {
  pub fn new(heap: &'h Heap) -> Self {
    Allocator { tracer: Tracer::default(), heap, next: 0, heads: [Addr::NULL; 4] }
  }

  fn head(&mut self, align: Align) -> &mut Addr {
    unsafe { self.heads.get_unchecked_mut(align as usize) }
  }

  fn push_addr(head: &mut Addr, addr: Addr) {
    addr.val().store(head.0 as u64, Relaxed);
    *head = addr;
  }

  /// Frees one word of an allocation of size `alloc_align`.
  #[inline(always)]
  pub fn free_word(&mut self, mut addr: Addr, alloc_align: Align) {
    if cfg!(feature = "_fuzz") {
      if cfg!(not(feature = "_fuzz_no_free")) {
        let free = Port::FREE_1.0;
        assert_ne!(addr.val().swap(free, Relaxed), free, "double free");
      }
      return;
    }
    let mut align = Align1;
    if align == alloc_align {
      trace!(self.tracer, "free");
      return Self::push_addr(self.head(align), addr);
    }
    addr.val().store(align.free(), Relaxed);
    while align != alloc_align {
      if addr.other(align).val().load(Relaxed) != align.free() {
        return;
      }
      trace!(self.tracer, "other free");
      let next_align = unsafe { align.next().unwrap_unchecked() };
      addr = addr.floor(next_align);
      let next_value = if next_align == alloc_align { self.head(alloc_align).0 as u64 } else { next_align.free() };
      if addr.val().compare_exchange(align.free(), next_value, Relaxed, Relaxed).is_err() {
        return trace!(self.tracer, "too slow");
      }
      trace!(self.tracer, "success");
      if next_align == alloc_align {
        let old_head = next_value;
        let new_head = addr;
        trace!(self.tracer, "appended", old_head, new_head);
        return *self.head(align) = addr;
      }
      align = next_align;
    }
  }

  /// Allocates a node, with a size specified by `align`.
  #[inline(never)]
  pub fn alloc(&mut self, align: Align) -> Addr {
    let head = self.head(align);
    let addr = if *head != Addr::NULL {
      let addr = *head;
      let next = Addr(head.val().load(Relaxed) as usize);
      *head = next;
      addr
    } else {
      let w = align.width() as usize;
      let x = w - 1;
      let index = (self.next + x) & !x;
      self.next = index + w;
      trace!(self, index);
      Addr(self.heap.0.get(index).expect("OOM") as *const AtomicU64 as usize)
    };
    trace!(self, align, addr);
    for i in 0 .. align.width() {
      addr.offset(i as usize).val().store(Port::LOCK.0, Relaxed);
    }
    addr
  }

  #[inline(always)]
  pub(crate) fn free_wire(&mut self, wire: Wire) {
    self.free_word(wire.addr(), wire.alloc_align());
  }

  /// If `trg` is a wire, frees the backing memory.
  #[inline(always)]
  pub(crate) fn free_trg(&mut self, trg: Trg) {
    if trg.is_wire() {
      self.free_wire(trg.as_wire());
    }
  }
}
