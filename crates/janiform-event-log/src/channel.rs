use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use crossbeam_channel::TrySendError;

use crate::error::EventLogError;
use crate::event::EventDeliveryPriority;
use crate::event::RunEvent;

/// Result of delivering an event through a bounded channel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventDeliveryOutcome {
    Delivered,
    DroppedVolatile,
}

/// Sender wrapper that enforces Janiform's required versus droppable event policy.
#[derive(Clone)]
pub struct RunEventSender {
    sender: Sender<RunEvent>,
}

impl RunEventSender {
    /// Send an event without blocking hot paths.
    pub fn try_send(&self, event: RunEvent) -> Result<EventDeliveryOutcome, EventLogError> {
        let event_kind = event.kind_name();
        let priority = event.priority;

        match self.sender.try_send(event) {
            Ok(()) => Ok(EventDeliveryOutcome::Delivered),
            Err(TrySendError::Full(_)) => match priority {
                EventDeliveryPriority::DroppableVolatile => {
                    Ok(EventDeliveryOutcome::DroppedVolatile)
                }
                EventDeliveryPriority::Required => {
                    Err(EventLogError::RequiredEventBackpressure { event_kind })
                }
            },
            Err(TrySendError::Disconnected(_)) => {
                Err(EventLogError::EventReceiverClosed { event_kind })
            }
        }
    }
}

/// Create a bounded channel for run events.
pub fn bounded_event_channel(
    capacity: usize,
) -> Result<(RunEventSender, Receiver<RunEvent>), EventLogError> {
    if capacity == 0 {
        return Err(EventLogError::EmptyChannelCapacity);
    }

    let (sender, receiver) = crossbeam_channel::bounded(capacity);

    Ok((RunEventSender { sender }, receiver))
}
