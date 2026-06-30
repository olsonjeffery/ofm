# OMP Subprocess Integration Notes for AI Agents

## Threading

- **Avoid `std::thread::spawn`.** This project runs inside a tokio runtime; spawning
  raw OS threads bypasses tokio's scheduling and can cause issues with resource
  tracking, test flakiness, and runtime shutdown.
- Prefer `tokio::spawn` for lightweight async tasks that run on the tokio
  runtime's worker threads.
- When blocking I/O is unavoidable (e.g. reading from a PTY), use
  `tokio::task::spawn_blocking`. The blocking task reads from the I/O source
  and sends events through an `mpsc::Sender` via `blocking_send`. See
  `spawn_reader` in `src/omp/mod.rs` for a concrete example.
