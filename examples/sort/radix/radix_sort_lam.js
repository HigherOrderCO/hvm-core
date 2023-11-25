const Empty = empty => single => concat => empty;
const Single = x => empty => single => concat => single(x);
const Concat = x0 => x1 => empty => single => concat => concat(x0)(x1);

const Free = free => used => node => free;
const Used = free => used => node => used;
const Node = x0 => x1 => free => used => node => node(x0)(x1);

const gen = n => x => {
  if (n === 0) {
    return Single(x);
  } else {
    const p  = n - 1;
    const x0 = x << 1;
    const x1 = x0 | 1;
    return Concat(gen(p)(x0))(gen(p)(x1));
  }
};

const rev = a => {
  const a_empty  = Empty;
  const a_single = x => Single(x);
  const a_concat = x0 => x1 => Concat(rev(x1))(rev(x0));
  return a(a_empty)(a_single)(a_concat);
};

const sum = a => {
  const a_empty  = 0;
  const a_single = x => x;
  const a_concat = x0 => x1 => sum(x0) + sum(x1);
  return a(a_empty)(a_single)(a_concat);
};

const sort = t => to_arr(to_map(t))(0);

const to_arr = a => k => {
  const a_free = Empty;
  const a_used = Single(k);
  const a_node = x0 => x1 => {
    const x0Arr = to_arr(x0)(k * 2);
    const x1Arr = to_arr(x1)(k * 2 + 1);
    return Concat(x0Arr)(x1Arr);
  };
  return a(a_free)(a_used)(a_node);
};

const to_map = a => {
  const a_empty  = Free;
  const a_single = x => radix(x);
  const a_concat = x0 => x1 => merge(to_map(x0))(to_map(x1));
  return a(a_empty)(a_single)(a_concat);
};

const merge = a => {
  const a_free = b => {
    const b_free = Free;
    const b_used = Used;
    const b_node = b0 => b1 => Node(b0)(b1);
    return b(b_free)(b_used)(b_node);
  };
  const a_used = b => {
    const b_free = Used;
    const b_used = Used;
    const b_node = b0 => b1 => 0;
    return b(b_free)(b_used)(b_node);
  };
  const a_node = a0 => a1 => b => {
    const b_free = a0 => a1 => Node(a0)(a1);
    const b_used = a0 => a1 => 0;
    const b_node = b0 => b1 => a0 => a1 => Node(merge(a0)(b0))(merge(a1)(b1));
    return b(b_free)(b_used)(b_node)(a0)(a1);
  };
  return a(a_free)(a_used)(a_node);
};

const radix = n => {
  let r = Used;
  r = swap(n & 1)(r)(Free);
  r = swap(n & 2)(r)(Free);
  r = swap(n & 4)(r)(Free);
  r = swap(n & 8)(r)(Free);
  r = swap(n & 16)(r)(Free);
  r = swap(n & 32)(r)(Free);
  r = swap(n & 64)(r)(Free);
  r = swap(n & 128)(r)(Free);
  r = swap(n & 256)(r)(Free);
  r = swap(n & 512)(r)(Free);
  r = swap(n & 1024)(r)(Free);
  r = swap(n & 2048)(r)(Free);
  r = swap(n & 4096)(r)(Free);
  r = swap(n & 8192)(r)(Free);
  r = swap(n & 16384)(r)(Free);
  r = swap(n & 32768)(r)(Free);
  r = swap(n & 65536)(r)(Free);
  r = swap(n & 131072)(r)(Free);
  r = swap(n & 262144)(r)(Free);
  r = swap(n & 524288)(r)(Free);
  r = swap(n & 1048576)(r)(Free);
  r = swap(n & 2097152)(r)(Free);
  r = swap(n & 4194304)(r)(Free);
  r = swap(n & 8388608)(r)(Free);
  return r;
};

const swap = n => x0 => x1 => {
  if (n === 0) {
    return Node(x0)(x1);
  } else {
    return Node(x1)(x0);
  }
};

console.log(sum(sort(rev(gen(22)(0)))));
