use crate::zone_event_type::ZoneEventType;
use crate::zone_message::ZoneMessage;

/// A pending zone event that has been queued for delivery to clients.
///
/// Zone events are created when entities within a zone change state (added,
/// removed, animated, etc.) and are consumed during the zone output phase
/// of each engine tick.
///
/// # Fields
///
/// * `event_type` -- Controls delivery scope: [`ZoneEventType::Enclosed`] for
///   broadcast to all observers, [`ZoneEventType::Follows`] for a single receiver.
/// * `receiver37` -- When `event_type` is `Follows`, the lower-37-bit player UID
///   that should receive this event. `None` for enclosed events.
/// * `message` -- The protocol message payload to serialize and send.
/// * `id` -- An optional entity identifier (oid or lid) used by
///   [`Zone::clear_queued_events`](crate::zone::Zone::clear_queued_events) to
///   cancel stale events when an entity is removed or replaced.
#[derive(Debug, Clone)]
pub struct ZoneEvent {
    pub event_type: ZoneEventType,
    pub receiver37: Option<u64>,
    pub message: ZoneMessage,
    pub id: Option<u64>,
}
