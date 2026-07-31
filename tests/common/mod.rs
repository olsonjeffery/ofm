/// Drop guard that drains the process-wide opencode server pool when a test
/// finishes (including on panic unwind). Without this, pooled `opencode
/// serve` subprocesses are reparented to PID 1 and leak, because the pool
/// is a `static` that is never dropped in test binaries.
pub struct PoolCleanupGuard;

impl Drop for PoolCleanupGuard {
    fn drop(&mut self) {
        ofm::opencode_sdk::pool::OpenCodeServerPool::instance().kill_all_sync();
    }
}

pub fn pool_cleanup_guard() -> PoolCleanupGuard {
    PoolCleanupGuard
}
