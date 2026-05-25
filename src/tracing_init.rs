//! Shared tracing setup: RFC 3339 timestamps in the local timezone (`TZ` on Unix).

use time::util;
use tracing_subscriber::fmt::time::OffsetTime;

/// Initialize timezone data from `TZ` before any worker threads start.
pub fn refresh_tz_from_env() {
    let _ = util::refresh_tz();
}

/// Cached local offset; call [`refresh_tz_from_env`] first on the main thread.
pub fn local_timer(
) -> Result<OffsetTime<time::format_description::well_known::Rfc3339>, time::error::IndeterminateOffset>
{
    refresh_tz_from_env();
    OffsetTime::local_rfc_3339()
}
