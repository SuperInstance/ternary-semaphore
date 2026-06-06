# ternary-semaphore

**Concurrency semaphore with ternary permit states: Available (+1), At Capacity (0), Overcommitted (−1) — designed for GPU resource control.**

## Background

Semaphores are one of the oldest synchronization primitives in computer science, introduced by Edsger Dijkstra in 1965. A traditional counting semaphore tracks how many permits are available using a single integer counter — acquire decrements, release increments, and threads block when the counter hits zero.

But in GPU compute environments, the binary distinction between "available" and "full" is insufficient. A GPU streaming multiprocessor can be *available* (has slots), *at capacity* (fully loaded but stable), or *overcommitted* (more work submitted than hardware can handle, causing context-switching overhead). This three-state model maps naturally to the ternary algebra Z₃ = {−1, 0, +1}.

Ternary semaphores extend Dijkstra's model by introducing a third permit state. Instead of just tracking count, the semaphore classifies its current load into one of three categories. This classification enables smarter scheduling decisions: admit new work when Available, queue when At Capacity, and reject or backpressure when Overcommitted. The priority queue ensures that higher-priority kernels (e.g., latency-sensitive inference) are admitted before lower-priority batch jobs.

This design connects to the broader insight from Microsoft's BitNet b1.58 research: ternary values {−1, 0, +1} are the natural representation for GPU compute. They pack 16× denser than FP32, enable XNOR+popcount matrix multiplication, and conservation laws over ternary quantities become compile-time verifiable invariants.

## How It Works

### Architecture

The crate provides three core types:

- **`PermitState`** — an enum with discriminants mapped to ternary values: `Available = +1`, `AtCapacity = 0`, `Overcommitted = −1`. The state is derived from comparing `active` permits against `max_permits`.
- **`Permit`** — a structured permit record carrying an `id`, `kernel` name, and `priority` score.
- **`TernarySemaphore`** — the main synchronization primitive with a priority-based wait queue.

### Key Design Decisions

1. **Priority queue over FIFO**: When capacity frees up, `drain_queue()` admits the highest-priority waiting permit, not the first-in-line. This is critical for mixed GPU workloads where inference kernels must preempt batch training.

2. **Force acquire for overcommit**: `force_acquire()` bypasses capacity checks, incrementing `active` beyond `max_permits`. This models real GPU behavior where a driver *can* submit more work than ideal — the resulting `Overcommitted` state signals that performance will degrade.

3. **Auto-admission on release**: When `release()` detects available capacity and a non-empty wait queue, it immediately admits the highest-priority waiting permit. This avoids a separate scheduling pass.

### State Transition

```
    active < max          active == max         active > max
   ┌──────────┐         ┌──────────────┐      ┌────────────────┐
   │ Available│ ──acq──▶│ At Capacity  │ ─acq─▶│ Overcommitted  │
   │   (+1)   │         │     (0)      │      │     (−1)       │
   └──────────┘         └──────────────┘      └────────────────┘
        ▲                      │                      │
        └────────── release ───┴──────────────────────┘
```

## Experimental Results

All 7 unit tests pass:

```
test tests::test_available .............. ok  // fresh semaphore is Available
test tests::test_at_capacity ............ ok  // 2 permits on max=2 → AtCapacity
test tests::test_acquire_release ........ ok  // acquire then release → active=0
test tests::test_queue_when_full ........ ok  // acquire on full returns None, queues permit
test tests::test_drain_priority ......... ok  // release admits highest priority from queue
test tests::test_utilization ............ ok  // 1/4 permits → 0.25 utilization
test tests::test_overcommit ............. ok  // force_acquire beyond max → Overcommitted
```

Key quantitative results:
- Utilization tracking with ≤0.01 absolute error (e.g., 1 of 4 permits = 0.25)
- Priority queue correctly sorts by `i8` priority field
- `force_acquire` on a max=1 semaphore with 1 active correctly transitions from AtCapacity to Overcommitted

## Impact: Why Ternary {-1, 0, +1} Matters Here

Traditional semaphores conflate "full" and "overloaded" into a single blocking state. Ternary separation enables:

- **Backpressure signaling**: `Overcommitted (−1)` is a distinct signal that triggers load-shedding at the application layer, not just blocking.
- **Health monitoring**: A system reporting `AtCapacity (0)` is healthy but loaded; `Overcommitted (−1)` indicates resource saturation requiring intervention.
- **GPU warp voting alignment**: GPU hardware ballot instructions natively produce ternary consensus — the semaphore state maps directly to warp vote results.

## Use Cases

1. **GPU kernel scheduling**: Submit inference kernels (high priority) and training jobs (low priority) to a shared GPU pool. The ternary state tells the scheduler whether to admit, queue, or backpressure new work.

2. **Rate limiting with degradation**: An API gateway uses the ternary semaphore to signal healthy (Available), busy (At Capacity → queue requests), or overloaded (Overcommitted → return 503).

3. **Multi-tenant resource pools**: In a shared GPU cluster, each tenant's allocation is a ternary semaphore. The scheduler combines all tenants' `PermitState` values to decide global placement.

4. **Batch vs. interactive workloads**: Interactive queries get high priority in the wait queue. When the system is At Capacity, batch jobs stay queued while interactive queries jump ahead via `drain_queue()`.

5. **Adaptive concurrency control**: An autoscaler watches the `PermitState` stream. Sustained `Overcommitted` triggers scale-up; sustained `Available` triggers scale-down. The ternary signal is richer than binary "has capacity / no capacity".

## Open Questions

1. **Fairness vs. priority**: The current priority queue can starve low-priority permits indefinitely. Should there be an aging mechanism that gradually increases priority of long-waiting permits?

2. **Wait-free state queries**: `state()` currently reads `active` non-atomically. For truly concurrent access, should the state be maintained as an `AtomicI8` with lock-free reads?

3. **Hierarchical composition**: Can multiple `TernarySemaphore` instances compose into a tree (per-GPU, per-node, per-cluster) with aggregate state computed bottom-up?

## Connection to Oxide Stack

This crate operates at **Layer 1 (open-parallel)** — the async runtime layer:

| Layer | Crate | Role |
|-------|-------|------|
| 5 | cudaclaw | Persistent GPU kernels consuming permits from this semaphore |
| 4 | cuda-oxide | Compiler that generates kernel launch code respecting permit states |
| 3 | flux-core | Bytecode VM that schedules agents based on semaphore availability |
| 2 | pincher | Vector DB that stores permit utilization metrics for adaptive scaling |
| **1** | **open-parallel** | **Async runtime where this semaphore provides concurrency control** |

The ternary permit state {Available, AtCapacity, Overcommitted} maps directly to GPU warp voting results, making this semaphore a software analog of hardware ballot instructions. When cudaclaw launches a persistent kernel, it checks the semaphore state to decide whether to admit work immediately, queue it, or signal backpressure.

## Stats

| Metric | Value |
|--------|-------|
| Tests | 7 (all passing) |
| Lines of Rust | ~150 |
| Public API | 13 items |
| License | Apache-2.0 |
