#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

typedef uint8_t u8;
typedef uint16_t u16;
typedef uint32_t u32;
typedef unsigned long long int u64;

// Configuration
// -------------

// This code is initially optimized for RTX 4090
const u32 BLOCK_LOG2    = 9;                         // log2 of block size
const u32 BLOCK_SIZE    = 1 << BLOCK_LOG2;           // threads per block
const u32 SQUAD_LOG2    = 2;                         // log2 of squad size
const u32 SQUAD_SIZE    = 1 << SQUAD_LOG2;           // threads per squad
const u32 GROUP_LOG2    = BLOCK_LOG2 - SQUAD_LOG2;   // log2 of group size
const u32 GROUP_SIZE    = 1 << GROUP_LOG2;           // squad per group
const u32 NODE_LOG2     = 28;                        // log2 of node size
const u32 NODE_SIZE     = 1 << NODE_LOG2;            // max total nodes (2GB addressable)
const u32 HEAD_LOG2     = GROUP_LOG2 * 2;            // log2 of head size
const u32 HEAD_SIZE     = 1 << HEAD_LOG2;            // max head pointers
const u32 MAX_THREADS   = BLOCK_SIZE * BLOCK_SIZE;   // total number of active threads
const u32 MAX_SQUADS    = GROUP_SIZE * GROUP_SIZE;   // total number of active squads
const u32 MAX_NEW_REDEX = 16;                        // max new redexes per rewrite
const u32 SMEM_SIZE     = 4;                         // u32's shared by squad
const u32 RBAG_SIZE     = 256;                       // max redexes per squad
const u32 LHDS_SIZE     = 32;                        // max local heads
const u32 BAGS_SIZE     = MAX_SQUADS * RBAG_SIZE;    // redexes per GPU

// Types
// -----

typedef u8  Tag; // pointer tag: 4-bit
typedef u32 Val; // pointer val: 28-bit

// Core terms
const Tag NIL = 0x0; // empty node
const Tag REF = 0x1; // reference to a definition (closed net)
const Tag ERA = 0x2; // unboxed eraser
const Tag VRR = 0x3; // variable pointing to root
const Tag VR1 = 0x4; // variable pointing to aux1 port of node
const Tag VR2 = 0x5; // variable pointing to aux2 port of node
const Tag RDR = 0x6; // redirection to root
const Tag RD1 = 0x7; // redirection to aux1 port of node
const Tag RD2 = 0x8; // redirection to aux2 port of node
const Tag NUM = 0x9; // unboxed number
const Tag CON = 0xA; // points to main port of con node
const Tag DUP = 0xB; // points to main port of dup node
const Tag TRI = 0xC; // points to main port of tri node
const Tag CTR = 0xF; // last constructor

// Special values
const u32 NEO = 0xFFFFFFFD; // recently allocated value
const u32 TMP = 0xFFFFFFFE; // node has been moved to redex bag
const u32 TKN = 0xFFFFFFFF; // value taken by another thread, will be replaced soon

// Unit types
const u32 A1 = 0; // focuses on the A node, P1 port
const u32 A2 = 1; // focuses on the A node, P2 port
const u32 B1 = 2; // focuses on the B node, P1 port
const u32 B2 = 3; // focuses on the B node, P2 port

// Ports (P1 or P2)
typedef u8 Port;
const u32 P1 = 0;
const u32 P2 = 1;

// Pointers = 4-bit tag + 28-bit val
typedef u32 Ptr;

// Nodes are pairs of pointers
typedef struct {
  Ptr ports[2];
} Node;

// Wires are pairs of pointers
typedef u64 Wire;

// An interaction net 
typedef struct {
  Ptr   root; // root wire
  Wire* bags; // redex bags (active pairs)
  Node* node; // memory buffer with all nodes
  Wire* head; // head expansion buffer
  u32   done; // number of completed threads
  u64   rwts; // number of rewrites performed
} Net;

// A unit local data
typedef struct {
  u32   tid;  // thread id (local)
  u32   uid;  // squad id (global)
  u32   qid;  // squad id (local: A1|A2|B1|B2)
  u32   port; // unit port (P1|P2)
  u32   aloc; // where to alloc next node
  u64   rwts; // local rewrites performed
  u32*  sm32; // shared 32-bit buffer
  u64*  sm64; // shared 64-bit buffer
  u64*  rlen; // local redex bag length
  Wire* rbag; // local redex bag
} Unit;


// TermBook
// --------

__constant__ u32* BOOK;

typedef u32 Book; // stored in a flat buffer

Book* init_book_on_gpu(u32* data, u32 size) {
  u32* gpu_book;
  cudaMalloc(&gpu_book, size * sizeof(u32));
  cudaMemcpyAsync(gpu_book, data, size * sizeof(u32), cudaMemcpyHostToDevice);
  cudaMemcpyToSymbol(BOOK, &gpu_book, sizeof(u32*));
  return gpu_book;
}

void book_free_on_gpu(Book* gpu_book) {
  cudaFree(gpu_book);
}

// Runtime
// -------

// Integer ceil division
__host__ __device__ u32 div(u32 a, u32 b) {
  return (a + b - 1) / b;
}

// Pseudorandom Number Generator
__host__ __device__ u32 rng(u32 a) {
  return a * 214013 + 2531011;
}

// Creates a new pointer
__host__ __device__ inline Ptr mkptr(Tag tag, Val val) {
  return ((u32)tag << 28) | (val & 0xFFFFFFF);
}

// Gets the tag of a pointer
__host__ __device__ inline Tag tag(Ptr ptr) {
  return (Tag)(ptr >> 28);
}

// Gets the value of a pointer
__host__ __device__ inline Val val(Ptr ptr) {
  return ptr & 0xFFFFFFF;
}

// Is this pointer a variable?
__host__ __device__ inline bool is_var(Ptr ptr) {
  return tag(ptr) >= VRR && tag(ptr) <= VR2;
}

// Is this pointer a redirection?
__host__ __device__ inline bool is_red(Ptr ptr) {
  return tag(ptr) >= RDR && tag(ptr) <= RD2;
}

// Is this pointer a constructor?
__host__ __device__ inline bool is_ctr(Ptr ptr) {
  return tag(ptr) >= CON && tag(ptr) < CTR;
}

// Is this pointer an eraser?
__host__ __device__ inline bool is_era(Ptr ptr) {
  return tag(ptr) == ERA;
}

// Is this pointer a number?
__host__ __device__ inline bool is_num(Ptr ptr) {
  return tag(ptr) == NUM;
}

// Is this pointer a reference?
__host__ __device__ inline bool is_ref(Ptr ptr) {
  return tag(ptr) == REF;
}

// Is this pointer a main port?
__host__ __device__ inline bool is_pri(Ptr ptr) {
  return is_era(ptr)
      || is_ctr(ptr)
      || is_num(ptr)
      || is_ref(ptr);
}

// Is this pointer carrying a location (that needs adjustment)?
__host__ __device__ inline bool has_loc(Ptr ptr) {
  return is_ctr(ptr)
      || is_var(ptr) && tag(ptr) != VRR
      || is_red(ptr) && tag(ptr) != RDR;
}

// Gets the target ref of a var or redirection pointer
__host__ __device__ inline Ptr* target(Net* net, Ptr ptr) {
  if (tag(ptr) == VRR || tag(ptr) == RDR) {
    return &net->root;
  } else if (tag(ptr) == VR1 || tag(ptr) == RD1) {
    return &net->node[val(ptr)].ports[P1];
  } else if (tag(ptr) == VR2 || tag(ptr) == RD2) {
    return &net->node[val(ptr)].ports[P2];
  } else {
    return NULL;
  }
}

// Traverses to the other side of a wire
__host__ __device__ Ptr enter(Net* net, Ptr ptr) {
  Ptr* ref = target(net, ptr);
  while (tag(*ref) >= RDR && tag(*ref) <= RD2) {
    ptr = *ref;
    ref = target(net, ptr);
  }
  return ptr;
}

// Transforms a variable into a redirection
__host__ __device__ inline Ptr redir(Ptr ptr) {
  return mkptr(tag(ptr) + (is_var(ptr) ? 3 : 0), val(ptr));
}

// Transforms a redirection into a variable
__host__ __device__ inline Ptr undir(Ptr ptr) {
  return mkptr(tag(ptr) - (is_red(ptr) ? 3 : 0), val(ptr));
}

// Creates a new wire
__host__ __device__ inline Wire mkwire(Ptr p1, Ptr p2) {
  return (((u64)p1) << 32) | ((u64)p2);
}

// Gets the left element of a wire
__host__ __device__ inline Ptr wire_lft(Wire wire) {
  return wire >> 32;
}

// Gets the right element of a wire
__host__ __device__ inline Ptr wire_rgt(Wire wire) {
  return wire & 0xFFFFFFFF;
}

// Creates a new node
__host__ __device__ inline Node mknode(Ptr p1, Ptr p2) {
  Node node;
  node.ports[P1] = p1;
  node.ports[P2] = p2;
  return node;
}

// Creates a nil node
__host__ __device__ inline Node Node_nil() {
  return mknode(mkptr(NIL, 0), mkptr(NIL, 0));
}

// Gets a reference to the index/port Ptr on the net
__device__ inline Ptr* at(Net* net, Val idx, Port port) {
  return &net->node[idx].ports[port];
}

// Allocates one node in memory
__device__ inline u32 alloc(Unit *unit, Net *net) {
  while (true) {
    u64* ref = (u64*)&net->node[unit->aloc];
    u64  got = atomicCAS((u64*)ref, 0, ((u64)NEO << 32) | (u64)NEO);
    unit->aloc = (unit->aloc + 1) % NODE_SIZE;
    if (got == 0) {
      return (unit->aloc - 1) % NODE_SIZE;
    }
  }
}

// Allocates many nodes in memory
// TODO: use the entire squad to perform this
__device__ inline u32 alloc_many(Unit *unit, Net *net, u32 size) {
  u64 MKNEO = ((u64)NEO << 32) | (u64)NEO;
  u32 space = 0;
  while (true) {
    if (unit->aloc + size - space > NODE_SIZE) {
      unit->aloc = 0;
    }
    u64* ref = (u64*)&net->node[unit->aloc];
    u64  got = atomicCAS(ref, 0, MKNEO);
    if (got != 0) {
      for (u32 i = 0; i < space; ++i) {
        u32  index = (unit->aloc - space + i) % NODE_SIZE;
        Node clear = mknode(mkptr(NIL,0), mkptr(NIL,0));
        u64* ref = (u64*)&net->node[index];
        u64  got = atomicCAS(ref, MKNEO, 0);
      }
      space = 0;
    } else {
      space += 1;
    }
    unit->aloc = (unit->aloc + 1) % NODE_SIZE;
    if (space == size) {
      return (unit->aloc - space) % NODE_SIZE;
    }
  }
}

// Gets the value of a ref; waits if taken.
__device__ Ptr take(Ptr* ref) {
  Ptr got = atomicExch((u32*)ref, TKN);
  while (got == TKN) {
    got = atomicExch((u32*)ref, TKN);
  }
  return got;
}

// Attempts to replace 'exp' by 'neo', until it succeeds
__device__ bool replace(Ptr* ref, Ptr exp, Ptr neo) {
  Ptr got = atomicCAS((u32*)ref, exp, neo);
  while (got != exp) {
    got = atomicCAS((u32*)ref, exp, neo);
  }
  return true;
}

// Splits elements of two arrays evenly between each-other
__device__ inline void split(u32 tid, u64* a_len, u64* a_arr, u64* b_len, u64* b_arr, u64 max_len) {
  __syncthreads();
  u64* A_len = *a_len < *b_len ? a_len : b_len;
  u64* B_len = *a_len < *b_len ? b_len : a_len;
  u64* A_arr = *a_len < *b_len ? a_arr : b_arr;
  u64* B_arr = *a_len < *b_len ? b_arr : a_arr;
  bool move  = *A_len + 1 < *B_len;
  u64  min   = *A_len;
  u64  max   = *B_len;
  __syncthreads();
  for (u64 t = 0; t < max_len / (SQUAD_SIZE * 2); ++t) {
    u64 i = min + t * (SQUAD_SIZE * 2) + tid;
    u64 value;
    if (move && i < max) {
      value = B_arr[i];
      B_arr[i] = 0;
    }
    __syncthreads();
    if (move && i < max) {
      if ((i - min) % 2 == 0) {
        A_arr[min + (t * (SQUAD_SIZE * 2) + tid) / 2] = value;
      } else {
        B_arr[min + (t * (SQUAD_SIZE * 2) + tid) / 2] = value;
      }
    }
  }
  __syncthreads();
  u64 old_A_len = *A_len;
  u64 old_B_len = *B_len;
  if (move && tid == 0) {
    u64 new_A_len = (*A_len + *B_len) / 2 + (*A_len + *B_len) % 2;
    u64 new_B_len = (*A_len + *B_len) / 2;
    *A_len = new_A_len;
    *B_len = new_B_len;
  }
  __syncthreads();
}

// Pops a redex
__device__ inline Wire pop_redex(Unit* unit) {
  if (unit->qid == A1) {
    Wire redex = mkwire(0,0);
    if (*unit->rlen > 0) {
      u64 index = *unit->rlen - 1;
      *unit->rlen -= 1;
      redex = unit->rbag[index];
      unit->rbag[index] = mkwire(0,0);
    }
    *unit->sm64 = redex;
  }
  __syncwarp();
  Wire got = *unit->sm64;
  __syncwarp();
  *unit->sm64 = 0;
  if (unit->qid <= A2) {
    return mkwire(wire_lft(got), wire_rgt(got));
  } else {
    return mkwire(wire_rgt(got), wire_lft(got));
  }
}

// Puts a redex
__device__ inline void put_redex(Unit* unit, Ptr a_ptr, Ptr b_ptr) {
  // optimization: avoids pushing non-reactive redexes
  bool a_era = is_era(a_ptr);
  bool b_era = is_era(b_ptr);
  bool a_ref = is_ref(a_ptr);
  bool b_ref = is_ref(b_ptr);
  if (a_era && b_era || a_ref && b_era || a_era && b_ref || a_ref && b_ref) {
    unit->rwts += 1;
    return;
  }

  // pushes redex to end of bag
  u32 index = atomicAdd(unit->rlen, 1);
  if (index < RBAG_SIZE - 1) {
    unit->rbag[index] = mkwire(a_ptr, b_ptr);
  }
}

// Adjusts a dereferenced pointer
__device__ Ptr adjust(Unit* unit, Ptr ptr, u32 delta) {
  return mkptr(tag(ptr), has_loc(ptr) ? val(ptr) + delta : val(ptr));
}

// Expands a reference
__device__ void deref(Unit* unit, Net* net, Book* book, Ptr* ref, Ptr up) {
  // Loads definition
  const u32  term = ref != NULL ? book[val(*ref)] : 0;
  const u32  nlen = book[term + 0];
  const u32  alen = book[term + 1];
  const u32  root = book[term + 2];
  const u32* node = &book[term + 3];
  const u32* acts = &book[term + 3 + nlen*2];

  // Allocates needed space
  if (term && unit->qid == A1) {
    unit->sm32[0] = alloc_many(unit, net, nlen);
  }
  __syncwarp();

  if (term) {
    // Gets allocated index
    u32 loc = unit->sm32[0];

    // Loads dereferenced nodes, adjusted
    for (u32 i = 0; i < div(nlen, SQUAD_SIZE); ++i) {
      u32 idx = i * SQUAD_SIZE + unit->qid;
      if (idx < nlen) {
        Ptr p1 = adjust(unit, node[idx*2+0], loc);
        Ptr p2 = adjust(unit, node[idx*2+1], loc);
        *at(net, loc + idx, P1) = p1;
        *at(net, loc + idx, P2) = p2;
      }
    }

    // Loads dereferenced redexes, adjusted
    for (u32 i = 0; i < div(alen, SQUAD_SIZE); ++i) {
      u32 idx = i * SQUAD_SIZE + unit->qid;
      if (idx < alen) {
        Ptr p1 = adjust(unit, acts[idx*2+0], loc);
        Ptr p2 = adjust(unit, acts[idx*2+1], loc);
        put_redex(unit, p1, p2);
      }
    }

    // Loads dereferenced root, adjusted
    *ref = adjust(unit, root, loc);

    // Links root
    if (unit->qid == A1) {
      Ptr* trg = target(net, *ref);
      if (trg != NULL) {
        *trg = up;
      }
    }
  }
  __syncwarp();
}

// Atomically links the node in 'src_ref' towards 'trg_ptr'.
//__device__ void link(Unit* unit, Net* net, Book* book, Ptr dir_ptr, Ptr* src_ref, Ptr src_ptr) {
  //// Create a new redex
  //if (is_pri(src_ptr) && is_pri(dir_ptr)) {
    //atomicCAS(src_ref, TKN, 0);
    //put_redex(unit, *src_ref, dir_ptr);
    //return;
  //}

  //// Move src towards either a VAR or another PRI
  //if (is_pri(src_ptr) && is_var(dir_ptr)) {
    //while (true) {
      //// Peek the target, which may not be owned by us.
      //Ptr* trg_ref = target(net, dir_ptr);
      //Ptr  trg_ptr = atomicAdd(trg_ref, 0);

      //// If target is a redirection, clear and move forward.
      //if (is_red(trg_ptr)) {
        //// We own the redirection, so we can mutate it.
        //*trg_ref = 0;
        //dir_ptr = trg_ptr;
        //continue;
      //}

      //// If target is a variable, try replacing it by the node.
      //else if (is_var(trg_ptr)) {
        //// Peeks the source node.
        //Ptr src_ptr = *src_ref;

        //// We don't own the var, so we must try replacing with a CAS.
        //if (atomicCAS((u32*)trg_ref, trg_ptr, src_ptr) == trg_ptr) {
          //// Collect the orphaned backward path.
          //trg_ref = target(net, trg_ptr);
          //trg_ptr = *trg_ref;
          //while (is_red(trg_ptr)) {
            //*trg_ref = 0;
            //trg_ref = target(net, trg_ptr);
            //trg_ptr = *trg_ref;
          //}
          //// Clear source location.
          //*src_ref = 0;
          //return;
        //}

        //// If the CAS failed, the var changed, so we try again.
        //continue;
      //}

      //// If it is a node, two threads will reach this branch.
      //else if (is_pri(trg_ptr) || is_ref(trg_ptr) || trg_ptr == TMP) {

        //// Sort references, to avoid deadlocks.
        //Ptr *fst_ref = src_ref < trg_ref ? src_ref : trg_ref;
        //Ptr *snd_ref = src_ref < trg_ref ? trg_ref : src_ref;

        //// Swap first reference by TMP placeholder.
        //Ptr fst_ptr = atomicExch((u32*)fst_ref, TMP);

        //// First to arrive creates a redex.
        //if (fst_ptr != TMP) {
          //Ptr snd_ptr = atomicExch((u32*)snd_ref, TMP);
          //put_redex(unit, fst_ptr, snd_ptr);
          //return;

        //// Second to arrive clears up the memory.
        //} else {
          //*fst_ref = 0;
          //replace((u32*)snd_ref, TMP, 0);
          //return;
        //}
      //}

      //// If it is taken, we wait.
      //else if (trg_ptr == TKN) {
        //continue;
      //}

      //// Shouldn't be reached.
      //else {
        //return;
      //}
    //}
  //}

  //// Optimization: safely shorten redirections
  //if (is_var(src_ptr) && is_var(dir_ptr)) {
    //while (true) {
      //Ptr* ste_ref = target(net, src_ptr);
      //Ptr  ste_ptr = *ste_ref;
      //if (is_var(ste_ptr)) {
        //Ptr* trg_ref = target(net, ste_ptr);
        //Ptr  trg_ptr = atomicAdd(trg_ref, 0);
        //if (is_red(trg_ptr)) {
          //Ptr neo_ptr = mkptr(tag(trg_ptr) - 3, val(trg_ptr));
          //Ptr updated = atomicCAS(ste_ref, ste_ptr, neo_ptr);
          //if (updated == ste_ptr) {
            //*trg_ref = 0;
            //continue;
          //}
        //}
      //}
      //break;
    //}
    //return;
  //}
//}

//__device__ void atomic_rewrite(Unit* unit, Net* net, Book* book, Ptr a_dir, Ptr b_dir) {
  //Ptr a_ptr = atomicExch(target(net, a_dir), TKN);
  //Ptr b_ptr = atomicExch(target(net, b_dir), TKN);
  //atomic_subst(unit, net, book, a_ptr, a_dir, b_ptr);
  //atomic_subst(unit, net, book, b_ptr, b_dir, a_ptr);
//}


// Rewrite
// -------

__device__ Unit init_unit(Net* net, bool flip) {
  __shared__ u32 SMEM[GROUP_SIZE * SMEM_SIZE];

  for (u32 i = 0; i < GROUP_SIZE * SMEM_SIZE / BLOCK_SIZE; ++i) {
    SMEM[i * BLOCK_SIZE + threadIdx.x] = 0;
  }
  __syncthreads();

  u32 tid = threadIdx.x;
  u32 gid = blockIdx.x * blockDim.x + tid;
  u32 uid = gid / SQUAD_SIZE;
  u32 row = uid / GROUP_SIZE;
  u32 col = uid % GROUP_SIZE;

  Unit unit;
  unit.uid  = flip ? col * GROUP_SIZE + row : row * GROUP_SIZE + col;
  unit.tid  = threadIdx.x;
  unit.qid  = unit.tid % 4;
  unit.aloc = rng(clock() * (gid + 1)) % NODE_SIZE;
  //unit.aloc = 0;
  unit.rwts = 0;
  unit.port = unit.tid % 2;
  unit.sm32 = (u32*)(SMEM + unit.tid / SQUAD_SIZE * SMEM_SIZE);
  unit.sm64 = (u64*)(SMEM + unit.tid / SQUAD_SIZE * SMEM_SIZE);
  unit.rlen = net->bags + unit.uid * RBAG_SIZE;
  unit.rbag = unit.rlen + 1;

  return unit;
}


__device__ void atomic_join(Unit* unit, Net* net, Book* book, Ptr a_ptr, Ptr* a_ref, Ptr b_ptr) {
  while (true) {
    Ptr* ste_ref = target(net, b_ptr);
    Ptr  ste_ptr = *ste_ref;
    if (is_var(ste_ptr)) {
      Ptr* trg_ref = target(net, ste_ptr);
      Ptr  trg_ptr = atomicAdd(trg_ref, 0);
      if (is_red(trg_ptr)) {
        Ptr neo_ptr = undir(trg_ptr);
        Ptr updated = atomicCAS(ste_ref, ste_ptr, neo_ptr);
        if (updated == ste_ptr) {
          *trg_ref = 0;
          continue;
        }
      }
    }
    break;
  }
}

__device__ void atomic_link(Unit* unit, Net* net, Book* book, Ptr a_ptr, Ptr* a_ref, Ptr b_ptr) {
  while (true) {
    // Peek the target, which may not be owned by us.
    Ptr* t_ref = target(net, a_ptr);
    Ptr  t_ptr = atomicAdd(t_ref, 0);

    // If target is a redirection, clear and move forward.
    if (is_red(t_ptr)) {
      // We own the redirection, so we can mutate it.
      *t_ref = 0;
      a_ptr = t_ptr;
      continue;
    }

    // If target is a variable, try replacing it by the node.
    else if (is_var(t_ptr)) {
      // We don't own the var, so we must try replacing with a CAS.
      if (atomicCAS(t_ref, t_ptr, b_ptr) == t_ptr) {
        // Collect the orphaned backward path.
        t_ref = target(net, t_ptr);
        t_ptr = *t_ref;
        while (is_red(t_ptr)) {
          *t_ref = 0;
          t_ref = target(net, t_ptr);
          t_ptr = *t_ref;
        }
        // Clear source location.
        *a_ref = 0;
        return;
      }

      // If the CAS failed, the var changed, so we try again.
      continue;
    }

    // If it is a node, two threads will reach this branch.
    else if (is_pri(t_ptr) || is_ref(t_ptr) || t_ptr == TMP) {
      // Sort references, to avoid deadlocks.
      Ptr *x_ref = a_ref < t_ref ? a_ref : t_ref;
      Ptr *y_ref = a_ref < t_ref ? t_ref : a_ref;

      // Swap first reference by TMP placeholder.
      Ptr x_ptr = atomicExch(x_ref, TMP);

      // First to arrive creates a redex.
      if (x_ptr != TMP) {
        Ptr y_ptr = atomicExch(y_ref, TMP);
        put_redex(unit, x_ptr, y_ptr);
        return;

      // Second to arrive clears up the memory.
      } else {
        *x_ref = 0;
        replace(y_ref, TMP, 0);
        return;
      }
    }

    // If it is taken, we wait.
    else if (t_ptr == TKN) {
      continue;
    }

    // Shouldn't be reached.
    else {
      return;
    }
  }
}

// An active wire is reduced by 4 parallel threads, each one performing "1/4" of
// the work. Each thread will be pointing to a node of the active pair, and an
// aux port of that node. So, when nodes A-B interact, we have 4 thread quads:
// - Thread A1: points to node A and its aux1
// - Thread A2: points to node A and its aux2
// - Thread B1: points to node B and its aux1
// - Thread B2: points to node B and its aux2
// This is organized so that local threads can perform the same instructions
// whenever possible. So, for example, in a commutation rule, all the 4 clones
// would be allocated at the same time.
__global__ void global_rewrite(Net* net, Book* book, u32 repeat, u32 tick, bool flip) {

  // Initializes local vars
  Unit unit = init_unit(net, flip);

  for (u32 turn = 0; turn < repeat; ++turn) {
    // Checks if we're full
    bool is_full = *unit.rlen > RBAG_SIZE - MAX_NEW_REDEX;

    // Pops a redex from local bag
    Wire redex;
    Ptr a_ptr, b_ptr;
    if (!is_full) {
      redex = pop_redex(&unit);
      a_ptr = wire_lft(redex);
      b_ptr = wire_rgt(redex);
    }
    __syncwarp();

    // Dereferences
    Ptr* deref_ptr = NULL;
    if (is_ref(a_ptr) && is_ctr(b_ptr)) {
      deref_ptr = &a_ptr;
    }
    if (is_ref(b_ptr) && is_ctr(a_ptr)) {
      deref_ptr = &b_ptr;
    }
    deref(&unit, net, book, deref_ptr, mkptr(NIL,0));

    // Defines type of interaction
    bool rewrite = !is_full && a_ptr != 0 && b_ptr != 0;
    bool var_pri = rewrite && is_var(a_ptr) && is_pri(b_ptr) && unit.port == P1;
    bool era_ctr = rewrite && is_era(a_ptr) && is_ctr(b_ptr);
    bool ctr_era = rewrite && is_ctr(a_ptr) && is_era(b_ptr);
    bool con_con = rewrite && is_ctr(a_ptr) && is_ctr(b_ptr) && tag(a_ptr) == tag(b_ptr);
    bool con_dup = rewrite && is_ctr(a_ptr) && is_ctr(b_ptr) && tag(a_ptr) != tag(b_ptr);

    // Local rewrite variables
    Ptr  ak_dir; // dir to our aux port
    Ptr  bk_dir; // dir to other aux port
    Ptr *ak_ref; // ref to our aux port
    Ptr *bk_ref; // ref to other aux port
    Ptr  ak_ptr; // val of our aux port
    Ptr  bk_ptr; // val to other aux port
    Ptr  mv_ptr; // val of ptr to send to other side

    // Inc rewrite count
    if (rewrite && unit.qid == A1) {
      unit.rwts += 1;
    }

    // Gets port here
    if (rewrite && (ctr_era || con_con || con_dup)) {
      ak_dir = mkptr(VR1 + unit.port, val(a_ptr));
      ak_ref = target(net, ak_dir);
      ak_ptr = take(ak_ref);
    }

    // Gets port there
    if (rewrite && (era_ctr || con_con || con_dup)) {
      bk_dir = mkptr(VR1 + unit.port, val(b_ptr));
      bk_ref = target(net, bk_dir);
    }

    // If era_ctr, send an erasure
    if (rewrite && era_ctr) {
      mv_ptr = mkptr(ERA, 0);
    }

    // If con_con, send a redirection
    if (rewrite && con_con) {
      mv_ptr = ak_ptr;
    }

    // If con_dup, alloc clones base index
    if (rewrite && con_dup && unit.qid == A1) {
      unit.sm32[0] = alloc_many(&unit, net, 4);
    }
    __syncwarp();

    // If con_dup, create inner wires between clones
    if (rewrite && con_dup) {
      u32 al_loc = unit.sm32[0];
      u32 cx_loc = al_loc + unit.qid;
      u32 c1_loc = al_loc + (unit.qid <= A2 ? 2 : 0);
      u32 c2_loc = al_loc + (unit.qid <= A2 ? 3 : 1);
      replace(at(net, cx_loc, P1), NEO, mkptr(unit.port == P1 ? VR1 : VR2, c1_loc));
      replace(at(net, cx_loc, P2), NEO, mkptr(unit.port == P1 ? VR1 : VR2, c2_loc));
      mv_ptr = mkptr(tag(a_ptr), cx_loc);
    }
    __syncwarp();

    // Send ptr to other side
    if (rewrite && (era_ctr || con_con || con_dup)) {
      unit.sm32[unit.qid + (unit.qid <= A2 ? 2 : -2)] = mv_ptr;
    }
    __syncwarp();

    // Receive ptr from other side
    if (rewrite && (con_con || ctr_era || con_dup)) {
      bk_ptr = unit.sm32[unit.qid];
    }
    __syncwarp();

    // If var_pri, the var must be a deref root, so we just subst
    if (rewrite && var_pri && unit.port == P1) {
      atomicExch(target(net, a_ptr), b_ptr);
    }
    __syncwarp();

    // Substitutes
    if (rewrite && (con_con || ctr_era || con_dup)) {
      if (is_var(ak_ptr)) {
        Ptr got = atomicCAS(target(net, ak_ptr), ak_dir, bk_ptr);
        if (got == ak_dir) {
          //printf("[%04x:%x] >> success\n", unit.uid, unit.qid);
          atomicExch(ak_ref, mkptr(NIL, 0));
        } else if (is_var(bk_ptr)) {
          //printf("[%04x:%x] >> failure var\n", unit.uid, unit.qid);
          atomicExch(ak_ref, redir(bk_ptr));
          atomic_join(&unit, net, book, ak_ptr, ak_ref, redir(bk_ptr));
        } else if (is_pri(bk_ptr)) {
          //printf("[%04x:%x] >> failure pri\n", unit.uid, unit.qid);
          atomicExch(ak_ref, bk_ptr);
          atomic_link(&unit, net, book, ak_ptr, ak_ref, bk_ptr);
        }
      } else if (is_pri(ak_ptr) && is_pri(bk_ptr)) {
        //printf("[%04x:%x] >> new redex\n", unit.uid, unit.qid);
        if (ak_ptr > bk_ptr || ctr_era || con_dup) {
          put_redex(&unit, bk_ptr, ak_ptr); // FIXME: swapping bloats rbag; why?
        }
        atomicExch(ak_ref, mkptr(NIL, 0));
      } else {
        atomicExch(ak_ref, mkptr(NIL, 0));
      }
    }
    __syncwarp();
  }

  // Splits redexes with neighbor
  u32  side  = ((unit.tid / SQUAD_SIZE) >> (GROUP_LOG2 - 1 - (tick % GROUP_LOG2))) & 1;
  u32  lpad  = (1 << (GROUP_LOG2 - 1)) >> (tick % GROUP_LOG2);
  u32  gpad  = flip ? lpad * GROUP_SIZE : lpad;
  u32  a_uid = unit.uid;
  u32  b_uid = side ? unit.uid - gpad : unit.uid + gpad;
  u64* a_len = net->bags + a_uid * RBAG_SIZE;
  u64* b_len = net->bags + b_uid * RBAG_SIZE;
  split(unit.qid + side * SQUAD_SIZE, a_len, a_len+1, b_len, b_len+1, RBAG_SIZE);
  __syncthreads();

  // When the work ends, sum stats
  if (unit.rwts > 0) {
    atomicAdd(&net->rwts, unit.rwts);
  }
}

void do_global_rewrite(Net* net, Book* book, u32 repeat, u32 tick, bool flip) {
  global_rewrite<<<GROUP_SIZE, BLOCK_SIZE>>>(net, book, repeat, tick, flip);
  // print any error launching this kernel:
  cudaError_t err = cudaGetLastError();
  if (err != cudaSuccess) {
    printf("CUDA error: %s\n", cudaGetErrorString(err));
  }
}

// Expand
// ------

// Collects local expansion heads recursively
__device__ void expand(Unit* unit, Net* net, Book* book, Ptr dir, u32* len, u32* lhds) {
  Ptr ptr = *target(net, dir);
  if (is_ctr(ptr)) {
    expand(unit, net, book, mkptr(VR1, val(ptr)), len, lhds);
    expand(unit, net, book, mkptr(VR2, val(ptr)), len, lhds);
  } else if (is_red(ptr)) {
    expand(unit, net, book, ptr, len, lhds);
  } else if (is_ref(ptr) && *len < LHDS_SIZE) {
    lhds[(*len)++] = dir;
  }
}

// Takes an initial head location for each squad
__global__ void global_expand_prepare(Net* net) {
  u32 uid = blockIdx.x * blockDim.x + threadIdx.x;

  // Traverses down
  u32 key = uid;
  Ptr dir = mkptr(VRR, 0);
  Ptr ptr, *ref;
  for (u32 depth = 0; depth < HEAD_LOG2; ++depth) {
    dir = enter(net, dir);
    ref = target(net, dir);
    if (ref != NULL) {
      ptr = *ref;
      if (is_ctr(ptr)) {
        dir = mkptr(key & 1 ? VR1 : VR2, val(ptr));
        key = key >> 1;
      }
    }
  }

  // Takes ptr
  dir = enter(net, dir);
  ref = target(net, dir);
  if (ref != NULL) {
    ptr = atomicExch(ref, TKN);
  }

  // Stores ptr
  if (ptr != TKN) {
    net->head[uid] = mkwire(dir, ptr);
  } else {
    net->head[uid] = mkwire(mkptr(NIL,0), mkptr(NIL,0));
  }

}

// Performs global expansion of heads
__global__ void global_expand(Net* net, Book* book) {
  __shared__ u32 HEAD[GROUP_SIZE * LHDS_SIZE];

  for (u32 i = 0; i < GROUP_SIZE * LHDS_SIZE / BLOCK_SIZE; ++i) {
    HEAD[i * BLOCK_SIZE + threadIdx.x] = 0;
  }
  __syncthreads();

  Unit unit = init_unit(net, 0);

  u32* head = HEAD + unit.tid / SQUAD_SIZE * LHDS_SIZE;

  Wire got = net->head[unit.uid];
  Ptr  dir = wire_lft(got);
  Ptr* ref = target(net, dir);
  Ptr  ptr = wire_rgt(got);

  if (unit.qid == A1 && ptr != mkptr(NIL,0)) {
    *ref = ptr;
  }
  __syncthreads();

  u32 len = 0;
  if (unit.qid == A1 && ptr != mkptr(NIL,0)) {
    expand(&unit, net, book, dir, &len, head);
  }
  __syncthreads();

  for (u32 i = 0; i < LHDS_SIZE; ++i) {
    Ptr  dir = head[i];
    Ptr* ref = target(net, dir);
    if (ref != NULL && !is_ref(*ref)) {
      ref = NULL;
    }
    deref(&unit, net, book, ref, dir);
  }
  __syncthreads();
}

// Performs a global head expansion
void do_global_expand(Net* net, Book* book) {
  global_expand_prepare<<<GROUP_SIZE, GROUP_SIZE>>>(net);
  global_expand<<<GROUP_SIZE, BLOCK_SIZE>>>(net, book);
}

// Host<->Device
// -------------

__host__ Net* mknet(u32 root_fn) {
  Net* net  = (Net*)malloc(sizeof(Net));
  net->root = mkptr(REF, root_fn);
  net->rwts = 0;
  net->done = 0;
  net->bags = (Wire*)malloc(BAGS_SIZE * sizeof(Wire));
  net->node = (Node*)malloc(NODE_SIZE * sizeof(Node));
  net->head = (Wire*)malloc(HEAD_SIZE * sizeof(Wire));
  memset(net->bags, 0, BAGS_SIZE * sizeof(Wire));
  memset(net->node, 0, NODE_SIZE * sizeof(Node));
  return net;
}

__host__ Net* net_to_gpu(Net* host_net) {
  // Allocate memory on the device for the Net object, and its data
  Net*  device_net;
  Wire* device_bags;
  Node* device_node;
  Wire* device_head;

  cudaMalloc((void**)&device_net, sizeof(Net));
  cudaMalloc((void**)&device_bags, BAGS_SIZE * sizeof(Wire));
  cudaMalloc((void**)&device_node, NODE_SIZE * sizeof(Node));
  cudaMalloc((void**)&device_head, HEAD_SIZE * sizeof(Wire));

  // Copy the host data to the device memory
  cudaMemcpyAsync(device_bags, host_net->bags, BAGS_SIZE * sizeof(Wire), cudaMemcpyHostToDevice);
  cudaMemcpyAsync(device_node, host_net->node, NODE_SIZE * sizeof(Node), cudaMemcpyHostToDevice);
  cudaMemcpyAsync(device_head, host_net->head, HEAD_SIZE * sizeof(Wire), cudaMemcpyHostToDevice);

  // Create a temporary host Net object with device pointers
  Net temp_net  = *host_net;
  temp_net.bags = device_bags;
  temp_net.node = device_node;
  temp_net.head = device_head;

  // Copy the temporary host Net object to the device memory
  cudaMemcpyAsync(device_net, &temp_net, sizeof(Net), cudaMemcpyHostToDevice);

  // Return the device pointer to the created Net object
  return device_net;
}

__host__ Net* net_to_cpu(Net* device_net) {
  // Create a new host Net object
  Net* host_net = (Net*)malloc(sizeof(Net));

  // Copy the device Net object to the host memory
  cudaMemcpyAsync(host_net, device_net, sizeof(Net), cudaMemcpyDeviceToHost);

  // Allocate host memory for data
  host_net->bags = (Wire*)malloc(BAGS_SIZE * sizeof(Wire));
  host_net->node = (Node*)malloc(NODE_SIZE * sizeof(Node));
  host_net->head = (Wire*)malloc(HEAD_SIZE * sizeof(Wire));

  // Retrieve the device pointers for data
  Wire* device_bags;
  Node* device_node;
  Ptr*  device_head;
  cudaMemcpyAsync(&device_bags, &(device_net->bags), sizeof(Wire*), cudaMemcpyDeviceToHost);
  cudaMemcpyAsync(&device_node, &(device_net->node), sizeof(Node*), cudaMemcpyDeviceToHost);
  cudaMemcpyAsync(&device_head, &(device_net->head), sizeof(Wire*), cudaMemcpyDeviceToHost);

  // Copy the device data to the host memory
  cudaMemcpyAsync(host_net->bags, device_bags, BAGS_SIZE * sizeof(Wire), cudaMemcpyDeviceToHost);
  cudaMemcpyAsync(host_net->node, device_node, NODE_SIZE * sizeof(Node), cudaMemcpyDeviceToHost);
  cudaMemcpyAsync(host_net->head, device_head, HEAD_SIZE * sizeof(Wire), cudaMemcpyDeviceToHost);

  return host_net;
}

__host__ void net_free_on_gpu(Net* device_net) {
  // Retrieve the device pointers for data
  Wire* device_bags;
  Node* device_node;
  Wire* device_head;
  cudaMemcpyAsync(&device_bags, &(device_net->bags), sizeof(Wire*), cudaMemcpyDeviceToHost);
  cudaMemcpyAsync(&device_node, &(device_net->node), sizeof(Node*), cudaMemcpyDeviceToHost);
  cudaMemcpyAsync(&device_head, &(device_net->head), sizeof(Wire*), cudaMemcpyDeviceToHost);

  // Free the device memory
  cudaFree(device_bags);
  cudaFree(device_node);
  cudaFree(device_head);
  cudaFree(device_net);
}

__host__ void net_free_on_cpu(Net* host_net) {
  free(host_net->bags);
  free(host_net->node);
  free(host_net->head);
  free(host_net);
}

// Debugging
// ---------

__host__ const char* show_ptr(Ptr ptr, u32 slot) {
  static char buffer[8][20];
  if (ptr == 0) {
    strcpy(buffer[slot], "           ");
    return buffer[slot];
  } else if (ptr == TKN) {
    strcpy(buffer[slot], "[.........]");
    return buffer[slot];
  } else {
    const char* tag_str = NULL;
    switch (tag(ptr)) {
      case VR1: tag_str = "VR1"; break;
      case VR2: tag_str = "VR2"; break;
      case NIL: tag_str = "NIL"; break;
      case REF: tag_str = "REF"; break;
      case NUM: tag_str = "NUM"; break;
      case ERA: tag_str = "ERA"; break;
      case VRR: tag_str = "VRR"; break;
      case RDR: tag_str = "RDR"; break;
      case RD1: tag_str = "RD1"; break;
      case RD2: tag_str = "RD2"; break;
      case CON: tag_str = "CON"; break;
      case DUP: tag_str = "DUP"; break;
      case TRI: tag_str = "TRI"; break;
      default:tag_str = tag(ptr) >= DUP ? "DUP" : "???"; break;
    }
    snprintf(buffer[slot], sizeof(buffer[slot]), "%s:%07X", tag_str, val(ptr));
    return buffer[slot];
  }
}

// Prints a net in hexadecimal, limited to a given size
void print_net(Net* net) {
  printf("Root:\n");
  printf("- %s\n", show_ptr(net->root,0));
  printf("Bags:\n");
  for (u32 i = 0; i < BAGS_SIZE; ++i) {
    if (i % RBAG_SIZE == 0 && net->bags[i] > 0) {
      printf("- [%07X] LEN=%llu\n", i, net->bags[i]);
      if (i > 0) break;
    } else {
      Ptr a = wire_lft(net->bags[i]);
      Ptr b = wire_rgt(net->bags[i]);
      if (a != 0 || b != 0) {
        printf("- [%07X] %s %s\n", i, show_ptr(a,0), show_ptr(b,1));
      }
    }
  }
  //printf("Node:\n");
  //for (u32 i = 0; i < NODE_SIZE; ++i) {
    //Ptr a = net->node[i].ports[P1];
    //Ptr b = net->node[i].ports[P2];
    //if (a != 0 || b != 0) {
      //printf("- [%07X] %s %s\n", i, show_ptr(a,0), show_ptr(b,1));
    //}
  //}
  printf("Rwts: %llu\n", net->rwts);
}

// Struct to represent a Map of entries using a simple array of (key,id) pairs
typedef struct {
  u32 keys[65536];
  u32 vals[65536];
  u32 size;
} Map;

// Function to insert a new entry into the map
__host__ void map_insert(Map* map, u32 key, u32 val) {
  map->keys[map->size] = key;
  map->vals[map->size] = val;
  map->size++;
}

// Function to lookup an id in the map by key
__host__ u32 map_lookup(Map* map, u32 key) {
  for (u32 i = 0; i < map->size; ++i) {
    if (map->keys[i] == key) {
      return map->vals[i];
    }
  }
  return map->size;
}

// Recursive function to print a term as a tree with unique variable IDs
__host__ void print_tree_go(Net* net, Ptr ptr, Map* var_ids) {
  if (is_var(ptr)) {
    u32 got = map_lookup(var_ids, ptr);
    if (got == var_ids->size) {
      u32 name = var_ids->size;
      Ptr targ = *target(net, enter(net, ptr));
      map_insert(var_ids, targ, name);
      printf("x%d", name);
    } else {
      printf("x%d", got);
    }
  } else if (is_ref(ptr)) {
    printf("{%x}", val(ptr));
  } else if (tag(ptr) == ERA) {
    printf("*");
  } else {
    switch (tag(ptr)) {
      case RDR: case RD1: case RD2:
        print_tree_go(net, *target(net, ptr), var_ids);
        break;
      default:
        printf("(%d ", tag(ptr) - CON);
        print_tree_go(net, net->node[val(ptr)].ports[P1], var_ids);
        printf(" ");
        print_tree_go(net, net->node[val(ptr)].ports[P2], var_ids);
        printf(")");
    }
  }
}

__host__ void print_tree(Net* net, Ptr ptr) {
  Map var_ids = { .size = 0 };
  print_tree_go(net, ptr, &var_ids);
  printf("\n");
}

// Book
// ----

const u32 F_E = 0x0000;
const u32 F_F = 0x0001;
const u32 F_I = 0x0002;
const u32 F_O = 0x0003;
const u32 F_S = 0x0004;
const u32 F_T = 0x0005;
const u32 F_Z = 0x0006;
const u32 F_af = 0x0007;
const u32 F_c0 = 0x0008;
const u32 F_c1 = 0x0009;
const u32 F_c2 = 0x000a;
const u32 F_c3 = 0x000b;
const u32 F_c4 = 0x000c;
const u32 F_c5 = 0x000d;
const u32 F_c6 = 0x000e;
const u32 F_c7 = 0x000f;
const u32 F_c8 = 0x0010;
const u32 F_c9 = 0x0011;
const u32 F_id = 0x0012;
const u32 F_k0 = 0x0013;
const u32 F_k1 = 0x0014;
const u32 F_k2 = 0x0015;
const u32 F_k3 = 0x0016;
const u32 F_k4 = 0x0017;
const u32 F_k5 = 0x0018;
const u32 F_k6 = 0x0019;
const u32 F_k7 = 0x001a;
const u32 F_k8 = 0x001b;
const u32 F_k9 = 0x001c;
const u32 F_afS = 0x001d;
const u32 F_afZ = 0x001e;
const u32 F_and = 0x001f;
const u32 F_brn = 0x0020;
const u32 F_c10 = 0x0021;
const u32 F_c11 = 0x0022;
const u32 F_c12 = 0x0023;
const u32 F_c13 = 0x0024;
const u32 F_c14 = 0x0025;
const u32 F_c15 = 0x0026;
const u32 F_c16 = 0x0027;
const u32 F_c17 = 0x0028;
const u32 F_c18 = 0x0029;
const u32 F_c19 = 0x002a;
const u32 F_c20 = 0x002b;
const u32 F_c21 = 0x002c;
const u32 F_c22 = 0x002d;
const u32 F_c23 = 0x002e;
const u32 F_c24 = 0x002f;
const u32 F_c25 = 0x0030;
const u32 F_c26 = 0x0031;
const u32 F_c_s = 0x0032;
const u32 F_c_z = 0x0033;
const u32 F_dec = 0x0034;
const u32 F_ex0 = 0x0035;
const u32 F_ex1 = 0x0036;
const u32 F_ex2 = 0x0037;
const u32 F_ex3 = 0x0038;
const u32 F_ex4 = 0x0039;
const u32 F_g_s = 0x003a;
const u32 F_g_z = 0x003b;
const u32 F_k10 = 0x003c;
const u32 F_k11 = 0x003d;
const u32 F_k12 = 0x003e;
const u32 F_k13 = 0x003f;
const u32 F_k14 = 0x0040;
const u32 F_k15 = 0x0041;
const u32 F_k16 = 0x0042;
const u32 F_k17 = 0x0043;
const u32 F_k18 = 0x0044;
const u32 F_k19 = 0x0045;
const u32 F_k20 = 0x0046;
const u32 F_k21 = 0x0047;
const u32 F_k22 = 0x0048;
const u32 F_k23 = 0x0049;
const u32 F_k24 = 0x004a;
const u32 F_low = 0x004b;
const u32 F_mul = 0x004c;
const u32 F_nid = 0x004d;
const u32 F_not = 0x004e;
const u32 F_run = 0x004f;
const u32 F_brnS = 0x0050;
const u32 F_brnZ = 0x0051;
const u32 F_decI = 0x0052;
const u32 F_decO = 0x0053;
const u32 F_lowI = 0x0054;
const u32 F_lowO = 0x0055;
const u32 F_nidS = 0x0056;
const u32 F_runI = 0x0057;
const u32 F_runO = 0x0058;

u32 BOOK_DATA[] = {
  0x00000059, // E
  0x00000062, // F
  0x00000069, // I
  0x00000076, // O
  0x00000083, // S
  0x0000008E, // T
  0x00000095, // Z
  0x0000009C, // af
  0x000000A5, // c0
  0x000000AC, // c1
  0x000000B5, // c2
  0x000000C2, // c3
  0x000000D3, // c4
  0x000000E8, // c5
  0x00000101, // c6
  0x0000011E, // c7
  0x0000013F, // c8
  0x00000164, // c9
  0x0000018D, // id
  0x00000192, // k0
  0x00000199, // k1
  0x000001A2, // k2
  0x000001AF, // k3
  0x000001C0, // k4
  0x000001D5, // k5
  0x000001EE, // k6
  0x0000020B, // k7
  0x0000022C, // k8
  0x00000251, // k9
  0x0000027A, // afS
  0x0000028F, // afZ
  0x00000292, // and
  0x000002A7, // brn
  0x000002B0, // c10
  0x000002DD, // c11
  0x0000030E, // c12
  0x00000343, // c13
  0x0000037C, // c14
  0x000003B9, // c15
  0x000003FA, // c16
  0x0000043F, // c17
  0x00000488, // c18
  0x000004D5, // c19
  0x00000526, // c20
  0x0000057B, // c21
  0x000005D4, // c22
  0x00000631, // c23
  0x00000692, // c24
  0x000006F7, // c25
  0x00000760, // c26
  0x000007CD, // c_s
  0x000007DE, // c_z
  0x000007E5, // dec
  0x000007F0, // ex0
  0x000007F7, // ex1
  0x00000800, // ex2
  0x0000080D, // ex3
  0x0000081A, // ex4
  0x00000827, // g_s
  0x00000834, // g_z
  0x00000839, // k10
  0x00000866, // k11
  0x00000897, // k12
  0x000008CC, // k13
  0x00000905, // k14
  0x00000942, // k15
  0x00000983, // k16
  0x000009C8, // k17
  0x00000A11, // k18
  0x00000A5E, // k19
  0x00000AAF, // k20
  0x00000B04, // k21
  0x00000B5D, // k22
  0x00000BBA, // k23
  0x00000C1B, // k24
  0x00000C80, // low
  0x00000C8B, // mul
  0x00000C98, // nid
  0x00000CA1, // not
  0x00000CAE, // run
  0x00000CB9, // brnS
  0x00000CCA, // brnZ
  0x00000CDB, // decI
  0x00000CE4, // decO
  0x00000CF1, // lowI
  0x00000CFE, // lowO
  0x00000D0B, // nidS
  0x00000D18, // runI
  0x00000D29, // runO
  // E
  0x00000003, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0x20000000, 0xA0000001,  0x20000000, 0xA0000002,  0x50000002, 0x40000002,
  // .acts
  // F
  0x00000002, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0x20000000, 0xA0000001,  0x50000001, 0x40000001,
  // .acts
  // I
  0x00000005, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0x40000003, 0xA0000001,  0x20000000, 0xA0000002,  0xA0000003, 0xA0000004,  0x40000000, 0x50000004,
  0x20000000, 0x50000003,
  // .acts
  // O
  0x00000005, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0x40000002, 0xA0000001,  0xA0000002, 0xA0000003,  0x40000000, 0x50000004,  0x20000000, 0xA0000004,
  0x20000000, 0x50000002,
  // .acts
  // S
  0x00000004, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0x40000002, 0xA0000001,  0xA0000002, 0xA0000003,  0x40000000, 0x50000003,  0x20000000, 0x50000002,
  // .acts
  // T
  0x00000002, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0x50000001, 0xA0000001,  0x20000000, 0x40000000,
  // .acts
  // Z
  0x00000002, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0x20000000, 0xA0000001,  0x50000001, 0x40000001,
  // .acts
  // af
  0x00000003, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xA0000001, 0x50000002,  0x1000001D, 0xA0000002,  0x1000001E, 0x50000000,
  // .acts
  // c0
  0x00000002, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0x20000000, 0xA0000001,  0x50000001, 0x40000001,
  // .acts
  // c1
  0x00000003, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xA0000001, 0xA0000002,  0x40000002, 0x50000002,  0x40000001, 0x50000001,
  // .acts
  // c2
  0x00000005, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xB0000001, 0xA0000004,  0xA0000002, 0xA0000003,  0x40000004, 0x40000003,  0x50000002, 0x50000004,
  0x40000002, 0x50000003,
  // .acts
  // c3
  0x00000007, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xB0000001, 0xA0000006,  0xB0000002, 0xA0000005,  0xA0000003, 0xA0000004,  0x40000006, 0x40000004,
  0x50000003, 0x40000005,  0x50000004, 0x50000006,  0x40000003, 0x50000005,
  // .acts
  // c4
  0x00000009, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xB0000001, 0xA0000008,  0xB0000002, 0xA0000007,  0xB0000003, 0xA0000006,  0xA0000004, 0xA0000005,
  0x40000008, 0x40000005,  0x50000004, 0x40000006,  0x50000005, 0x40000007,  0x50000006, 0x50000008,
  0x40000004, 0x50000007,
  // .acts
  // c5
  0x0000000B, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xB0000001, 0xA000000A,  0xB0000002, 0xA0000009,  0xB0000003, 0xA0000008,  0xB0000004, 0xA0000007,
  0xA0000005, 0xA0000006,  0x4000000A, 0x40000006,  0x50000005, 0x40000007,  0x50000006, 0x40000008,
  0x50000007, 0x40000009,  0x50000008, 0x5000000A,  0x40000005, 0x50000009,
  // .acts
  // c6
  0x0000000D, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xB0000001, 0xA000000C,  0xB0000002, 0xA000000B,  0xB0000003, 0xA000000A,  0xB0000004, 0xA0000009,
  0xB0000005, 0xA0000008,  0xA0000006, 0xA0000007,  0x4000000C, 0x40000007,  0x50000006, 0x40000008,
  0x50000007, 0x40000009,  0x50000008, 0x4000000A,  0x50000009, 0x4000000B,  0x5000000A, 0x5000000C,
  0x40000006, 0x5000000B,
  // .acts
  // c7
  0x0000000F, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xB0000001, 0xA000000E,  0xB0000002, 0xA000000D,  0xB0000003, 0xA000000C,  0xB0000004, 0xA000000B,
  0xB0000005, 0xA000000A,  0xB0000006, 0xA0000009,  0xA0000007, 0xA0000008,  0x4000000E, 0x40000008,
  0x50000007, 0x40000009,  0x50000008, 0x4000000A,  0x50000009, 0x4000000B,  0x5000000A, 0x4000000C,
  0x5000000B, 0x4000000D,  0x5000000C, 0x5000000E,  0x40000007, 0x5000000D,
  // .acts
  // c8
  0x00000011, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xB0000001, 0xA0000010,  0xB0000002, 0xA000000F,  0xB0000003, 0xA000000E,  0xB0000004, 0xA000000D,
  0xB0000005, 0xA000000C,  0xB0000006, 0xA000000B,  0xB0000007, 0xA000000A,  0xA0000008, 0xA0000009,
  0x40000010, 0x40000009,  0x50000008, 0x4000000A,  0x50000009, 0x4000000B,  0x5000000A, 0x4000000C,
  0x5000000B, 0x4000000D,  0x5000000C, 0x4000000E,  0x5000000D, 0x4000000F,  0x5000000E, 0x50000010,
  0x40000008, 0x5000000F,
  // .acts
  // c9
  0x00000013, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xB0000001, 0xA0000012,  0xB0000002, 0xA0000011,  0xB0000003, 0xA0000010,  0xB0000004, 0xA000000F,
  0xB0000005, 0xA000000E,  0xB0000006, 0xA000000D,  0xB0000007, 0xA000000C,  0xB0000008, 0xA000000B,
  0xA0000009, 0xA000000A,  0x40000012, 0x4000000A,  0x50000009, 0x4000000B,  0x5000000A, 0x4000000C,
  0x5000000B, 0x4000000D,  0x5000000C, 0x4000000E,  0x5000000D, 0x4000000F,  0x5000000E, 0x40000010,
  0x5000000F, 0x40000011,  0x50000010, 0x50000012,  0x40000009, 0x50000011,
  // .acts
  // id
  0x00000001, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0x50000000, 0x40000000,
  // .acts
  // k0
  0x00000002, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0x20000000, 0xA0000001,  0x50000001, 0x40000001,
  // .acts
  // k1
  0x00000003, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xA0000001, 0xA0000002,  0x40000002, 0x50000002,  0x40000001, 0x50000001,
  // .acts
  // k2
  0x00000005, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xC0000001, 0xA0000004,  0xA0000002, 0xA0000003,  0x40000004, 0x40000003,  0x50000002, 0x50000004,
  0x40000002, 0x50000003,
  // .acts
  // k3
  0x00000007, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xC0000001, 0xA0000006,  0xC0000002, 0xA0000005,  0xA0000003, 0xA0000004,  0x40000006, 0x40000004,
  0x50000003, 0x40000005,  0x50000004, 0x50000006,  0x40000003, 0x50000005,
  // .acts
  // k4
  0x00000009, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xC0000001, 0xA0000008,  0xC0000002, 0xA0000007,  0xC0000003, 0xA0000006,  0xA0000004, 0xA0000005,
  0x40000008, 0x40000005,  0x50000004, 0x40000006,  0x50000005, 0x40000007,  0x50000006, 0x50000008,
  0x40000004, 0x50000007,
  // .acts
  // k5
  0x0000000B, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xC0000001, 0xA000000A,  0xC0000002, 0xA0000009,  0xC0000003, 0xA0000008,  0xC0000004, 0xA0000007,
  0xA0000005, 0xA0000006,  0x4000000A, 0x40000006,  0x50000005, 0x40000007,  0x50000006, 0x40000008,
  0x50000007, 0x40000009,  0x50000008, 0x5000000A,  0x40000005, 0x50000009,
  // .acts
  // k6
  0x0000000D, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xC0000001, 0xA000000C,  0xC0000002, 0xA000000B,  0xC0000003, 0xA000000A,  0xC0000004, 0xA0000009,
  0xC0000005, 0xA0000008,  0xA0000006, 0xA0000007,  0x4000000C, 0x40000007,  0x50000006, 0x40000008,
  0x50000007, 0x40000009,  0x50000008, 0x4000000A,  0x50000009, 0x4000000B,  0x5000000A, 0x5000000C,
  0x40000006, 0x5000000B,
  // .acts
  // k7
  0x0000000F, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xC0000001, 0xA000000E,  0xC0000002, 0xA000000D,  0xC0000003, 0xA000000C,  0xC0000004, 0xA000000B,
  0xC0000005, 0xA000000A,  0xC0000006, 0xA0000009,  0xA0000007, 0xA0000008,  0x4000000E, 0x40000008,
  0x50000007, 0x40000009,  0x50000008, 0x4000000A,  0x50000009, 0x4000000B,  0x5000000A, 0x4000000C,
  0x5000000B, 0x4000000D,  0x5000000C, 0x5000000E,  0x40000007, 0x5000000D,
  // .acts
  // k8
  0x00000011, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xC0000001, 0xA0000010,  0xC0000002, 0xA000000F,  0xC0000003, 0xA000000E,  0xC0000004, 0xA000000D,
  0xC0000005, 0xA000000C,  0xC0000006, 0xA000000B,  0xC0000007, 0xA000000A,  0xA0000008, 0xA0000009,
  0x40000010, 0x40000009,  0x50000008, 0x4000000A,  0x50000009, 0x4000000B,  0x5000000A, 0x4000000C,
  0x5000000B, 0x4000000D,  0x5000000C, 0x4000000E,  0x5000000D, 0x4000000F,  0x5000000E, 0x50000010,
  0x40000008, 0x5000000F,
  // .acts
  // k9
  0x00000013, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xC0000001, 0xA0000012,  0xC0000002, 0xA0000011,  0xC0000003, 0xA0000010,  0xC0000004, 0xA000000F,
  0xC0000005, 0xA000000E,  0xC0000006, 0xA000000D,  0xC0000007, 0xA000000C,  0xC0000008, 0xA000000B,
  0xA0000009, 0xA000000A,  0x40000012, 0x4000000A,  0x50000009, 0x4000000B,  0x5000000A, 0x4000000C,
  0x5000000B, 0x4000000D,  0x5000000C, 0x4000000E,  0x5000000D, 0x4000000F,  0x5000000E, 0x40000010,
  0x5000000F, 0x40000011,  0x50000010, 0x50000012,  0x40000009, 0x50000011,
  // .acts
  // afS
  0x00000006, // .nlen
  0x00000003, // .alen
  0xA0000000, // .root
  // .node
  0xB0000001, 0x50000004,  0x40000005, 0x40000002,  0x50000001, 0x40000004,  0x50000005, 0xA0000004,
  0x50000002, 0x50000000,  0x40000001, 0x40000003,
  // .acts
  0xA0000002, 0x10000007,  0xA0000003, 0x1000001F,  0xA0000005, 0x10000007,
  // afZ
  0x00000000, // .nlen
  0x00000000, // .alen
  0x10000005, // .root
  // .node
  // .acts
  // and
  0x00000009, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xA0000001, 0x50000005,  0xA0000002, 0xA0000005,  0xA0000003, 0x50000004,  0x10000005, 0xA0000004,
  0x10000001, 0x50000002,  0xA0000006, 0x50000000,  0xA0000007, 0x50000008,  0x10000001, 0xA0000008,
  0x10000001, 0x50000006,
  // .acts
  // brn
  0x00000003, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xA0000001, 0x50000002,  0x10000050, 0xA0000002,  0x10000051, 0x50000000,
  // .acts
  // c10
  0x00000015, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xB0000001, 0xA0000014,  0xB0000002, 0xA0000013,  0xB0000003, 0xA0000012,  0xB0000004, 0xA0000011,
  0xB0000005, 0xA0000010,  0xB0000006, 0xA000000F,  0xB0000007, 0xA000000E,  0xB0000008, 0xA000000D,
  0xB0000009, 0xA000000C,  0xA000000A, 0xA000000B,  0x40000014, 0x4000000B,  0x5000000A, 0x4000000C,
  0x5000000B, 0x4000000D,  0x5000000C, 0x4000000E,  0x5000000D, 0x4000000F,  0x5000000E, 0x40000010,
  0x5000000F, 0x40000011,  0x50000010, 0x40000012,  0x50000011, 0x40000013,  0x50000012, 0x50000014,
  0x4000000A, 0x50000013,
  // .acts
  // c11
  0x00000017, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xB0000001, 0xA0000016,  0xB0000002, 0xA0000015,  0xB0000003, 0xA0000014,  0xB0000004, 0xA0000013,
  0xB0000005, 0xA0000012,  0xB0000006, 0xA0000011,  0xB0000007, 0xA0000010,  0xB0000008, 0xA000000F,
  0xB0000009, 0xA000000E,  0xB000000A, 0xA000000D,  0xA000000B, 0xA000000C,  0x40000016, 0x4000000C,
  0x5000000B, 0x4000000D,  0x5000000C, 0x4000000E,  0x5000000D, 0x4000000F,  0x5000000E, 0x40000010,
  0x5000000F, 0x40000011,  0x50000010, 0x40000012,  0x50000011, 0x40000013,  0x50000012, 0x40000014,
  0x50000013, 0x40000015,  0x50000014, 0x50000016,  0x4000000B, 0x50000015,
  // .acts
  // c12
  0x00000019, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xB0000001, 0xA0000018,  0xB0000002, 0xA0000017,  0xB0000003, 0xA0000016,  0xB0000004, 0xA0000015,
  0xB0000005, 0xA0000014,  0xB0000006, 0xA0000013,  0xB0000007, 0xA0000012,  0xB0000008, 0xA0000011,
  0xB0000009, 0xA0000010,  0xB000000A, 0xA000000F,  0xB000000B, 0xA000000E,  0xA000000C, 0xA000000D,
  0x40000018, 0x4000000D,  0x5000000C, 0x4000000E,  0x5000000D, 0x4000000F,  0x5000000E, 0x40000010,
  0x5000000F, 0x40000011,  0x50000010, 0x40000012,  0x50000011, 0x40000013,  0x50000012, 0x40000014,
  0x50000013, 0x40000015,  0x50000014, 0x40000016,  0x50000015, 0x40000017,  0x50000016, 0x50000018,
  0x4000000C, 0x50000017,
  // .acts
  // c13
  0x0000001B, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xB0000001, 0xA000001A,  0xB0000002, 0xA0000019,  0xB0000003, 0xA0000018,  0xB0000004, 0xA0000017,
  0xB0000005, 0xA0000016,  0xB0000006, 0xA0000015,  0xB0000007, 0xA0000014,  0xB0000008, 0xA0000013,
  0xB0000009, 0xA0000012,  0xB000000A, 0xA0000011,  0xB000000B, 0xA0000010,  0xB000000C, 0xA000000F,
  0xA000000D, 0xA000000E,  0x4000001A, 0x4000000E,  0x5000000D, 0x4000000F,  0x5000000E, 0x40000010,
  0x5000000F, 0x40000011,  0x50000010, 0x40000012,  0x50000011, 0x40000013,  0x50000012, 0x40000014,
  0x50000013, 0x40000015,  0x50000014, 0x40000016,  0x50000015, 0x40000017,  0x50000016, 0x40000018,
  0x50000017, 0x40000019,  0x50000018, 0x5000001A,  0x4000000D, 0x50000019,
  // .acts
  // c14
  0x0000001D, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xB0000001, 0xA000001C,  0xB0000002, 0xA000001B,  0xB0000003, 0xA000001A,  0xB0000004, 0xA0000019,
  0xB0000005, 0xA0000018,  0xB0000006, 0xA0000017,  0xB0000007, 0xA0000016,  0xB0000008, 0xA0000015,
  0xB0000009, 0xA0000014,  0xB000000A, 0xA0000013,  0xB000000B, 0xA0000012,  0xB000000C, 0xA0000011,
  0xB000000D, 0xA0000010,  0xA000000E, 0xA000000F,  0x4000001C, 0x4000000F,  0x5000000E, 0x40000010,
  0x5000000F, 0x40000011,  0x50000010, 0x40000012,  0x50000011, 0x40000013,  0x50000012, 0x40000014,
  0x50000013, 0x40000015,  0x50000014, 0x40000016,  0x50000015, 0x40000017,  0x50000016, 0x40000018,
  0x50000017, 0x40000019,  0x50000018, 0x4000001A,  0x50000019, 0x4000001B,  0x5000001A, 0x5000001C,
  0x4000000E, 0x5000001B,
  // .acts
  // c15
  0x0000001F, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xB0000001, 0xA000001E,  0xB0000002, 0xA000001D,  0xB0000003, 0xA000001C,  0xB0000004, 0xA000001B,
  0xB0000005, 0xA000001A,  0xB0000006, 0xA0000019,  0xB0000007, 0xA0000018,  0xB0000008, 0xA0000017,
  0xB0000009, 0xA0000016,  0xB000000A, 0xA0000015,  0xB000000B, 0xA0000014,  0xB000000C, 0xA0000013,
  0xB000000D, 0xA0000012,  0xB000000E, 0xA0000011,  0xA000000F, 0xA0000010,  0x4000001E, 0x40000010,
  0x5000000F, 0x40000011,  0x50000010, 0x40000012,  0x50000011, 0x40000013,  0x50000012, 0x40000014,
  0x50000013, 0x40000015,  0x50000014, 0x40000016,  0x50000015, 0x40000017,  0x50000016, 0x40000018,
  0x50000017, 0x40000019,  0x50000018, 0x4000001A,  0x50000019, 0x4000001B,  0x5000001A, 0x4000001C,
  0x5000001B, 0x4000001D,  0x5000001C, 0x5000001E,  0x4000000F, 0x5000001D,
  // .acts
  // c16
  0x00000021, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xB0000001, 0xA0000020,  0xB0000002, 0xA000001F,  0xB0000003, 0xA000001E,  0xB0000004, 0xA000001D,
  0xB0000005, 0xA000001C,  0xB0000006, 0xA000001B,  0xB0000007, 0xA000001A,  0xB0000008, 0xA0000019,
  0xB0000009, 0xA0000018,  0xB000000A, 0xA0000017,  0xB000000B, 0xA0000016,  0xB000000C, 0xA0000015,
  0xB000000D, 0xA0000014,  0xB000000E, 0xA0000013,  0xB000000F, 0xA0000012,  0xA0000010, 0xA0000011,
  0x00000000, 0x40000011,  0x50000010, 0x40000012,  0x50000011, 0x40000013,  0x50000012, 0x40000014,
  0x50000013, 0x40000015,  0x50000014, 0x40000016,  0x50000015, 0x40000017,  0x50000016, 0x40000018,
  0x50000017, 0x40000019,  0x50000018, 0x4000001A,  0x50000019, 0x4000001B,  0x5000001A, 0x4000001C,
  0x5000001B, 0x4000001D,  0x5000001C, 0x4000001E,  0x5000001D, 0x4000001F,  0x5000001E, 0x50000020,
  0x00000000, 0x5000001F,
  // .acts
  // c17
  0x00000023, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xB0000001, 0xA0000022,  0xB0000002, 0xA0000021,  0xB0000003, 0xA0000020,  0xB0000004, 0xA000001F,
  0xB0000005, 0xA000001E,  0xB0000006, 0xA000001D,  0xB0000007, 0xA000001C,  0xB0000008, 0xA000001B,
  0xB0000009, 0xA000001A,  0xB000000A, 0xA0000019,  0xB000000B, 0xA0000018,  0xB000000C, 0xA0000017,
  0xB000000D, 0xA0000016,  0xB000000E, 0xA0000015,  0xB000000F, 0xA0000014,  0xB0000010, 0xA0000013,
  0xA0000011, 0xA0000012,  0x40000022, 0x40000012,  0x50000011, 0x40000013,  0x50000012, 0x40000014,
  0x50000013, 0x40000015,  0x50000014, 0x40000016,  0x50000015, 0x40000017,  0x50000016, 0x40000018,
  0x50000017, 0x40000019,  0x50000018, 0x4000001A,  0x50000019, 0x4000001B,  0x5000001A, 0x4000001C,
  0x5000001B, 0x4000001D,  0x5000001C, 0x4000001E,  0x5000001D, 0x4000001F,  0x5000001E, 0x40000020,
  0x5000001F, 0x40000021,  0x50000020, 0x50000022,  0x40000011, 0x50000021,
  // .acts
  // c18
  0x00000025, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xB0000001, 0xA0000024,  0xB0000002, 0xA0000023,  0xB0000003, 0xA0000022,  0xB0000004, 0xA0000021,
  0xB0000005, 0xA0000020,  0xB0000006, 0xA000001F,  0xB0000007, 0xA000001E,  0xB0000008, 0xA000001D,
  0xB0000009, 0xA000001C,  0xB000000A, 0xA000001B,  0xB000000B, 0xA000001A,  0xB000000C, 0xA0000019,
  0xB000000D, 0xA0000018,  0xB000000E, 0xA0000017,  0xB000000F, 0xA0000016,  0xB0000010, 0xA0000015,
  0xB0000011, 0xA0000014,  0xA0000012, 0xA0000013,  0x40000024, 0x40000013,  0x50000012, 0x40000014,
  0x50000013, 0x40000015,  0x50000014, 0x40000016,  0x50000015, 0x40000017,  0x50000016, 0x40000018,
  0x50000017, 0x40000019,  0x50000018, 0x4000001A,  0x50000019, 0x4000001B,  0x5000001A, 0x4000001C,
  0x5000001B, 0x4000001D,  0x5000001C, 0x4000001E,  0x5000001D, 0x4000001F,  0x5000001E, 0x40000020,
  0x5000001F, 0x40000021,  0x50000020, 0x40000022,  0x50000021, 0x40000023,  0x50000022, 0x50000024,
  0x40000012, 0x50000023,
  // .acts
  // c19
  0x00000027, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xB0000001, 0xA0000026,  0xB0000002, 0xA0000025,  0xB0000003, 0xA0000024,  0xB0000004, 0xA0000023,
  0xB0000005, 0xA0000022,  0xB0000006, 0xA0000021,  0xB0000007, 0xA0000020,  0xB0000008, 0xA000001F,
  0xB0000009, 0xA000001E,  0xB000000A, 0xA000001D,  0xB000000B, 0xA000001C,  0xB000000C, 0xA000001B,
  0xB000000D, 0xA000001A,  0xB000000E, 0xA0000019,  0xB000000F, 0xA0000018,  0xB0000010, 0xA0000017,
  0xB0000011, 0xA0000016,  0xB0000012, 0xA0000015,  0xA0000013, 0xA0000014,  0x40000026, 0x40000014,
  0x50000013, 0x40000015,  0x50000014, 0x40000016,  0x50000015, 0x40000017,  0x50000016, 0x40000018,
  0x50000017, 0x40000019,  0x50000018, 0x4000001A,  0x50000019, 0x4000001B,  0x5000001A, 0x4000001C,
  0x5000001B, 0x4000001D,  0x5000001C, 0x4000001E,  0x5000001D, 0x4000001F,  0x5000001E, 0x40000020,
  0x5000001F, 0x40000021,  0x50000020, 0x40000022,  0x50000021, 0x40000023,  0x50000022, 0x40000024,
  0x50000023, 0x40000025,  0x50000024, 0x50000026,  0x40000013, 0x50000025,
  // .acts
  // c20
  0x00000029, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xB0000001, 0xA0000028,  0xB0000002, 0xA0000027,  0xB0000003, 0xA0000026,  0xB0000004, 0xA0000025,
  0xB0000005, 0xA0000024,  0xB0000006, 0xA0000023,  0xB0000007, 0xA0000022,  0xB0000008, 0xA0000021,
  0xB0000009, 0xA0000020,  0xB000000A, 0xA000001F,  0xB000000B, 0xA000001E,  0xB000000C, 0xA000001D,
  0xB000000D, 0xA000001C,  0xB000000E, 0xA000001B,  0xB000000F, 0xA000001A,  0xB0000010, 0xA0000019,
  0xB0000011, 0xA0000018,  0xB0000012, 0xA0000017,  0xB0000013, 0xA0000016,  0xA0000014, 0xA0000015,
  0x40000028, 0x40000015,  0x50000014, 0x40000016,  0x50000015, 0x40000017,  0x50000016, 0x40000018,
  0x50000017, 0x40000019,  0x50000018, 0x4000001A,  0x50000019, 0x4000001B,  0x5000001A, 0x4000001C,
  0x5000001B, 0x4000001D,  0x5000001C, 0x4000001E,  0x5000001D, 0x4000001F,  0x5000001E, 0x40000020,
  0x5000001F, 0x40000021,  0x50000020, 0x40000022,  0x50000021, 0x40000023,  0x50000022, 0x40000024,
  0x50000023, 0x40000025,  0x50000024, 0x40000026,  0x50000025, 0x40000027,  0x50000026, 0x50000028,
  0x40000014, 0x50000027,
  // .acts
  // c21
  0x0000002B, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xB0000001, 0xA000002A,  0xB0000002, 0xA0000029,  0xB0000003, 0xA0000028,  0xB0000004, 0xA0000027,
  0xB0000005, 0xA0000026,  0xB0000006, 0xA0000025,  0xB0000007, 0xA0000024,  0xB0000008, 0xA0000023,
  0xB0000009, 0xA0000022,  0xB000000A, 0xA0000021,  0xB000000B, 0xA0000020,  0xB000000C, 0xA000001F,
  0xB000000D, 0xA000001E,  0xB000000E, 0xA000001D,  0xB000000F, 0xA000001C,  0xB0000010, 0xA000001B,
  0xB0000011, 0xA000001A,  0xB0000012, 0xA0000019,  0xB0000013, 0xA0000018,  0xB0000014, 0xA0000017,
  0xA0000015, 0xA0000016,  0x4000002A, 0x40000016,  0x50000015, 0x40000017,  0x50000016, 0x40000018,
  0x50000017, 0x40000019,  0x50000018, 0x4000001A,  0x50000019, 0x4000001B,  0x5000001A, 0x4000001C,
  0x5000001B, 0x4000001D,  0x5000001C, 0x4000001E,  0x5000001D, 0x4000001F,  0x5000001E, 0x40000020,
  0x5000001F, 0x40000021,  0x50000020, 0x40000022,  0x50000021, 0x40000023,  0x50000022, 0x40000024,
  0x50000023, 0x40000025,  0x50000024, 0x40000026,  0x50000025, 0x40000027,  0x50000026, 0x40000028,
  0x50000027, 0x40000029,  0x50000028, 0x5000002A,  0x40000015, 0x50000029,
  // .acts
  // c22
  0x0000002D, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xB0000001, 0xA000002C,  0xB0000002, 0xA000002B,  0xB0000003, 0xA000002A,  0xB0000004, 0xA0000029,
  0xB0000005, 0xA0000028,  0xB0000006, 0xA0000027,  0xB0000007, 0xA0000026,  0xB0000008, 0xA0000025,
  0xB0000009, 0xA0000024,  0xB000000A, 0xA0000023,  0xB000000B, 0xA0000022,  0xB000000C, 0xA0000021,
  0xB000000D, 0xA0000020,  0xB000000E, 0xA000001F,  0xB000000F, 0xA000001E,  0xB0000010, 0xA000001D,
  0xB0000011, 0xA000001C,  0xB0000012, 0xA000001B,  0xB0000013, 0xA000001A,  0xB0000014, 0xA0000019,
  0xB0000015, 0xA0000018,  0xA0000016, 0xA0000017,  0x4000002C, 0x40000017,  0x50000016, 0x40000018,
  0x50000017, 0x40000019,  0x50000018, 0x4000001A,  0x50000019, 0x4000001B,  0x5000001A, 0x4000001C,
  0x5000001B, 0x4000001D,  0x5000001C, 0x4000001E,  0x5000001D, 0x4000001F,  0x5000001E, 0x40000020,
  0x5000001F, 0x40000021,  0x50000020, 0x40000022,  0x50000021, 0x40000023,  0x50000022, 0x40000024,
  0x50000023, 0x40000025,  0x50000024, 0x40000026,  0x50000025, 0x40000027,  0x50000026, 0x40000028,
  0x50000027, 0x40000029,  0x50000028, 0x4000002A,  0x50000029, 0x4000002B,  0x5000002A, 0x5000002C,
  0x40000016, 0x5000002B,
  // .acts
  // c23
  0x0000002F, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xB0000001, 0xA000002E,  0xB0000002, 0xA000002D,  0xB0000003, 0xA000002C,  0xB0000004, 0xA000002B,
  0xB0000005, 0xA000002A,  0xB0000006, 0xA0000029,  0xB0000007, 0xA0000028,  0xB0000008, 0xA0000027,
  0xB0000009, 0xA0000026,  0xB000000A, 0xA0000025,  0xB000000B, 0xA0000024,  0xB000000C, 0xA0000023,
  0xB000000D, 0xA0000022,  0xB000000E, 0xA0000021,  0xB000000F, 0xA0000020,  0xB0000010, 0xA000001F,
  0xB0000011, 0xA000001E,  0xB0000012, 0xA000001D,  0xB0000013, 0xA000001C,  0xB0000014, 0xA000001B,
  0xB0000015, 0xA000001A,  0xB0000016, 0xA0000019,  0xA0000017, 0xA0000018,  0x4000002E, 0x40000018,
  0x50000017, 0x40000019,  0x50000018, 0x4000001A,  0x50000019, 0x4000001B,  0x5000001A, 0x4000001C,
  0x5000001B, 0x4000001D,  0x5000001C, 0x4000001E,  0x5000001D, 0x4000001F,  0x5000001E, 0x40000020,
  0x5000001F, 0x40000021,  0x50000020, 0x40000022,  0x50000021, 0x40000023,  0x50000022, 0x40000024,
  0x50000023, 0x40000025,  0x50000024, 0x40000026,  0x50000025, 0x40000027,  0x50000026, 0x40000028,
  0x50000027, 0x40000029,  0x50000028, 0x4000002A,  0x50000029, 0x4000002B,  0x5000002A, 0x4000002C,
  0x5000002B, 0x4000002D,  0x5000002C, 0x5000002E,  0x40000017, 0x5000002D,
  // .acts
  // c24
  0x00000031, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xB0000001, 0xA0000030,  0xB0000002, 0xA000002F,  0xB0000003, 0xA000002E,  0xB0000004, 0xA000002D,
  0xB0000005, 0xA000002C,  0xB0000006, 0xA000002B,  0xB0000007, 0xA000002A,  0xB0000008, 0xA0000029,
  0xB0000009, 0xA0000028,  0xB000000A, 0xA0000027,  0xB000000B, 0xA0000026,  0xB000000C, 0xA0000025,
  0xB000000D, 0xA0000024,  0xB000000E, 0xA0000023,  0xB000000F, 0xA0000022,  0xB0000010, 0xA0000021,
  0xB0000011, 0xA0000020,  0xB0000012, 0xA000001F,  0xB0000013, 0xA000001E,  0xB0000014, 0xA000001D,
  0xB0000015, 0xA000001C,  0xB0000016, 0xA000001B,  0xB0000017, 0xA000001A,  0xA0000018, 0xA0000019,
  0x40000030, 0x40000019,  0x50000018, 0x4000001A,  0x50000019, 0x4000001B,  0x5000001A, 0x4000001C,
  0x5000001B, 0x4000001D,  0x5000001C, 0x4000001E,  0x5000001D, 0x4000001F,  0x5000001E, 0x40000020,
  0x5000001F, 0x40000021,  0x50000020, 0x40000022,  0x50000021, 0x40000023,  0x50000022, 0x40000024,
  0x50000023, 0x40000025,  0x50000024, 0x40000026,  0x50000025, 0x40000027,  0x50000026, 0x40000028,
  0x50000027, 0x40000029,  0x50000028, 0x4000002A,  0x50000029, 0x4000002B,  0x5000002A, 0x4000002C,
  0x5000002B, 0x4000002D,  0x5000002C, 0x4000002E,  0x5000002D, 0x4000002F,  0x5000002E, 0x50000030,
  0x40000018, 0x5000002F,
  // .acts
  // c25
  0x00000033, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xB0000001, 0xA0000032,  0xB0000002, 0xA0000031,  0xB0000003, 0xA0000030,  0xB0000004, 0xA000002F,
  0xB0000005, 0xA000002E,  0xB0000006, 0xA000002D,  0xB0000007, 0xA000002C,  0xB0000008, 0xA000002B,
  0xB0000009, 0xA000002A,  0xB000000A, 0xA0000029,  0xB000000B, 0xA0000028,  0xB000000C, 0xA0000027,
  0xB000000D, 0xA0000026,  0xB000000E, 0xA0000025,  0xB000000F, 0xA0000024,  0xB0000010, 0xA0000023,
  0xB0000011, 0xA0000022,  0xB0000012, 0xA0000021,  0xB0000013, 0xA0000020,  0xB0000014, 0xA000001F,
  0xB0000015, 0xA000001E,  0xB0000016, 0xA000001D,  0xB0000017, 0xA000001C,  0xB0000018, 0xA000001B,
  0xA0000019, 0xA000001A,  0x00000000, 0x40000032,  0x50000019, 0x4000001B,  0x5000001A, 0x4000001C,
  0x5000001B, 0x4000001D,  0x5000001C, 0x4000001E,  0x5000001D, 0x4000001F,  0x5000001E, 0x40000020,
  0x5000001F, 0x40000021,  0x50000020, 0x40000022,  0x50000021, 0x40000023,  0x50000022, 0x40000024,
  0x50000023, 0x40000025,  0x50000024, 0x40000026,  0x50000025, 0x40000027,  0x50000026, 0x40000028,
  0x50000027, 0x40000029,  0x50000028, 0x4000002A,  0x50000029, 0x4000002B,  0x5000002A, 0x4000002C,
  0x5000002B, 0x4000002D,  0x5000002C, 0x4000002E,  0x5000002D, 0x4000002F,  0x5000002E, 0x40000030,
  0x5000002F, 0x40000031,  0x50000030, 0x50000032,  0x50000019, 0x50000031,
  // .acts
  // c26
  0x00000035, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xB0000001, 0xA0000034,  0xB0000002, 0xA0000033,  0xB0000003, 0xA0000032,  0xB0000004, 0xA0000031,
  0xB0000005, 0xA0000030,  0xB0000006, 0xA000002F,  0xB0000007, 0xA000002E,  0xB0000008, 0xA000002D,
  0xB0000009, 0xA000002C,  0xB000000A, 0xA000002B,  0xB000000B, 0xA000002A,  0xB000000C, 0xA0000029,
  0xB000000D, 0xA0000028,  0xB000000E, 0xA0000027,  0xB000000F, 0xA0000026,  0xB0000010, 0xA0000025,
  0xB0000011, 0xA0000024,  0xB0000012, 0xA0000023,  0xB0000013, 0xA0000022,  0xB0000014, 0xA0000021,
  0xB0000015, 0xA0000020,  0xB0000016, 0xA000001F,  0xB0000017, 0xA000001E,  0xB0000018, 0xA000001D,
  0xB0000019, 0xA000001C,  0xA000001A, 0xA000001B,  0x00000000, 0x4000001B,  0x5000001A, 0x40000034,
  0x5000001B, 0x4000001D,  0x5000001C, 0x4000001E,  0x5000001D, 0x4000001F,  0x5000001E, 0x40000020,
  0x5000001F, 0x40000021,  0x50000020, 0x40000022,  0x50000021, 0x40000023,  0x50000022, 0x40000024,
  0x50000023, 0x40000025,  0x50000024, 0x40000026,  0x50000025, 0x40000027,  0x50000026, 0x40000028,
  0x50000027, 0x40000029,  0x50000028, 0x4000002A,  0x50000029, 0x4000002B,  0x5000002A, 0x4000002C,
  0x5000002B, 0x4000002D,  0x5000002C, 0x4000002E,  0x5000002D, 0x4000002F,  0x5000002E, 0x40000030,
  0x5000002F, 0x40000031,  0x50000030, 0x40000032,  0x50000031, 0x40000033,  0x50000032, 0x50000034,
  0x5000001B, 0x50000033,
  // .acts
  // c_s
  0x00000007, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xA0000001, 0xA0000003,  0x50000004, 0xA0000002,  0x40000006, 0x40000005,  0xB0000004, 0xA0000006,
  0xA0000005, 0x40000001,  0x50000002, 0x50000006,  0x40000002, 0x50000005,
  // .acts
  // c_z
  0x00000002, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0x20000000, 0xA0000001,  0x50000001, 0x40000001,
  // .acts
  // dec
  0x00000004, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xA0000001, 0x50000003,  0x10000053, 0xA0000002,  0x10000052, 0xA0000003,  0x10000000, 0x50000000,
  // .acts
  // ex0
  0x00000001, // .nlen
  0x00000001, // .alen
  0x50000000, // .root
  // .node
  0x10000015, 0x30000000,
  // .acts
  0x1000000A, 0xA0000000,
  // ex1
  0x00000002, // .nlen
  0x00000001, // .alen
  0x50000001, // .root
  // .node
  0x1000003A, 0xA0000001,  0x1000003B, 0x30000000,
  // .acts
  0x1000002F, 0xA0000000,
  // ex2
  0x00000003, // .nlen
  0x00000002, // .alen
  0x50000002, // .root
  // .node
  0x10000002, 0xA0000001,  0x10000000, 0x40000002,  0x50000001, 0x30000000,
  // .acts
  0x10000021, 0xA0000000,  0x1000004F, 0xA0000002,
  // ex3
  0x00000003, // .nlen
  0x00000002, // .alen
  0x50000002, // .root
  // .node
  0x10000004, 0xA0000001,  0x10000006, 0x40000002,  0x50000001, 0x30000000,
  // .acts
  0x10000028, 0xA0000000,  0x10000020, 0xA0000002,
  // ex4
  0x00000003, // .nlen
  0x00000002, // .alen
  0x50000000, // .root
  // .node
  0x50000002, 0x30000000,  0x10000004, 0xA0000002,  0x10000006, 0x40000000,
  // .acts
  0xA0000000, 0x10000007,  0xA0000001, 0x10000009,
  // g_s
  0x00000005, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xC0000001, 0xA0000002,  0x40000003, 0x40000004,  0xA0000003, 0x50000004,  0x40000001, 0xA0000004,
  0x50000001, 0x50000002,
  // .acts
  // g_z
  0x00000001, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0x50000000, 0x40000000,
  // .acts
  // k10
  0x00000015, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xC0000001, 0xA0000014,  0xC0000002, 0xA0000013,  0xC0000003, 0xA0000012,  0xC0000004, 0xA0000011,
  0xC0000005, 0xA0000010,  0xC0000006, 0xA000000F,  0xC0000007, 0xA000000E,  0xC0000008, 0xA000000D,
  0xC0000009, 0xA000000C,  0xA000000A, 0xA000000B,  0x40000014, 0x4000000B,  0x5000000A, 0x4000000C,
  0x5000000B, 0x4000000D,  0x5000000C, 0x4000000E,  0x5000000D, 0x4000000F,  0x5000000E, 0x40000010,
  0x5000000F, 0x40000011,  0x50000010, 0x40000012,  0x50000011, 0x40000013,  0x50000012, 0x50000014,
  0x4000000A, 0x50000013,
  // .acts
  // k11
  0x00000017, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xC0000001, 0xA0000016,  0xC0000002, 0xA0000015,  0xC0000003, 0xA0000014,  0xC0000004, 0xA0000013,
  0xC0000005, 0xA0000012,  0xC0000006, 0xA0000011,  0xC0000007, 0xA0000010,  0xC0000008, 0xA000000F,
  0xC0000009, 0xA000000E,  0xC000000A, 0xA000000D,  0xA000000B, 0xA000000C,  0x40000016, 0x4000000C,
  0x5000000B, 0x4000000D,  0x5000000C, 0x4000000E,  0x5000000D, 0x4000000F,  0x5000000E, 0x40000010,
  0x5000000F, 0x40000011,  0x50000010, 0x40000012,  0x50000011, 0x40000013,  0x50000012, 0x40000014,
  0x50000013, 0x40000015,  0x50000014, 0x50000016,  0x4000000B, 0x50000015,
  // .acts
  // k12
  0x00000019, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xC0000001, 0xA0000018,  0xC0000002, 0xA0000017,  0xC0000003, 0xA0000016,  0xC0000004, 0xA0000015,
  0xC0000005, 0xA0000014,  0xC0000006, 0xA0000013,  0xC0000007, 0xA0000012,  0xC0000008, 0xA0000011,
  0xC0000009, 0xA0000010,  0xC000000A, 0xA000000F,  0xC000000B, 0xA000000E,  0xA000000C, 0xA000000D,
  0x40000018, 0x4000000D,  0x5000000C, 0x4000000E,  0x5000000D, 0x4000000F,  0x5000000E, 0x40000010,
  0x5000000F, 0x40000011,  0x50000010, 0x40000012,  0x50000011, 0x40000013,  0x50000012, 0x40000014,
  0x50000013, 0x40000015,  0x50000014, 0x40000016,  0x50000015, 0x40000017,  0x50000016, 0x50000018,
  0x4000000C, 0x50000017,
  // .acts
  // k13
  0x0000001B, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xC0000001, 0xA000001A,  0xC0000002, 0xA0000019,  0xC0000003, 0xA0000018,  0xC0000004, 0xA0000017,
  0xC0000005, 0xA0000016,  0xC0000006, 0xA0000015,  0xC0000007, 0xA0000014,  0xC0000008, 0xA0000013,
  0xC0000009, 0xA0000012,  0xC000000A, 0xA0000011,  0xC000000B, 0xA0000010,  0xC000000C, 0xA000000F,
  0xA000000D, 0xA000000E,  0x4000001A, 0x4000000E,  0x5000000D, 0x4000000F,  0x5000000E, 0x40000010,
  0x5000000F, 0x40000011,  0x50000010, 0x40000012,  0x50000011, 0x40000013,  0x50000012, 0x40000014,
  0x50000013, 0x40000015,  0x50000014, 0x40000016,  0x50000015, 0x40000017,  0x50000016, 0x40000018,
  0x50000017, 0x40000019,  0x50000018, 0x5000001A,  0x4000000D, 0x50000019,
  // .acts
  // k14
  0x0000001D, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xC0000001, 0xA000001C,  0xC0000002, 0xA000001B,  0xC0000003, 0xA000001A,  0xC0000004, 0xA0000019,
  0xC0000005, 0xA0000018,  0xC0000006, 0xA0000017,  0xC0000007, 0xA0000016,  0xC0000008, 0xA0000015,
  0xC0000009, 0xA0000014,  0xC000000A, 0xA0000013,  0xC000000B, 0xA0000012,  0xC000000C, 0xA0000011,
  0xC000000D, 0xA0000010,  0xA000000E, 0xA000000F,  0x4000001C, 0x4000000F,  0x5000000E, 0x40000010,
  0x5000000F, 0x40000011,  0x50000010, 0x40000012,  0x50000011, 0x40000013,  0x50000012, 0x40000014,
  0x50000013, 0x40000015,  0x50000014, 0x40000016,  0x50000015, 0x40000017,  0x50000016, 0x40000018,
  0x50000017, 0x40000019,  0x50000018, 0x4000001A,  0x50000019, 0x4000001B,  0x5000001A, 0x5000001C,
  0x4000000E, 0x5000001B,
  // .acts
  // k15
  0x0000001F, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xC0000001, 0xA000001E,  0xC0000002, 0xA000001D,  0xC0000003, 0xA000001C,  0xC0000004, 0xA000001B,
  0xC0000005, 0xA000001A,  0xC0000006, 0xA0000019,  0xC0000007, 0xA0000018,  0xC0000008, 0xA0000017,
  0xC0000009, 0xA0000016,  0xC000000A, 0xA0000015,  0xC000000B, 0xA0000014,  0xC000000C, 0xA0000013,
  0xC000000D, 0xA0000012,  0xC000000E, 0xA0000011,  0xA000000F, 0xA0000010,  0x4000001E, 0x40000010,
  0x5000000F, 0x40000011,  0x50000010, 0x40000012,  0x50000011, 0x40000013,  0x50000012, 0x40000014,
  0x50000013, 0x40000015,  0x50000014, 0x40000016,  0x50000015, 0x40000017,  0x50000016, 0x40000018,
  0x50000017, 0x40000019,  0x50000018, 0x4000001A,  0x50000019, 0x4000001B,  0x5000001A, 0x4000001C,
  0x5000001B, 0x4000001D,  0x5000001C, 0x5000001E,  0x4000000F, 0x5000001D,
  // .acts
  // k16
  0x00000021, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xC0000001, 0xA0000020,  0xC0000002, 0xA000001F,  0xC0000003, 0xA000001E,  0xC0000004, 0xA000001D,
  0xC0000005, 0xA000001C,  0xC0000006, 0xA000001B,  0xC0000007, 0xA000001A,  0xC0000008, 0xA0000019,
  0xC0000009, 0xA0000018,  0xC000000A, 0xA0000017,  0xC000000B, 0xA0000016,  0xC000000C, 0xA0000015,
  0xC000000D, 0xA0000014,  0xC000000E, 0xA0000013,  0xC000000F, 0xA0000012,  0xA0000010, 0xA0000011,
  0x40000020, 0x40000011,  0x50000010, 0x40000012,  0x50000011, 0x40000013,  0x50000012, 0x40000014,
  0x50000013, 0x40000015,  0x50000014, 0x40000016,  0x50000015, 0x40000017,  0x50000016, 0x40000018,
  0x50000017, 0x40000019,  0x50000018, 0x4000001A,  0x50000019, 0x4000001B,  0x5000001A, 0x4000001C,
  0x5000001B, 0x4000001D,  0x5000001C, 0x4000001E,  0x5000001D, 0x4000001F,  0x5000001E, 0x50000020,
  0x40000010, 0x5000001F,
  // .acts
  // k17
  0x00000023, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xC0000001, 0xA0000022,  0xC0000002, 0xA0000021,  0xC0000003, 0xA0000020,  0xC0000004, 0xA000001F,
  0xC0000005, 0xA000001E,  0xC0000006, 0xA000001D,  0xC0000007, 0xA000001C,  0xC0000008, 0xA000001B,
  0xC0000009, 0xA000001A,  0xC000000A, 0xA0000019,  0xC000000B, 0xA0000018,  0xC000000C, 0xA0000017,
  0xC000000D, 0xA0000016,  0xC000000E, 0xA0000015,  0xC000000F, 0xA0000014,  0xC0000010, 0xA0000013,
  0xA0000011, 0xA0000012,  0x40000022, 0x40000012,  0x50000011, 0x40000013,  0x50000012, 0x40000014,
  0x50000013, 0x40000015,  0x50000014, 0x40000016,  0x50000015, 0x40000017,  0x50000016, 0x40000018,
  0x50000017, 0x40000019,  0x50000018, 0x4000001A,  0x50000019, 0x4000001B,  0x5000001A, 0x4000001C,
  0x5000001B, 0x4000001D,  0x5000001C, 0x4000001E,  0x5000001D, 0x4000001F,  0x5000001E, 0x40000020,
  0x5000001F, 0x40000021,  0x50000020, 0x50000022,  0x40000011, 0x50000021,
  // .acts
  // k18
  0x00000025, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xC0000001, 0xA0000024,  0xC0000002, 0xA0000023,  0xC0000003, 0xA0000022,  0xC0000004, 0xA0000021,
  0xC0000005, 0xA0000020,  0xC0000006, 0xA000001F,  0xC0000007, 0xA000001E,  0xC0000008, 0xA000001D,
  0xC0000009, 0xA000001C,  0xC000000A, 0xA000001B,  0xC000000B, 0xA000001A,  0xC000000C, 0xA0000019,
  0xC000000D, 0xA0000018,  0xC000000E, 0xA0000017,  0xC000000F, 0xA0000016,  0xC0000010, 0xA0000015,
  0xC0000011, 0xA0000014,  0xA0000012, 0xA0000013,  0x40000024, 0x40000013,  0x50000012, 0x40000014,
  0x50000013, 0x40000015,  0x50000014, 0x40000016,  0x50000015, 0x40000017,  0x50000016, 0x40000018,
  0x50000017, 0x40000019,  0x50000018, 0x4000001A,  0x50000019, 0x4000001B,  0x5000001A, 0x4000001C,
  0x5000001B, 0x4000001D,  0x5000001C, 0x4000001E,  0x5000001D, 0x4000001F,  0x5000001E, 0x40000020,
  0x5000001F, 0x40000021,  0x50000020, 0x40000022,  0x50000021, 0x40000023,  0x50000022, 0x50000024,
  0x40000012, 0x50000023,
  // .acts
  // k19
  0x00000027, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xC0000001, 0xA0000026,  0xC0000002, 0xA0000025,  0xC0000003, 0xA0000024,  0xC0000004, 0xA0000023,
  0xC0000005, 0xA0000022,  0xC0000006, 0xA0000021,  0xC0000007, 0xA0000020,  0xC0000008, 0xA000001F,
  0xC0000009, 0xA000001E,  0xC000000A, 0xA000001D,  0xC000000B, 0xA000001C,  0xC000000C, 0xA000001B,
  0xC000000D, 0xA000001A,  0xC000000E, 0xA0000019,  0xC000000F, 0xA0000018,  0xC0000010, 0xA0000017,
  0xC0000011, 0xA0000016,  0xC0000012, 0xA0000015,  0xA0000013, 0xA0000014,  0x40000026, 0x40000014,
  0x50000013, 0x40000015,  0x50000014, 0x40000016,  0x50000015, 0x40000017,  0x50000016, 0x40000018,
  0x50000017, 0x40000019,  0x50000018, 0x4000001A,  0x50000019, 0x4000001B,  0x5000001A, 0x4000001C,
  0x5000001B, 0x4000001D,  0x5000001C, 0x4000001E,  0x5000001D, 0x4000001F,  0x5000001E, 0x40000020,
  0x5000001F, 0x40000021,  0x50000020, 0x40000022,  0x50000021, 0x40000023,  0x50000022, 0x40000024,
  0x50000023, 0x40000025,  0x50000024, 0x50000026,  0x40000013, 0x50000025,
  // .acts
  // k20
  0x00000029, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xC0000001, 0xA0000028,  0xC0000002, 0xA0000027,  0xC0000003, 0xA0000026,  0xC0000004, 0xA0000025,
  0xC0000005, 0xA0000024,  0xC0000006, 0xA0000023,  0xC0000007, 0xA0000022,  0xC0000008, 0xA0000021,
  0xC0000009, 0xA0000020,  0xC000000A, 0xA000001F,  0xC000000B, 0xA000001E,  0xC000000C, 0xA000001D,
  0xC000000D, 0xA000001C,  0xC000000E, 0xA000001B,  0xC000000F, 0xA000001A,  0xC0000010, 0xA0000019,
  0xC0000011, 0xA0000018,  0xC0000012, 0xA0000017,  0xC0000013, 0xA0000016,  0xA0000014, 0xA0000015,
  0x40000028, 0x40000015,  0x50000014, 0x40000016,  0x50000015, 0x40000017,  0x50000016, 0x40000018,
  0x50000017, 0x40000019,  0x50000018, 0x4000001A,  0x50000019, 0x4000001B,  0x5000001A, 0x4000001C,
  0x5000001B, 0x4000001D,  0x5000001C, 0x4000001E,  0x5000001D, 0x4000001F,  0x5000001E, 0x40000020,
  0x5000001F, 0x40000021,  0x50000020, 0x40000022,  0x50000021, 0x40000023,  0x50000022, 0x40000024,
  0x50000023, 0x40000025,  0x50000024, 0x40000026,  0x50000025, 0x40000027,  0x50000026, 0x50000028,
  0x40000014, 0x50000027,
  // .acts
  // k21
  0x0000002B, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xC0000001, 0xA000002A,  0xC0000002, 0xA0000029,  0xC0000003, 0xA0000028,  0xC0000004, 0xA0000027,
  0xC0000005, 0xA0000026,  0xC0000006, 0xA0000025,  0xC0000007, 0xA0000024,  0xC0000008, 0xA0000023,
  0xC0000009, 0xA0000022,  0xC000000A, 0xA0000021,  0xC000000B, 0xA0000020,  0xC000000C, 0xA000001F,
  0xC000000D, 0xA000001E,  0xC000000E, 0xA000001D,  0xC000000F, 0xA000001C,  0xC0000010, 0xA000001B,
  0xC0000011, 0xA000001A,  0xC0000012, 0xA0000019,  0xC0000013, 0xA0000018,  0xC0000014, 0xA0000017,
  0xA0000015, 0xA0000016,  0x4000002A, 0x40000016,  0x50000015, 0x40000017,  0x50000016, 0x40000018,
  0x50000017, 0x40000019,  0x50000018, 0x4000001A,  0x50000019, 0x4000001B,  0x5000001A, 0x4000001C,
  0x5000001B, 0x4000001D,  0x5000001C, 0x4000001E,  0x5000001D, 0x4000001F,  0x5000001E, 0x40000020,
  0x5000001F, 0x40000021,  0x50000020, 0x40000022,  0x50000021, 0x40000023,  0x50000022, 0x40000024,
  0x50000023, 0x40000025,  0x50000024, 0x40000026,  0x50000025, 0x40000027,  0x50000026, 0x40000028,
  0x50000027, 0x40000029,  0x50000028, 0x5000002A,  0x40000015, 0x50000029,
  // .acts
  // k22
  0x0000002D, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xC0000001, 0xA000002C,  0xC0000002, 0xA000002B,  0xC0000003, 0xA000002A,  0xC0000004, 0xA0000029,
  0xC0000005, 0xA0000028,  0xC0000006, 0xA0000027,  0xC0000007, 0xA0000026,  0xC0000008, 0xA0000025,
  0xC0000009, 0xA0000024,  0xC000000A, 0xA0000023,  0xC000000B, 0xA0000022,  0xC000000C, 0xA0000021,
  0xC000000D, 0xA0000020,  0xC000000E, 0xA000001F,  0xC000000F, 0xA000001E,  0xC0000010, 0xA000001D,
  0xC0000011, 0xA000001C,  0xC0000012, 0xA000001B,  0xC0000013, 0xA000001A,  0xC0000014, 0xA0000019,
  0xC0000015, 0xA0000018,  0xA0000016, 0xA0000017,  0x4000002C, 0x40000017,  0x50000016, 0x40000018,
  0x50000017, 0x40000019,  0x50000018, 0x4000001A,  0x50000019, 0x4000001B,  0x5000001A, 0x4000001C,
  0x5000001B, 0x4000001D,  0x5000001C, 0x4000001E,  0x5000001D, 0x4000001F,  0x5000001E, 0x40000020,
  0x5000001F, 0x40000021,  0x50000020, 0x40000022,  0x50000021, 0x40000023,  0x50000022, 0x40000024,
  0x50000023, 0x40000025,  0x50000024, 0x40000026,  0x50000025, 0x40000027,  0x50000026, 0x40000028,
  0x50000027, 0x40000029,  0x50000028, 0x4000002A,  0x50000029, 0x4000002B,  0x5000002A, 0x5000002C,
  0x40000016, 0x5000002B,
  // .acts
  // k23
  0x0000002F, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xC0000001, 0xA000002E,  0xC0000002, 0xA000002D,  0xC0000003, 0xA000002C,  0xC0000004, 0xA000002B,
  0xC0000005, 0xA000002A,  0xC0000006, 0xA0000029,  0xC0000007, 0xA0000028,  0xC0000008, 0xA0000027,
  0xC0000009, 0xA0000026,  0xC000000A, 0xA0000025,  0xC000000B, 0xA0000024,  0xC000000C, 0xA0000023,
  0xC000000D, 0xA0000022,  0xC000000E, 0xA0000021,  0xC000000F, 0xA0000020,  0xC0000010, 0xA000001F,
  0xC0000011, 0xA000001E,  0xC0000012, 0xA000001D,  0xC0000013, 0xA000001C,  0xC0000014, 0xA000001B,
  0xC0000015, 0xA000001A,  0xC0000016, 0xA0000019,  0xA0000017, 0xA0000018,  0x4000002E, 0x40000018,
  0x50000017, 0x40000019,  0x50000018, 0x4000001A,  0x50000019, 0x4000001B,  0x5000001A, 0x4000001C,
  0x5000001B, 0x4000001D,  0x5000001C, 0x4000001E,  0x5000001D, 0x4000001F,  0x5000001E, 0x40000020,
  0x5000001F, 0x40000021,  0x50000020, 0x40000022,  0x50000021, 0x40000023,  0x50000022, 0x40000024,
  0x50000023, 0x40000025,  0x50000024, 0x40000026,  0x50000025, 0x40000027,  0x50000026, 0x40000028,
  0x50000027, 0x40000029,  0x50000028, 0x4000002A,  0x50000029, 0x4000002B,  0x5000002A, 0x4000002C,
  0x5000002B, 0x4000002D,  0x5000002C, 0x5000002E,  0x40000017, 0x5000002D,
  // .acts
  // k24
  0x00000031, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xC0000001, 0xA0000030,  0xC0000002, 0xA000002F,  0xC0000003, 0xA000002E,  0xC0000004, 0xA000002D,
  0xC0000005, 0xA000002C,  0xC0000006, 0xA000002B,  0xC0000007, 0xA000002A,  0xC0000008, 0xA0000029,
  0xC0000009, 0xA0000028,  0xC000000A, 0xA0000027,  0xC000000B, 0xA0000026,  0xC000000C, 0xA0000025,
  0xC000000D, 0xA0000024,  0xC000000E, 0xA0000023,  0xC000000F, 0xA0000022,  0xC0000010, 0xA0000021,
  0xC0000011, 0xA0000020,  0xC0000012, 0xA000001F,  0xC0000013, 0xA000001E,  0xC0000014, 0xA000001D,
  0xC0000015, 0xA000001C,  0xC0000016, 0xA000001B,  0xC0000017, 0xA000001A,  0xA0000018, 0xA0000019,
  0x40000030, 0x40000019,  0x50000018, 0x4000001A,  0x50000019, 0x4000001B,  0x5000001A, 0x4000001C,
  0x5000001B, 0x4000001D,  0x5000001C, 0x4000001E,  0x5000001D, 0x4000001F,  0x5000001E, 0x40000020,
  0x5000001F, 0x40000021,  0x50000020, 0x40000022,  0x50000021, 0x40000023,  0x50000022, 0x40000024,
  0x50000023, 0x40000025,  0x50000024, 0x40000026,  0x50000025, 0x40000027,  0x50000026, 0x40000028,
  0x50000027, 0x40000029,  0x50000028, 0x4000002A,  0x50000029, 0x4000002B,  0x5000002A, 0x4000002C,
  0x5000002B, 0x4000002D,  0x5000002C, 0x4000002E,  0x5000002D, 0x4000002F,  0x5000002E, 0x50000030,
  0x40000018, 0x5000002F,
  // .acts
  // low
  0x00000004, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xA0000001, 0x50000003,  0x10000055, 0xA0000002,  0x10000054, 0xA0000003,  0x10000000, 0x50000000,
  // .acts
  // mul
  0x00000005, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xA0000001, 0xA0000002,  0x50000003, 0x50000004,  0xA0000003, 0xA0000004,  0x40000004, 0x40000001,
  0x40000003, 0x50000001,
  // .acts
  // nid
  0x00000003, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xA0000001, 0x50000002,  0x10000056, 0xA0000002,  0x10000006, 0x50000000,
  // .acts
  // not
  0x00000005, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xA0000001, 0xA0000003,  0x40000004, 0xA0000002,  0x40000003, 0x50000004,  0x40000002, 0xA0000004,
  0x40000001, 0x50000002,
  // .acts
  // run
  0x00000004, // .nlen
  0x00000000, // .alen
  0xA0000000, // .root
  // .node
  0xA0000001, 0x50000003,  0x10000058, 0xA0000002,  0x10000057, 0xA0000003,  0x10000000, 0x50000000,
  // .acts
  // brnS
  0x00000005, // .nlen
  0x00000002, // .alen
  0xA0000000, // .root
  // .node
  0xB0000001, 0xA0000002,  0x40000003, 0x40000004,  0x50000003, 0x50000004,  0x40000001, 0x40000002,
  0x50000001, 0x50000002,
  // .acts
  0x10000020, 0xA0000003,  0x10000020, 0xA0000004,
  // brnZ
  0x00000005, // .nlen
  0x00000002, // .alen
  0x50000000, // .root
  // .node
  0x50000004, 0x30000000,  0x1000000A, 0xA0000002,  0x1000000D, 0xA0000003,  0x10000002, 0xA0000004,
  0x10000000, 0x40000000,
  // .acts
  0x1000004F, 0xA0000000,  0x1000004C, 0xA0000001,
  // decI
  0x00000002, // .nlen
  0x00000001, // .alen
  0xA0000000, // .root
  // .node
  0x40000001, 0x50000001,  0x40000000, 0x50000000,
  // .acts
  0x1000004B, 0xA0000001,
  // decO
  0x00000003, // .nlen
  0x00000002, // .alen
  0xA0000000, // .root
  // .node
  0x40000002, 0x50000001,  0x50000002, 0x50000000,  0x40000000, 0x40000001,
  // .acts
  0x10000002, 0xA0000001,  0x10000034, 0xA0000002,
  // lowI
  0x00000003, // .nlen
  0x00000002, // .alen
  0xA0000000, // .root
  // .node
  0x40000001, 0x50000002,  0x40000000, 0x40000002,  0x50000001, 0x50000000,
  // .acts
  0x10000002, 0xA0000001,  0x10000003, 0xA0000002,
  // lowO
  0x00000003, // .nlen
  0x00000002, // .alen
  0xA0000000, // .root
  // .node
  0x40000001, 0x50000002,  0x40000000, 0x40000002,  0x50000001, 0x50000000,
  // .acts
  0x10000003, 0xA0000001,  0x10000003, 0xA0000002,
  // nidS
  0x00000003, // .nlen
  0x00000002, // .alen
  0xA0000000, // .root
  // .node
  0x40000002, 0x50000001,  0x50000002, 0x50000000,  0x40000000, 0x40000001,
  // .acts
  0x10000004, 0xA0000001,  0x1000004D, 0xA0000002,
  // runI
  0x00000004, // .nlen
  0x00000003, // .alen
  0xA0000000, // .root
  // .node
  0x40000003, 0x50000001,  0x50000002, 0x50000000,  0x50000003, 0x40000001,  0x40000000, 0x40000002,
  // .acts
  0x1000004F, 0xA0000001,  0x10000034, 0xA0000002,  0x10000002, 0xA0000003,
  // runO
  0x00000004, // .nlen
  0x00000003, // .alen
  0xA0000000, // .root
  // .node
  0x40000003, 0x50000001,  0x50000002, 0x50000000,  0x50000003, 0x40000001,  0x40000000, 0x40000002,
  // .acts
  0x1000004F, 0xA0000001,  0x10000034, 0xA0000002,  0x10000003, 0xA0000003,
};

const size_t BOOK_SIZE = sizeof(BOOK_DATA) / sizeof(u32);

// Main
// ----

int main() {
  // Prints device info
  int device;
  cudaDeviceProp prop;
  cudaGetDevice(&device);
  cudaGetDeviceProperties(&prop, device);
  printf("CUDA Device: %s, Compute Capability: %d.%d\n\n", prop.name, prop.major, prop.minor);

  // Allocates net on CPU
  Net* cpu_net = mknet(F_ex3);

  // Prints the input net
  printf("\nINPUT\n=====\n\n");
  print_net(cpu_net);

  // Uploads net and book to GPU
  Net* gpu_net = net_to_gpu(cpu_net);
  Book* gpu_book = init_book_on_gpu(BOOK_DATA, BOOK_SIZE);

  // Marks init time
  struct timespec start, end;
  clock_gettime(CLOCK_MONOTONIC_RAW, &start);

  // Normalizes
  do_global_expand(gpu_net, gpu_book);
  for (u32 tick = 0; tick < 128; ++tick) {
    do_global_rewrite(gpu_net, gpu_book, 16, tick, (tick / GROUP_LOG2) % 2);
  }
  do_global_expand(gpu_net, gpu_book);
  do_global_rewrite(gpu_net, gpu_book, 200000, 0, 0);
  cudaDeviceSynchronize();

  // Gets end time
  clock_gettime(CLOCK_MONOTONIC_RAW, &end);
  uint32_t delta_time = (end.tv_sec - start.tv_sec) * 1000 + (end.tv_nsec - start.tv_nsec) / 1000000;

  // Reads result back to cpu
  Net* norm = net_to_cpu(gpu_net);

  // Prints the output
  printf("\nNORMAL ~ rewrites=%llu\n======\n\n", norm->rwts);
  //print_tree(norm, norm->root);
  print_net(norm);
  printf("Time: %.3f s\n", ((double)delta_time) / 1000.0);
  printf("RPS : %.3f million\n", ((double)norm->rwts) / ((double)delta_time) / 1000.0);

  // Clears CPU memory
  net_free_on_gpu(gpu_net);
  book_free_on_gpu(gpu_book);

  // Clears GPU memory
  net_free_on_cpu(cpu_net);
  net_free_on_cpu(norm);

  return 0;
}
