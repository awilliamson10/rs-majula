/// Specifies how a zone event is delivered to clients.
///
/// Zone events fall into two delivery categories based on their intended
/// audience. The engine uses this distinction when building per-player
/// update packets each tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneEventType {
    /// The event is broadcast to every player observing this zone.
    ///
    /// Enclosed events are batched into a shared byte buffer via
    /// [`Zone::compute_shared`](crate::zone::Zone::compute_shared) so they
    /// can be written once and sent to all observers.
    Enclosed,
    /// The event is sent only to a specific receiver (identified by `receiver37`).
    ///
    /// Follows events target a single player -- for example, a privately
    /// visible obj that only its owner can see. These are filtered per-player
    /// in [`Zone::visible_follows_events`](crate::zone::Zone::visible_follows_events).
    Follows,
}
