//! Pure, bounded service restart transitions, not a process launcher.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State { Stopped, Running, WaitingRestart, Exhausted }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event { Start, Crash, Retry, Stop }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Service {
    pub state: State, pub retries: u8, pub retry_budget: u8, pub generation: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceError { InvalidTransition, CounterOverflow }

pub fn transition(mut service: Service, event: Event) -> Result<Service, ServiceError> {
    match (service.state, event) {
        (_, Event::Stop) => service.state = State::Stopped,
        (State::Stopped, Event::Start) => {
            service.generation = service.generation.checked_add(1)
                .ok_or(ServiceError::CounterOverflow)?;
            service.retries = 0;
            service.state = State::Running;
        }
        (State::Running, Event::Crash) => {
            service.state = if service.retries < service.retry_budget {
                State::WaitingRestart
            } else { State::Exhausted };
        }
        (State::WaitingRestart, Event::Retry) => {
            service.retries = service.retries.checked_add(1)
                .ok_or(ServiceError::CounterOverflow)?;
            service.generation = service.generation.checked_add(1)
                .ok_or(ServiceError::CounterOverflow)?;
            service.state = State::Running;
        }
        _ => return Err(ServiceError::InvalidTransition),
    }
    Ok(service)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn base() -> Service {
        Service { state: State::Stopped, retries: 0, retry_budget: 1, generation: 0 }
    }
    #[test]
    fn restart_is_bounded_and_increments_generation() {
        let a = transition(base(), Event::Start).unwrap();
        assert_eq!(a.generation, 1);
        let b = transition(a, Event::Crash).unwrap();
        assert_eq!(b.state, State::WaitingRestart);
        let c = transition(b, Event::Retry).unwrap();
        assert_eq!((c.retries, c.generation), (1, 2));
        assert_eq!(transition(c, Event::Crash).unwrap().state, State::Exhausted);
    }
    #[test]
    fn invalid_transition_is_rejected() {
        assert_eq!(transition(base(), Event::Retry), Err(ServiceError::InvalidTransition));
        assert_eq!(transition(base(), Event::Crash), Err(ServiceError::InvalidTransition));
    }
    #[test]
    fn generation_overflow_does_not_start_service() {
        let mut s = base(); s.generation = u32::MAX;
        assert_eq!(transition(s, Event::Start), Err(ServiceError::CounterOverflow));
        assert_eq!(transition(s, Event::Stop).unwrap().state, State::Stopped);
    }
}
