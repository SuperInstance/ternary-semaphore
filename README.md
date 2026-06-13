# ternary-semaphore

Ternary semaphore for **GPU resource control** with three-state capacity tracking: `+1` (available), `0` (at capacity), and `-1` (overcommitted). Provides priority-queued acquisition, auto-admission on release, and utilization metrics.

## Why It Matters

Binary semaphores (locked/unlocked) can't distinguish "all permits in use" from "system overloaded." Ternary semaphores add an explicit **overcommitted** state, enabling:

| State | Value | Meaning | Action |
|-------|-------|---------|--------|
| Available | `+1` | `active < max` | Grant immediately |
| AtCapacity | `0` | `active == max` | Queue request |
| Overcommitted | `-1` | `active > max` | Alert, reject, or backpressure |

The `force_acquire` path allows controlled overcommit for priority workloads (e.g., kernel launches that must execute), while the priority queue ensures high-priority kernels get admitted first when permits free up.

## How It Works

### State Transitions

```
                  acquire (active < max)
   Available ─────────────────────────→ AtCapacity
   (+1)                                  (0)
       ↑                                    │
       └── release (active < max) ──────────┘
                           │
           force_acquire   │   force_acquire
       (active == max) ────┼────→ Overcommitted
                           │        (-1)
                           │
                      release
```

### Acquisition Protocol

**`try_acquire(kernel, priority)`:**

```
if active < max_permits:
    active += 1
    return Some(permit_id)
else:
    enqueue(permit with priority)
    return None
```

**`force_acquire(kernel)`:**

```
active += 1   // no check — overcommit allowed
return permit_id
```

**`release()`:**

```
active -= 1
if active < max_permits and queue not empty:
    dequeue highest priority → auto-admit
    return Some(admitted_permit)
return None
```

### Priority Queue Admission

`drain_queue()` admits waiting permits in priority order until capacity is full:

```
while active < max_permits and queue not empty:
    find max-priority permit in queue
    admit it
```

Finding the max-priority permit is O(Q) per admission (linear scan). Total: O(Q²) worst case for full drain. This is acceptable for moderate queue sizes; for very large queues, a `BinaryHeap` would reduce to O(Q log Q).

**Complexity:**
- `try_acquire`: O(1)
- `force_acquire`: O(1)
- `release`: O(Q) (priority dequeue)
- `drain_queue`: O(Q²) worst case

### Utilization

```
utilization = active / max_permits
```

At `utilization = 1.0`, the semaphore is AtCapacity. Above 1.0, it's Overcommitted.

## Quick Start

```rust
use ternary_semaphore::{TernarySemaphore, PermitState};

let mut sem = TernarySemaphore::new(4);

assert_eq!(sem.state(), PermitState::Available);

let id = sem.try_acquire("matmul", priority: 0);
assert!(id.is_some());
assert_eq!(sem.active_count(), 1);

// Fill to capacity
for _ in 0..3 { sem.force_acquire("conv"); }
assert_eq!(sem.state(), PermitState::AtCapacity);

// Release auto-admits from queue
sem.try_acquire("queued_kernel", 1);  // gets queued
let next = sem.release();
assert!(next.is_some()); // auto-admitted high-priority waiter
```

## API

### `TernarySemaphore`

| Method | Returns | Description |
|--------|---------|-------------|
| `new(max_permits)` | `Self` | Initialize with capacity |
| `state()` | `PermitState` | Current: Available / AtCapacity / Overcommitted |
| `try_acquire(kernel, priority)` | `Option<u64>` | Acquire if available, else queue |
| `force_acquire(kernel)` | `u64` | Acquire unconditionally (may overcommit) |
| `release()` | `Option<Permit>` | Free a permit; auto-admit next from queue |
| `drain_queue()` | `Vec<Permit>` | Admit all possible queued permits |
| `active_count()` | `usize` | Currently held permits |
| `waiting_count()` | `usize` | Queued requests |
| `utilization()` | `f64` | active / max_permits |

### `Permit`

```rust
pub struct Permit {
    pub id: u64,
    pub kernel: String,
    pub priority: i8,  // -1, 0, +1
}
```

## Architecture Notes

The **γ + η = C** invariant: *generation* (γ) is the permit acquisition process (new work entering the system), *entropy* (η) is the queue depth diversity (how many distinct priority levels are waiting), and *conservation* (C) is the invariant `active ≤ max_permits` under normal operation. The `force_acquire` path deliberately violates C for priority override, entering the `Overcommitted (-1)` state — this is the entropy-overflow valve that prevents deadlock while signaling the violation through the ternary state.

## References

- **Counting semaphores:** Dijkstra, E. W. "Cooperating Sequential Processes" (1965)
- **Priority inversion:** Sha, L., Rajkumar, R. & Lehoczky, J. "Priority Inheritance Protocols" (1990)
- **GPU resource management:** NVIDIA, "CUDA C++ Programming Guide" §7 (occupancy)
- **Overcommit strategies:** Govindan, S. et al. "Cuanta: Quantifying Effects of Scheduler" (2011)

## License

MIT
