# Backend

## Testing Expectations

- No use case or route change is complete without tests.
- Unit tests mock the service traits (`mockall`) and live next to the code under test.
- Integration tests in `backend/src/infrastructure/pg/` sit behind the `integration-tests` feature and require the
  container from `make docker-up`, so a plain `cargo test` stays offline.
- Do not merge code that breaks existing tests.

Minimal unit-test checklist:

1. Mock setup for every service trait the use case touches.
2. Input construction, including invalid and boundary values.
3. Assertions on the returned value or the exact error variant.
