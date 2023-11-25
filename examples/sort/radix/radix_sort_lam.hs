import Prelude hiding (concat)
import Data.Word
import Data.Bits (shiftL, (.&.), (.|.))

newtype Arr = Arr { matchArr :: forall r. r -> (Word32 -> r) -> (Arr -> Arr -> r) -> r }
newtype Map = Map { matchApp :: forall r. r -> r -> (Map -> Map -> r) -> r }

empty :: Arr
empty = Arr $ \empty single concat -> empty

single :: Word32 -> Arr
single x = Arr $ \empty single concat -> single x

concat :: Arr -> Arr -> Arr
concat x0 x1 = Arr $ \empty single concat -> concat x0 x1

free :: Map
free = Map $ \free used node -> free

used :: Map
used = Map $ \free used node -> used

node :: Map -> Map -> Map
node x0 x1 = Map $ \free used node -> node x0 x1

gen :: Word32 -> Word32 -> Arr
gen n x = case n of
  0 -> single x
  p -> let x0 = x `shiftL` 1
           x1 = x0 .|. 1
       in concat (gen (p - 1) x0) (gen (p - 1) x1)

rev :: Arr -> Arr
rev a = let a_empty  = empty
            a_single = single
            a_concat = \x0 x1 -> concat (rev x1) (rev x0)
        in matchArr a a_empty a_single a_concat

sumArr :: Arr -> Word32
sumArr a = let a_empty  = 0
               a_single = id
               a_concat = \x0 x1 -> sumArr x0 + sumArr x1
           in matchArr a a_empty a_single a_concat

sort :: Arr -> Arr
sort t = to_arr (to_map t) 0

to_arr :: Map -> Word32 -> Arr
to_arr a = let a_free = \k -> empty
               a_used = \k -> single k
               a_node = \x0 x1 k -> concat (to_arr x0 (k * 2)) (to_arr x1 (k * 2 + 1))
           in matchApp a a_free a_used a_node

to_map :: Arr -> Map
to_map a = let a_empty  = free
               a_single = \x -> radix x
               a_concat = \x0 x1 -> merge (to_map x0) (to_map x1)
           in matchArr a a_empty a_single a_concat

merge :: Map -> Map -> Map
merge a =
  let a_free = \b ->
        let b_free = free
            b_used = used
            b_node = \b0 b1 -> node b0 b1
        in matchApp b b_free b_used b_node
      a_used = \b ->
        let b_free = used
            b_used = used
            b_node = \b0 b1 -> undefined
        in matchApp b b_free b_used b_node
      a_node = \a0 a1 b ->
        let b_free = \a0 a1 -> node a0 a1
            b_used = \a0 a1 -> undefined
            b_node = \b0 b1 a0 a1 -> node (merge a0 b0) (merge a1 b1)
        in matchApp b b_free b_used b_node a0 a1
  in matchApp a a_free a_used a_node

radix :: Word32 -> Map
radix n =
  let r0  = used
      r1  = swap (n .&. 1) r0 free
      r2  = swap (n .&. 2) r1 free
      r3  = swap (n .&. 4) r2 free
      r4  = swap (n .&. 8) r3 free
      r5  = swap (n .&. 16) r4 free
      r6  = swap (n .&. 32) r5 free
      r7  = swap (n .&. 64) r6 free
      r8  = swap (n .&. 128) r7 free
      r9  = swap (n .&. 256) r8 free
      r10 = swap (n .&. 512) r9 free
      r11 = swap (n .&. 1024) r10 free
      r12 = swap (n .&. 2048) r11 free
      r13 = swap (n .&. 4096) r12 free
      r14 = swap (n .&. 8192) r13 free
      r15 = swap (n .&. 16384) r14 free
      r16 = swap (n .&. 32768) r15 free
      r17 = swap (n .&. 65536) r16 free
      r18 = swap (n .&. 131072) r17 free
      r19 = swap (n .&. 262144) r18 free
      r20 = swap (n .&. 524288) r19 free
      r21 = swap (n .&. 1048576) r20 free
      r22 = swap (n .&. 2097152) r21 free
      r23 = swap (n .&. 4194304) r22 free
      r24 = swap (n .&. 8388608) r23 free
  in r24

swap :: Word32 -> Map -> Map -> Map
swap n x0 x1 = case n of
  0 -> node x0 x1
  _ -> node x1 x0

main :: IO ()
main = print $ sumArr (sort (rev (gen 22 0)))
