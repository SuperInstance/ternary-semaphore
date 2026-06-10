# ternary-semaphore

Ternary semaphore for GPU resource concurrency control.

## Why This Exists

A standard semaphore tells you "has capacity" or "full." But in GPU scheduling, there's a third state that matters: **overcommitted** — more permits issued than slots, because a high-priority kernel forced its way in. Knowing whether you're at capacity vs. overcommitted drives different scheduling decisions. At capacity means queue the next kernel. Overcommitted means drain before accepting anything new.

This crate implements a counting semaphore with priority queuing, force-acquire for emergencies, and a ternary state signal: `Available (+1)`, `AtCapacity (0)`, `Overcommitted (-1)`.

## Architecture

### Core Types

- **`PermitState`** — Ternary: `Available (+1)`, `AtCapacity (0)`, `Overcommitted (-1)`.
- **`Permit`** — An acquired permit with `id`, `kernel` name, and `priority` (ternary).
- **`TernarySemaphore`** — Tracks `max_permits`, active count, and a priority wait queue.

### Key Behaviors

- **try_acquire**: If capacity exists, issue a permit. Otherwise, queue by priority.
- **force_acquire**: Bypass capacity — emergency kernel gets a permit even if overcommitted.
- **release**: Return a permit. If queued kernels exist, dequeue the highest priority.
- **drain_queue**: Flush all waiting kernels (e.g., during shutdown).

## Usage

```rust
use ternary_semaphore::{TernarySemaphore, PermitState};

let mut sem = TernarySemaphore::new(4); // 4 GPU slots

let p1 = sem.try_acquire("matmul", 1).unwrap();   // priority +1
let p2 = sem.try_acquire("conv2d", 0).unwrap();   // priority 0
assert_eq!(sem.state(), PermitState::Available);

// Fill to capacity
let p3 = sem.try_acquire("layernorm", 1).unwrap();
let p4 = sem.try_acquire("softmax", -1).unwrap();
assert_eq!(sem.state(), PermitState::AtCapacity);

// Emergency bypass
let p5 = sem.force_acquire("critical_kernel");
assert_eq!(sem.state(), PermitState::Overcommitted);

// Release
sem.release();
assert_eq!(sem.utilization(), 1.0); // still at max
```

## API Reference

| Method | Returns | Description |
|--------|---------|-------------|
| `new(max_permits)` | `TernarySemaphore` | Create with N slots |
| `state()` | `PermitState` | Current ternary state |
| `try_acquire(kernel, priority)` | `Option<u64>` | Get permit if available, else queue |
| `force_acquire(kernel)` | `u64` | Emergency: always succeeds |
| `release()` | `Option<Permit>` | Release and maybe dequeue |
| `drain_queue()` | `Vec<Permit>` | Flush all waiting kernels |
| `active_count()` | `usize` | Currently active permits |
| `waiting_count()` | `usize` | Queued kernels |
| `utilization()` | `f64` | active / max ratio |
| `issued()` | `u64` | Total permits ever issued |

## The Deeper Idea

Force-acquire is the GPU scheduling equivalent of **interrupt priority inversion rescue**. When a high-priority kernel (e.g., real-time inference) arrives and all slots are taken by low-priority batch jobs, you have two choices: wait (latency spike) or evict (complexity). Force-acquire takes a third path: issue the permit anyway, accept temporary overcommitment, and let the next release naturally correct. The ternary state tells downstream systems "we're in emergency mode" without requiring a full scheduling overhaul.

## Related Crates

- **ternary-lease** — distributed leases with ternary states
- **ternary-backpressure** — backpressure signals for pipeline stages
- **ternary-rate-limiter** — rate limiting with ternary feedback
