#!/usr/bin/env python3
"""Reproduce zkAPI Pedersen balance generators.

Derivation:
  keccak256("zkapi.pedersen.h2c.v1" || label || counter_be32)
  x = digest mod STARK_FIELD_PRIME
  y^2 = x^3 + x + STARK_BETA
  accept the first counter with a square root and select the even y root.
"""

from hashlib import sha3_256

P = int("0800000000000011000000000000000000000000000000000000000000000001", 16)
BETA = int("06f21413efbe40de150e596d72f7a8c5609ad26c15c915c1f4cdfcb99cee9e89", 16)
DOMAIN = b"zkapi.pedersen.h2c.v1"


def sqrt_mod(n: int) -> int | None:
    if n == 0:
        return 0
    if pow(n, (P - 1) // 2, P) != 1:
        return None
    q = P - 1
    s = 0
    while q % 2 == 0:
        s += 1
        q //= 2
    z = 2
    while pow(z, (P - 1) // 2, P) != P - 1:
        z += 1
    m = s
    c = pow(z, q, P)
    t = pow(n, q, P)
    r = pow(n, (q + 1) // 2, P)
    while t != 1:
        i = 1
        t2 = pow(t, 2, P)
        while t2 != 1:
            t2 = pow(t2, 2, P)
            i += 1
        b = pow(c, 1 << (m - i - 1), P)
        m = i
        c = pow(b, 2, P)
        t = (t * c) % P
        r = (r * b) % P
    return r


def derive(label: bytes) -> tuple[int, int, int]:
    for counter in range(1_000_000):
        digest = sha3_256(DOMAIN + label + counter.to_bytes(4, "big")).digest()
        x = int.from_bytes(digest, "big") % P
        rhs = (pow(x, 3, P) + x + BETA) % P
        y = sqrt_mod(rhs)
        if y is not None and y != 0:
            if y & 1:
                y = P - y
            return counter, x, y
    raise RuntimeError(f"no curve point found for {label!r}")


for label in (b"zkapi.bal.g", b"zkapi.bal.h"):
    counter, x, y = derive(label)
    print(f"{label.decode()} counter={counter}")
    print(f"x=0x{x:064x}")
    print(f"y=0x{y:064x}")
