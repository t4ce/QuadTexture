//! Retry one operation while its caller retains the same frame lease or fence.

pub fn retry_while<T, E>(
    mut operation: impl FnMut() -> Result<T, E>,
    mut retryable: impl FnMut(&E) -> bool,
    mut yield_once: impl FnMut(),
) -> Result<T, E> {
    loop {
        match operation() {
            Err(error) if retryable(&error) => yield_once(),
            result => return result,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn retries_the_same_pending_operation_until_it_completes() {
        let pending_point = 73u64;
        let mut calls = Vec::new();
        let yields = Cell::new(0);
        let complete = retry_while(
            || {
                calls.push(pending_point);
                if calls.len() < 3 {
                    Err(-16)
                } else {
                    Ok(pending_point)
                }
            },
            |error| *error == -16,
            || yields.set(yields.get() + 1),
        );
        assert_eq!(complete, Ok(73));
        assert_eq!(calls, [73, 73, 73]);
        assert_eq!(yields.get(), 2);
    }

    #[test]
    fn a_fatal_error_after_busy_is_returned_without_further_calls() {
        let mut calls = 0;
        let yields = Cell::new(0);
        let result: Result<(), i32> = retry_while(
            || {
                calls += 1;
                if calls == 1 { Err(-16) } else { Err(-5) }
            },
            |error| *error == -16,
            || yields.set(yields.get() + 1),
        );
        assert_eq!(result, Err(-5));
        assert_eq!(calls, 2);
        assert_eq!(yields.get(), 1);
    }

    #[test]
    fn immediate_completion_needs_no_yield() {
        let result: Result<u32, i32> = retry_while(
            || Ok(11),
            |error| *error == -16,
            || panic!("a completed operation must not yield"),
        );
        assert_eq!(result, Ok(11));
    }
}
