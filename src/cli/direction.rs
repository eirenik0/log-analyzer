use crate::parser::{EventDirection, RequestDirection};
use clap::ValueEnum;

/// Common representation for both event and request directions
#[derive(Debug, Clone, PartialEq, ValueEnum)]
pub enum Direction {
    Outgoing, // Represents EventDirection::Emit or RequestDirection::Send
    Incoming, // Represents EventDirection::Receive or RequestDirection::Receive
}

// Implement conversion from EventDirection to Direction
impl From<EventDirection> for Direction {
    fn from(event_direction: EventDirection) -> Self {
        match event_direction {
            EventDirection::Emit => Direction::Outgoing,
            EventDirection::Receive => Direction::Incoming,
        }
    }
}

// Implement conversion from RequestDirection to Direction
impl From<RequestDirection> for Direction {
    fn from(request_direction: RequestDirection) -> Self {
        match request_direction {
            RequestDirection::Send => Direction::Outgoing,
            RequestDirection::Receive => Direction::Incoming,
        }
    }
}

// If needed, you can also implement conversions from Direction back to each specific enum
impl TryFrom<Direction> for EventDirection {
    type Error = &'static str;

    fn try_from(direction: Direction) -> Result<Self, Self::Error> {
        match direction {
            Direction::Outgoing => Ok(EventDirection::Emit),
            Direction::Incoming => Ok(EventDirection::Receive),
        }
    }
}

impl TryFrom<Direction> for RequestDirection {
    type Error = &'static str;

    fn try_from(direction: Direction) -> Result<Self, Self::Error> {
        match direction {
            Direction::Outgoing => Ok(RequestDirection::Send),
            Direction::Incoming => Ok(RequestDirection::Receive),
        }
    }
}
