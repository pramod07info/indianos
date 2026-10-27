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

/// Calculate the earliest retry time with linear backoff. A running/stopped
/// service cannot allocate a restart timer; overflow denies scheduling.
pub fn retry_deadline(service: Service, now_ticks: u64, interval_ticks: u64)
    -> Result<u64, ServiceError>
{
    if service.state != State::WaitingRestart || interval_ticks == 0 {
        return Err(ServiceError::InvalidTransition);
    }
    if service.retries >= service.retry_budget {
        return Err(ServiceError::InvalidTransition);
    }
    let next = u64::from(service.retries) + 1;
    let delay = interval_ticks.checked_mul(next).ok_or(ServiceError::CounterOverflow)?;
    now_ticks.checked_add(delay).ok_or(ServiceError::CounterOverflow)
}
#[cfg(test)]
mod backoff_tests {
    use super::*;
    fn ready() -> Service {
        Service { state: State::WaitingRestart, retries: 1, retry_budget: 3, generation: 4 }
    }
    #[test]
    fn backoff_grows_with_retry_count() {
        assert_eq!(retry_deadline(ready(), 100, 5), Ok(110));
        let mut s = ready(); s.retries = 2;
        assert_eq!(retry_deadline(s, 100, 5), Ok(115));
    }
    #[test]
    fn cannot_retry_stopped_or_exhausted_service() {
        let mut s = ready(); s.state = State::Stopped;
        assert_eq!(retry_deadline(s, 100, 5), Err(ServiceError::InvalidTransition));
        s.state = State::WaitingRestart; s.retries = 3;
        assert_eq!(retry_deadline(s, 100, 5), Err(ServiceError::InvalidTransition));
    }
    #[test]
    fn prevents_timestamp_wraparound() {
        assert_eq!(retry_deadline(ready(), u64::MAX, 1), Err(ServiceError::CounterOverflow));
        assert_eq!(retry_deadline(ready(), 100, 0), Err(ServiceError::InvalidTransition));
    }
}


/// Permit one bounded service restart only after an explicit scheduler
/// deadline and exact generation match; no real process is launched.
pub fn retry_if_due(service:Service,observed_generation:u32,
    now_ticks:u64,deadline_ticks:u64)->Result<Service,ServiceError>{
    if service.state!=State::WaitingRestart||
       service.generation==0||
       observed_generation!=service.generation||
       now_ticks<deadline_ticks||
       service.retries>=service.retry_budget{
        return Err(ServiceError::InvalidTransition)
    }
    transition(service,Event::Retry)
}
#[cfg(test)]
mod retry_gate_tests{
    use super::*;
    fn waiting()->Service{
        Service{state:State::WaitingRestart,retries:0,
                retry_budget:2,generation:11}
    }
    #[test]fn exact_deadline_rotates_generation(){
        let restarted=retry_if_due(waiting(),11,150,150).unwrap();
        assert_eq!(restarted.state,State::Running);
        assert_eq!(restarted.generation,12);
        assert_eq!(restarted.retries,1);
    }
    #[test]fn early_or_stale_restart_is_blocked(){
        assert_eq!(retry_if_due(waiting(),11,149,150),
                   Err(ServiceError::InvalidTransition));
        assert_eq!(retry_if_due(waiting(),10,150,150),
                   Err(ServiceError::InvalidTransition));
        assert_eq!(retry_if_due(waiting(),0,150,150),
                   Err(ServiceError::InvalidTransition));
    }
    #[test]fn exhausted_and_generation_wrap_are_rejected(){
        let mut s=waiting();s.retries=2;
        assert_eq!(retry_if_due(s,11,150,150),Err(ServiceError::InvalidTransition));
        s.retries=0;s.state=State::Exhausted;
        assert_eq!(retry_if_due(s,11,150,150),Err(ServiceError::InvalidTransition));
        s.state=State::WaitingRestart;s.generation=u32::MAX;
        assert_eq!(retry_if_due(s,u32::MAX,150,150),Err(ServiceError::CounterOverflow));
    }
}
