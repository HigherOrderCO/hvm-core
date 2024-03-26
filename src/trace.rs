//! An efficient debugging utility to trace the execution of hvm-core.
//!
//! Some bugs in hvm-core occur so rarely (once in hundreds of millions of
//! interactions) that standard debugging tools are ineffective. Traditional
//! `println!` debugging can slow down the process to the point where the bug in
//! question will never be reached.
//!
//! When the `trace` feature is enabled, the `trace!` macro writes a compact
//! binary execution log to an in-memory ring buffer. When a segfault/panic is
//! encountered, the `_read_traces` function can read the data from these
//! buffers out in a human-readable format. The `trace!` macro is sufficiently
//! performant that it can reasonably be called even inside hvmc's hot reduction
//! loop.
//!
//! To use this utility, compile with `--features trace`, run the resulting
//! binary in a debugger, and – when the bug occurs – call [`_read_traces()`] to
//! dump all traces to stderr. You can also call the [`set_hook()`] function to
//! automatically print traces on panic.
//!
//! ```sh
//! $ cargo build --release --features trace; rust-lldb -- ./target/release/hvmc run path/to/file.hvmc -s
//! (lldb) process launch -e path/to/trace/output
//! ...
//! Process ##### stopped
//! * thread ###, name = 't##', stop reason = signal SIGABRT
//! ...
//! (lldb) image lookup -s _read_traces
//! 1 symbols match '_read_traces' in .../hvm-core/target/release/hvmc:
//!   Address: hvmc[0x000000010002fe94] (hvmc.__TEXT.__text + 191024)
//!   Summary: hvmc`_read_traces
//! (lldb) expr ((void(*)(long long))0x000000010002fe94)(-1)
//! (lldb) ^D
//! $ less path/to/trace/output
//! 0ct0b half_atomic_link [src/run.rs:510] #1854603
//!         a_dir: 00000002b02968f1 [Var 0002b02968f0]
//!         b_ptr: 0001600002ac8582 [Ref 600002ac8580]
//! 0ct0b comm02 [src/run.rs:761] #1854603
//!         a: 0001600002ac8582 [Ref 600002ac8580]
//!         b: 00020002b02968f7 [Ctr 2 0002b02968f0]
//! 0ct0b interact [src/run.rs:655] #1854603
//!         a: 0001600002ac8582 [Ref 600002ac8580]
//!         b: 00020002b02968f7 [Ctr 2 0002b02968f0]
//! 0ct0b linker [src/run.rs:525] #1854602
//!         a_ptr: 0001600002ac8142 [Ref 600002ac8140]
//!         b_ptr: 0000000330795f71 [Var 000330795f70]
//! ...
//! ```
//!
//! The initial five characters identifies the thread an entry corresponds to
//! (the first two hex chars are the index of the thread, and the last two are
//! the net's `tid`).
//!
//! Entries are reverse-chronological (the top is the most recent). Note that
//! chronology between threads is only synced periodically (currently, at each
//! interaction). However, updates propagate between threads unpredictably, so
//! the timeline is only guaranteed to be consistent between entries for the
//! same thread.
//!
//! For certain bugs, it may be useful to modify `main.rs` to repeatedly run the
//! program (until an error is encountered). In this case, one can run
//! [`_reset_traces()`] before each iteration, to discard the traces of the
//! previous iteration.

#![cfg_attr(not(feature = "trace"), allow(unused))]

use std::{
  cell::{Cell, UnsafeCell},
  fmt::{self, Debug, Formatter, Write},
  sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Mutex, Once,
  },
};

use crate::{
  ops::Op,
  run::{Addr, Align, Port, Trg, Wire},
};

#[cfg(not(feature = "trace"))]
#[derive(Default)]
pub struct Tracer(());

#[cfg(not(feature = "trace"))]
impl Tracer {
  #[inline(always)]
  pub fn sync(&self) {}
  #[inline(always)]
  #[doc(hidden)]
  pub fn trace<S: TraceSourceBearer, A: TraceArgs>(&self, _: A) {}
  #[inline(always)]
  pub fn set_tid(&self, _: usize) {}
}

#[macro_export]
macro_rules! trace {
  ($tracer:expr, $str:literal $(, $x:expr)* $(,)?) => {{
    struct __;
    impl $crate::trace::TraceSourceBearer for __ {
      const SOURCE: $crate::trace::TraceSource = $crate::trace::TraceSource {
        func: {
          #[cfg(feature = "trace")] { std::any::type_name::<__>() }
          #[cfg(not(feature = "trace"))] { "" }
        },
        file: file!(),
        line: line!(),
        str: $str,
        args: &[$(stringify!($x)),*],
      };
    }
    if cfg!(feature = "trace") {
      $tracer.trace::<__, _>(($(&$x,)*));
    }
  }};
  ($tracer:expr $(, $x:expr)* $(,)?) => {
    trace!($tracer, "", $($x),*)
  };
}

#[cfg(feature = "trace")]
#[derive(Default)]
pub struct Tracer(TraceWriter);

#[cfg(feature = "trace")]
impl Tracer {
  #[inline(always)]
  pub fn sync(&self) {
    self.0.sync()
  }
  #[inline(always)]
  #[doc(hidden)]
  pub fn trace<S: TraceSourceBearer, A: TraceArgs>(&self, args: A) {
    self.0.trace::<S, A>(args)
  }
  #[inline(always)]
  pub fn set_tid(&self, tid: usize) {
    self.0.set_tid(tid)
  }
}

pub trait TraceSourceBearer {
  const SOURCE: TraceSource;
}

#[doc(hidden)]
pub struct TraceSource {
  pub func: &'static str,
  pub file: &'static str,
  pub line: u32,
  pub str: &'static str,
  pub args: &'static [&'static str],
}

#[doc(hidden)]
pub struct TraceMetadata {
  pub source: TraceSource,
  pub arg_fmts: &'static [fn(u64, &mut fmt::Formatter) -> fmt::Result],
}

const TRACE_SIZE: usize = 1 << 22;

static TRACE_NONCE: AtomicU64 = AtomicU64::new(1);

struct TraceLock {
  locked: AtomicBool,
  data: UnsafeCell<TraceData>,
}

struct TraceData {
  tid: usize,
  cursor: usize,
  data: Box<[u64; TRACE_SIZE]>,
}

impl TraceData {
  fn write_word(&mut self, word: u64) {
    self.data[self.cursor] = word;
    self.cursor = (self.cursor + 1) % TRACE_SIZE;
  }
}

static ACTIVE_TRACERS: Mutex<Vec<Box<TraceLock>>> = Mutex::new(Vec::new());

struct TraceWriter {
  lock: &'static TraceLock,
  nonce: Cell<u64>,
}

unsafe impl Send for TraceWriter {}

impl Default for TraceWriter {
  fn default() -> Self {
    let boxed = Box::new(TraceLock {
      locked: AtomicBool::new(false),
      data: UnsafeCell::new(TraceData { tid: 0, cursor: 0, data: Box::new([0; TRACE_SIZE]) }),
    });
    let lock = unsafe { &*(&*boxed as *const _) };
    let mut active_tracers = ACTIVE_TRACERS.lock().unwrap();
    active_tracers.push(boxed);
    TraceWriter { lock, nonce: Cell::new(TRACE_NONCE.fetch_add(1, Ordering::Relaxed)) }
  }
}

impl TraceWriter {
  fn sync(&self) {
    self.nonce.set(TRACE_NONCE.fetch_add(1, Ordering::Relaxed));
  }
  fn acquire(&self, cb: impl FnOnce(&mut TraceData)) {
    while self.lock.locked.compare_exchange_weak(false, true, Ordering::Relaxed, Ordering::Relaxed).is_err() {
      std::hint::spin_loop();
    }
    cb(unsafe { &mut *self.lock.data.get() });
    self.lock.locked.store(false, Ordering::Release);
  }
  fn trace<S: TraceSourceBearer, A: TraceArgs>(&self, args: A) {
    if cfg!(feature = "_fuzz") {
      self.sync();
    }
    let meta: &'static _ = &TraceMetadata { source: S::SOURCE, arg_fmts: A::FMTS };
    self.acquire(|data| {
      let nonce = self.nonce.get();
      for arg in args.to_words().rev() {
        data.write_word(arg);
      }
      data.write_word(meta as *const _ as u64);
      data.write_word(nonce);
    })
  }
  fn set_tid(&self, tid: usize) {
    self.acquire(|data| data.tid = tid);
  }
}

struct TraceReader<'a> {
  data: &'a TraceData,
  cursor: usize,
  id: usize,
}

impl<'a> TraceReader<'a> {
  fn new(data: &'a TraceData, id: usize) -> Self {
    TraceReader { data, cursor: (TRACE_SIZE + data.cursor - 1) % TRACE_SIZE, id }
  }
  fn read_entry(&mut self, f: &mut impl Write) -> Option<fmt::Result> {
    let nonce = self.read_word()?;
    let meta = self.read_word()?;
    if meta == 0 {
      self.cursor = self.data.cursor;
      return None;
    }
    let meta = unsafe { &*(meta as *const TraceMetadata) };
    if self.remaining() < meta.source.args.len() {
      self.cursor = self.data.cursor;
      return None;
    }
    Some((|| {
      writeln!(
        f,
        "{:02x}t{:02x} {}{}{} [{}:{}] #{}",
        self.id,
        self.data.tid,
        meta.source.func.trim_end_matches("::__").trim_end_matches("::{{closure}}").rsplit("::").next().unwrap(),
        if meta.source.str.is_empty() { "" } else { " " },
        meta.source.str,
        meta.source.file,
        meta.source.line,
        nonce,
      )?;
      let max_len = meta.source.args.iter().map(|x| x.len()).max().unwrap_or(0);
      for (&arg, &fmt) in meta.source.args.iter().zip(meta.arg_fmts) {
        let word = self.read_word().unwrap();
        for _ in 0 .. (8 + max_len - arg.len()) {
          f.write_char(' ')?;
        }
        writeln!(f, "{}: {:?}", arg, FmtWord(fmt, word))?;
      }
      Ok(())
    })())
  }
  fn peek_word(&self) -> Option<u64> {
    if self.cursor == self.data.cursor { None } else { Some(self.data.data[self.cursor]) }
  }
  fn read_word(&mut self) -> Option<u64> {
    let word = self.peek_word()?;
    self.cursor = (self.cursor + TRACE_SIZE - 1) % TRACE_SIZE;
    Some(word)
  }
  fn remaining(&self) -> usize {
    (self.cursor + TRACE_SIZE - self.data.cursor) % TRACE_SIZE
  }
}

#[cfg_attr(feature = "trace", no_mangle)]
pub fn _read_traces(limit: usize) {
  let active_tracers = &*ACTIVE_TRACERS.lock().unwrap();
  let mut readers = active_tracers
    .iter()
    .enumerate()
    .map(|(i, t)| {
      while t.locked.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
        std::hint::spin_loop();
      }
      TraceReader::new(unsafe { &*t.data.get() }, i)
    })
    .collect::<Vec<_>>();
  let mut out = String::new();
  for _ in 0 .. limit {
    let Some((1 .., r)) = readers.iter_mut().filter_map(|x| Some((x.peek_word()?, x))).max_by_key(|x| x.0) else {
      break;
    };
    r.read_entry(&mut out);
  }
  eprintln!("{}", out);
}

pub unsafe fn _reset_traces() {
  ACTIVE_TRACERS.lock().unwrap().clear();
  TRACE_NONCE.store(1, Ordering::Relaxed);
}

pub trait TraceArg {
  fn to_word(&self) -> u64;
  fn from_word(word: u64) -> impl Debug;
}

impl<'a, T: TraceArg> TraceArg for &'a T {
  fn to_word(&self) -> u64 {
    (*self).to_word()
  }

  fn from_word(word: u64) -> impl Debug {
    T::from_word(word)
  }
}

impl TraceArg for Port {
  fn to_word(&self) -> u64 {
    self.0
  }
  fn from_word(word: u64) -> impl Debug {
    Port(word)
  }
}

impl TraceArg for Wire {
  fn to_word(&self) -> u64 {
    self.0 as u64
  }
  fn from_word(word: u64) -> impl Debug {
    Wire(word as _)
  }
}

impl TraceArg for Trg {
  fn to_word(&self) -> u64 {
    self.0.0 as u64
  }
  fn from_word(word: u64) -> impl Debug {
    Trg(Port(word))
  }
}

impl TraceArg for Op {
  fn to_word(&self) -> u64 {
    *self as u64
  }
  fn from_word(word: u64) -> impl Debug {
    unsafe { Op::try_from(word as u16).unwrap_unchecked() }
  }
}

impl TraceArg for Align {
  fn to_word(&self) -> u64 {
    *self as u64
  }
  fn from_word(word: u64) -> impl Debug {
    unsafe { Align::try_from(word as u8).unwrap_unchecked() }
  }
}

impl TraceArg for Addr {
  fn to_word(&self) -> u64 {
    self.0 as u64
  }
  fn from_word(word: u64) -> impl Debug {
    Addr(word as _)
  }
}

macro_rules! impl_trace_num {
  ($num:ty) => {
    impl TraceArg for $num {
      fn to_word(&self) -> u64 {
        *self as _
      }
      fn from_word(word: u64) -> impl Debug {
        word as Self
      }
    }
  };
}

impl_trace_num!(u8);
impl_trace_num!(u16);
impl_trace_num!(u32);
impl_trace_num!(u64);
impl_trace_num!(usize);

impl TraceArg for bool {
  fn to_word(&self) -> u64 {
    *self as _
  }
  fn from_word(word: u64) -> impl Debug {
    word != 0
  }
}

impl<T> TraceArg for *const T {
  fn to_word(&self) -> u64 {
    *self as _
  }
  fn from_word(word: u64) -> impl Debug {
    word as Self
  }
}

#[doc(hidden)]
pub trait TraceArgs {
  const FMTS: &'static [fn(u64, &mut fmt::Formatter) -> fmt::Result];
  fn to_words(self) -> impl DoubleEndedIterator<Item = u64>;
}

impl TraceArgs for () {
  const FMTS: &'static [fn(u64, &mut fmt::Formatter) -> fmt::Result] = &[];
  fn to_words(self) -> impl DoubleEndedIterator<Item = u64> {
    [].into_iter()
  }
}

macro_rules! impl_trace_args_tuple {
  ($($T:ident)*) => {
    impl<'a, $($T: TraceArg,)*> TraceArgs for ($(&'a $T,)*) {
      const FMTS: &'static [fn(u64, &mut fmt::Formatter) -> fmt::Result] = &[$(fmt::<$T>,)*];
      fn to_words(self) -> impl DoubleEndedIterator<Item = u64> {
        let ($($T,)*) = self;
        [$($T.to_word(),)*].into_iter()
      }
    }
  };
}

impl_trace_args_tuple!(A);
impl_trace_args_tuple!(A B);
impl_trace_args_tuple!(A B C);
impl_trace_args_tuple!(A B C D);
impl_trace_args_tuple!(A B C D E);
impl_trace_args_tuple!(A B C D E F);

fn fmt<T: TraceArg>(word: u64, f: &mut fmt::Formatter) -> fmt::Result {
  T::from_word(word).fmt(f)
}

struct FmtWord(fn(word: u64, f: &mut fmt::Formatter) -> fmt::Result, u64);

impl fmt::Debug for FmtWord {
  fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
    self.0(self.1, f)
  }
}

pub fn set_hook() {
  static ONCE: Once = Once::new();
  if cfg!(feature = "trace") {
    ONCE.call_once(|| {
      let hook = std::panic::take_hook();
      std::panic::set_hook(Box::new(move |info| {
        hook(info);
        _read_traces(usize::MAX);
      }));
    })
  }
}
