pub mod output_cache;
pub mod user_cache;

/// The "interval" for caching data.
///
/// This is not represented as a time interval, but a denominator for the current amount of chunks.
/// For example, see [`utils::export::fetch_chunks`]:
///
/// ```
/// if curr_chunk.is_multiple_of(CACHE_INTERVAL) {
///     cache_tx.send(token.clone()).await?;
/// }
/// ```
///
/// Since 1 chunk = 100 messages, this will run the cache every 10.000 messages.
pub const CACHE_INTERVAL: u64 = 100;
